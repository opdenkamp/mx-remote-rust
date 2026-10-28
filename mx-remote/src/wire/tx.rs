// Author: Lars Op den Kamp (lars@opdenkamp-it.nl)
// Copyright (c) 2026 Op den Kamp IT Solutions

//! The transmit path: the protocol-floor gate, and the only way through it.

use std::fmt;
use std::io;

use super::conn::{Conn, Path};
use super::frame::build_frame;
use super::opcode::{op, stamp_for, Opcode};
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
    /// The opcode has no entry in the protocol table, so there is no version
    /// to stamp it with.
    ///
    /// Nothing this library sends can reach this: every opcode it declares has
    /// an entry, and a test holds that true. It exists so that adding one
    /// without an entry fails at the send rather than going out stamped with a
    /// guess.
    UnknownOpcode {
        /// The opcode that has no entry.
        opcode: u16,
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
            Self::UnknownOpcode { opcode } => {
                write!(f, "opcode {opcode:#04x} has no protocol table entry")
            }
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
        /// The path the device is heard by, for a frame that goes by that
        /// endpoint alone; `None` for one that goes as any other does.
        heard_by: Option<Path>,
    },
    /// No single recipient: discovery, hello, the monitoring pulse and the
    /// signal-status request that asks every bay on the network to report.
    Broadcast,
}

impl Addressee {
    /// Addresses a frame to a device.
    pub(crate) fn device(target: &dyn ProtocolTarget) -> Self {
        Self::Device {
            serial: target.serial().to_owned(),
            protocol: target.supported_protocol(),
            heard_by: None,
        }
    }

    /// Addresses a frame to a device by the one endpoint it is heard by, as a
    /// unit sends a frame meant for one unit alone (`MX_TX_DIRECT`).
    pub(crate) fn direct(target: &dyn ProtocolTarget, heard_by: Path) -> Self {
        Self::Device {
            serial: target.serial().to_owned(),
            protocol: target.supported_protocol(),
            heard_by: Some(heard_by),
        }
    }
}

/// Which of the two endpoints a frame leaves by.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Endpoints {
    /// The discovery group alone, or the one endpoint a client has.
    Multicast,
    /// The broadcast endpoint alone, or the one endpoint a client has.
    Broadcast,
    /// Both, where both are bound.
    Both,
}

/// The endpoints a frame goes by, as a unit picks them.
///
/// A frame for every unit at once goes to both (`MX_TX_ALL`), and one for a
/// device by the endpoint it is heard by (`MX_TX_DIRECT`). Any other goes to
/// the group, and to the broadcast endpoint as well while `broadcast_needed`
/// says some peer needs it (`MX_TX_ACTIVE`).
pub(crate) fn endpoints_for(
    to: &Addressee,
    opcode: Opcode,
    payload: &[u8],
    broadcast_needed: bool,
) -> Endpoints {
    match to {
        Addressee::Device {
            heard_by: Some(Path::Broadcast),
            ..
        } => Endpoints::Broadcast,
        Addressee::Device {
            heard_by: Some(Path::Multicast),
            ..
        } => Endpoints::Multicast,
        _ if broadcast_needed || for_every_unit(opcode, payload) => Endpoints::Both,
        _ => Endpoints::Multicast,
    }
}

/// Whether a unit sends this frame to both endpoints whatever its peers need
/// (`MX_TX_ALL`): a discover, reboot or factory reset of every unit - an
/// empty payload, or the single byte 0xFF - the monitoring pulse, the bay
/// detection and the FPGA upgrade. One naming a unit goes as any other frame.
///
/// The hello is not among them: it reaches the broadcast endpoint on a
/// clock of its own, which [`Tx::send`]'s caller keeps.
fn for_every_unit(opcode: Opcode, payload: &[u8]) -> bool {
    let to_all = matches!(payload, [] | [0xFF]);
    match opcode {
        op::SYS_DISCOVER => payload.is_empty(),
        op::SYS_REBOOT | op::SYS_FACTORY_RESET => to_all,
        op::SYS_MONITORING_PULSE | op::V2IP_DETECT_BAYS | op::V2IP_UPGRADE_FPGA => true,
        _ => false,
    }
}

/// A tap on the transmit path.
#[cfg(test)]
pub(crate) type TxTap = std::sync::Arc<dyn Fn(&[u8]) + Send + Sync>;

