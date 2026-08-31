// Author: Lars Op den Kamp (lars@opdenkamp-it.nl)
// Copyright (c) 2026 Op den Kamp IT Solutions

//! The wire format: frame header, identifiers, opcodes and enumerations.
//!
//! The frame constructor, the socket write and the transmit path that joins
//! them all live here rather than on the public surface, and the first two are
//! reachable only from inside this module. Every frame is therefore built by
//! one function and written by one other, with the per-opcode protocol-floor
//! check between them: a send that skips the check cannot be written, because
//! there is nothing outside this module for it to call.
//!
//! The payload builders are the exception, and are not one: they assemble
//! bytes and have no way to reach a socket, so a caller holding one still has
//! to hand it to the transmit path to get it sent.

mod bayconfig;
mod conn;
mod constants;
mod enums;
mod frame;
mod netif;
mod opcode;
mod payload;
mod tx;
mod uid;

#[cfg(test)]
mod vectors;

pub use constants::*;
pub use enums::{
    BayFeatures, BayStatus, DeviceFeature, EdidProfile, FirmwareType, LinkFeature,
    MultiviewerAspectRatio, MultiviewerBool, MultiviewerEdidTemplate, MultiviewerHdcpMode,
    MultiviewerItcMode, MultiviewerOutputMode, MultiviewerPipPosition, MultiviewerPipSize,
    MultiviewerSource, MultiviewerViewMode, MxrSignalType, RcAction, RcKey, RcType, UtpLinkSpeed,
};
pub use netif::valid_addresses;
pub use tx::SendError;
pub use uid::{BayUid, DeviceUid, UidParseError};

pub(crate) use bayconfig::{parse_bay_config, BayConfig, BAY_CONFIG_SIZE};
pub(crate) use conn::Conn;
pub(crate) use frame::{cstr, Frame, DEVICE_NAME_LEN, FW_VERSION_LEN};
pub(crate) use netif::broadcast_address;
pub(crate) use opcode::{audio_sub, mv_sub, op, Opcode};
pub(crate) use payload::*;
pub(crate) use tx::{Addressee, ProtocolTarget, Tx};

// Reached only from the test modules elsewhere in the crate, so they are cut
// from a release build rather than carried there behind an allow.
#[cfg(test)]
pub(crate) use frame::HEADER_LEN;
// Read only by the socket tests, which are Linux-only.
#[cfg(all(test, target_os = "linux"))]
pub(crate) use netif::default_local_ip;
#[cfg(test)]
pub(crate) use opcode::protocol_for;
