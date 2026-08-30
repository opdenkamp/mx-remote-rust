// Author: Lars Op den Kamp (lars@opdenkamp-it.nl)
// Copyright (c) 2026 Op den Kamp IT Solutions

//! The transmit path: the protocol-floor gate, and the only way through it.

use std::fmt;
use std::io;

use super::conn::Conn;
use super::frame::build_frame;
use super::opcode::{stamp_for, Opcode};
use super::uid::DeviceUid;

/// Why a frame was not sent.
#[derive(Debug)]
#[non_exhaustive]
pub enum SendError {
    /// The addressed device speaks a protocol older than the opcode requires.
    ///
    /// A receiver silently drops any frame stamped above its own version, with
    /// no NAK, so sending anyway would report success and change nothing.
    ProtocolTooOld {
        /// Serial number of the addressed device.
        serial: String,
        /// The opcode that was refused.
        opcode: u16,
        /// The protocol version the device reports.
        have: u16,
        /// The version the opcode requires.
        need: u16,
    },
    /// The client is not connected, because it was never started or has been
    /// closed.
    NotConnected,
    /// The socket write failed.
    Io(io::Error),
}

impl fmt::Display for SendError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ProtocolTooOld {
                serial,
                opcode,
                have,
                need,
            } => write!(
                f,
                "{serial} reports protocol {have:#04x}, opcode {opcode:#04x} needs {need:#04x}"
            ),
            Self::NotConnected => write!(f, "not connected"),
            Self::Io(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for SendError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(e) => Some(e),
            _ => None,
        }
    }
}

impl From<io::Error> for SendError {
    fn from(e: io::Error) -> Self {
        Self::Io(e)
    }
}

/// What the protocol floor is checked against.
///
/// A device reports the highest protocol version it can decode in its hello.
pub(crate) trait ProtocolTarget {
    /// Serial number, for the error message.
    fn serial(&self) -> &str;
    /// The version the device reports, or zero when it has not said.
    fn supported_protocol(&self) -> u16;
}

/// Who a frame is addressed to.
///
/// Naming the recipient is a separate decision from building the payload,
/// because the addressed device is a payload uid at an offset that differs per
/// opcode while the opcode itself sits at a fixed place in the header. This is
/// an enumeration rather than an `Option` so that a frame with no single
/// recipient says so, instead of looking like one whose sender did not bother.
#[derive(Clone, Debug)]
pub(crate) enum Addressee {
    /// Addressed to one device, whose protocol floor is checked.
    Device {
        /// Serial number, for the error message.
        serial: String,
        /// The version the device reports.
        protocol: u16,
    },
    /// No single recipient. Only discovery, hello and the monitoring pulse.
    Broadcast,
}

impl Addressee {
    /// Addresses a frame to a device.
    pub(crate) fn device(target: &dyn ProtocolTarget) -> Self {
        Self::Device {
            serial: target.serial().to_owned(),
            protocol: target.supported_protocol(),
        }
    }
}

/// The transmit side of a client: the socket, and this client's own identifier.
///
/// [`Tx::send`] is the only path from an opcode to the wire. It is the gate
/// that refuses a frame the target cannot decode, and it can be the gate
/// because the two things it sits between - the frame constructor and the
/// socket write - are both private to this module and unreachable from
/// anywhere else in the crate.
/// A tap on the transmit path, for a test reading back what would go on the
/// wire.
#[cfg(test)]
pub(crate) type TxTap = std::sync::Arc<dyn Fn(&[u8]) + Send + Sync>;

#[derive(Default)]
pub(crate) struct Tx {
    conn: Option<std::sync::Arc<Conn>>,
    /// Captures each frame that passes the gate. Frames are assembled inside
    /// the method that sends them and cannot be reached any other way, so this
    /// is how a test reads back what would go on the wire.
    #[cfg(test)]
    tap: Option<TxTap>,
}

impl Tx {
    /// Replaces the socket.
    pub(crate) fn set_conn(&mut self, conn: Option<Conn>) {
        self.conn = conn.map(std::sync::Arc::new);
    }

    /// A handle on the socket that outlives this lock.
    ///
    /// The receive thread parks in the kernel for as long as its read timeout,
    /// and must not hold the transmit lock while it does or every send would
    /// wait behind it. Holding a share of the socket instead also settles what
    /// a reconfiguration does to a thread already reading: the old socket stays
    /// open until that read returns, so its descriptor cannot be reissued to
    /// something else underneath it.
    pub(crate) fn conn(&self) -> Option<std::sync::Arc<Conn>> {
        self.conn.clone()
    }

    #[cfg(test)]
    pub(crate) fn set_tap(&mut self, tap: TxTap) {
        self.tap = Some(tap);
    }

    /// Builds a frame and writes it to the wire, unless `to` cannot decode it.
    ///
    /// The frame is stamped with the version the opcode itself needs rather
    /// than the version this library speaks, so a device that caps lower still
    /// accepts every opcode it does understand. The gate compares against that
    /// same stamp: a receiver drops what is stamped above its own version, so
    /// checking anything else would leave the hole the gate exists to close.
    pub(crate) fn send(
        &self,
        to: &Addressee,
        uid: DeviceUid,
        opcode: Opcode,
        payload: &[u8],
    ) -> Result<usize, SendError> {
        let need = stamp_for(opcode);
        if let Addressee::Device { serial, protocol } = to {
            // A device that has not reported a version is let through: not
            // knowing is not the same as knowing it is too old.
            if *protocol != 0 && *protocol < need {
                return Err(SendError::ProtocolTooOld {
                    serial: serial.clone(),
                    opcode: opcode.0,
                    have: *protocol,
                    need,
                });
            }
        }

        let frame = build_frame(uid, opcode, need, payload);
        #[cfg(test)]
        if let Some(tap) = &self.tap {
            tap(&frame);
        }
        let conn = self.conn.as_ref().ok_or(SendError::NotConnected)?;
        Ok(conn.send(&frame)?)
    }
}