/// The transmit side of a client: the socket, and this client's own identifier.
///
/// [`Tx::send`] is the only path from an opcode to the wire. It is the gate
/// that refuses a frame the target cannot decode, and it can be the gate
/// because the two things it sits between - the frame constructor and the
/// socket write - are both private to this module and unreachable from
/// anywhere else in the crate.
#[derive(Default)]
pub(crate) struct Tx {
    conn: Option<std::sync::Arc<Conn>>,
    /// Captures each frame that passes the gate. Frames are assembled inside
    /// the method that sends them and cannot be reached any other way, so this
    /// is how a test reads back what would go on the wire.
    #[cfg(test)]
    tap: Option<TxTap>,
    /// The endpoints each frame that passed the gate was meant for.
    #[cfg(test)]
    pub(crate) sent_on: Vec<Endpoints>,
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
    ///
    /// `broadcast_needed` says some peer has to be reached by broadcast too;
    /// [`endpoints_for`] decides what that means for this frame.
    pub(crate) fn send(
        &mut self,
        to: &Addressee,
        uid: DeviceUid,
        opcode: Opcode,
        payload: &[u8],
        broadcast_needed: bool,
    ) -> Result<usize, SendError> {
        let need = stamp_for(opcode).ok_or(SendError::UnknownOpcode { opcode: opcode.0 })?;
        if let Addressee::Device {
            serial, protocol, ..
        } = to
        {
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

        let endpoints = endpoints_for(to, opcode, payload, broadcast_needed);
        let frame = build_frame(uid, opcode, need, payload);
        #[cfg(test)]
        {
            if let Some(tap) = &self.tap {
                tap(&frame);
            }
            self.sent_on.push(endpoints);
        }
        let conn = self.conn.as_ref().ok_or(SendError::NotConnected)?;
        Ok(conn.send(&frame, endpoints)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Unit;

    impl ProtocolTarget for Unit {
        fn serial(&self) -> &str {
            "EP0001"
        }

        fn supported_protocol(&self) -> u16 {
            0x31
        }
    }

    fn active(opcode: Opcode, payload: &[u8], broadcast_needed: bool) -> Endpoints {
        endpoints_for(&Addressee::Broadcast, opcode, payload, broadcast_needed)
    }

    /// A frame to every unit at once goes to both endpoints: a discover, a
    /// reboot or factory reset of every unit, the pulse, the bay detection
    /// and the FPGA upgrade.
    #[test]
    fn a_frame_for_every_unit_goes_to_both() {
        for (opcode, payload) in [
            (op::SYS_DISCOVER, &[][..]),
            (op::SYS_REBOOT, &[][..]),
            (op::SYS_REBOOT, &[0xFF][..]),
            (op::SYS_FACTORY_RESET, &[][..]),
            (op::SYS_FACTORY_RESET, &[0xFF][..]),
            (op::SYS_MONITORING_PULSE, &[][..]),
            (op::V2IP_DETECT_BAYS, &[][..]),
            (op::V2IP_UPGRADE_FPGA, &[][..]),
        ] {
            assert_eq!(
                active(opcode, payload, false),
                Endpoints::Both,
                "{opcode:?} {payload:?}"
            );
        }
    }

    /// A discover, reboot or factory reset naming one unit, and a hello, go
    /// as any other frame: to the group, and to broadcast too only while a
    /// peer needs it.
    #[test]
    fn a_frame_naming_one_unit_goes_as_any_other() {
        let uid = [0x5A; 16];
        for (opcode, payload) in [
            (op::SYS_DISCOVER, &uid[..]),
            (op::SYS_REBOOT, &uid[..]),
            (op::SYS_FACTORY_RESET, &uid[..]),
            (op::SYS_HELLO, &[0; 54][..]),
            (op::MESH_OPERATION, &[0; 40][..]),
        ] {
            assert_eq!(
                active(opcode, payload, false),
                Endpoints::Multicast,
                "{opcode:?}"
            );
            assert_eq!(active(opcode, payload, true), Endpoints::Both, "{opcode:?}");
        }
    }

    /// A frame for one device alone goes by the one endpoint it is heard by,
    /// whatever other peers need.
    #[test]
    fn a_direct_frame_goes_by_the_devices_own_endpoint() {
        for needed in [false, true] {
            let by =
                |path| endpoints_for(&Addressee::direct(&Unit, path), op::SYS_PING, &[], needed);
            assert_eq!(by(Path::Broadcast), Endpoints::Broadcast);
            assert_eq!(by(Path::Multicast), Endpoints::Multicast);
        }
    }
}
