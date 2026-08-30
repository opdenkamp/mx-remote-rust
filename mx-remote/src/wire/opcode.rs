// Author: Lars Op den Kamp (lars@opdenkamp-it.nl)
// Copyright (c) 2026 Op den Kamp IT Solutions

//! Frame opcodes and the protocol version each one requires.

use super::constants::PROTOCOL_VERSION;

/// The opcode field of a frame header.
///
/// A newtype so that it cannot be confused with the protocol version beside it
/// in a frame header, and so an opcode this library has no name for still
/// round-trips.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct Opcode(pub(crate) u16);

/// Every opcode this library names.
///
/// Opcodes 0x25 and 0x33 are retired and reserved. They were live in an earlier
/// generation, so an old unit would decode a frame sent on one as whatever it
/// used to be - never reuse them.
pub(crate) mod op {
    use super::Opcode;

    pub(crate) const SYS_HELLO: Opcode = Opcode(0x00);
    pub(crate) const SYS_DISCOVER: Opcode = Opcode(0x01);
    pub(crate) const SYS_BAY_CONFIG: Opcode = Opcode(0x02);
    pub(crate) const SYS_LINKS: Opcode = Opcode(0x03);
    pub(crate) const DEV_CONNECT: Opcode = Opcode(0x04);
    pub(crate) const DEV_POWER_CHANGE: Opcode = Opcode(0x05);
    pub(crate) const DEV_SIGNAL_OLD: Opcode = Opcode(0x06);
    pub(crate) const DEV_EDID: Opcode = Opcode(0x07);
    pub(crate) const MX_ROUTE: Opcode = Opcode(0x08);
    pub(crate) const MX_SET_ROUTE: Opcode = Opcode(0x09);
    pub(crate) const RC_IR: Opcode = Opcode(0x0A);
    pub(crate) const RC_KEY: Opcode = Opcode(0x0B);
    pub(crate) const RC_TX_KEY: Opcode = Opcode(0x0C);
    pub(crate) const RC_ACTION: Opcode = Opcode(0x0D);
    pub(crate) const RC_TX_ACTION: Opcode = Opcode(0x0E);
    pub(crate) const AUDIO_VOLUME_UP: Opcode = Opcode(0x0F);
    pub(crate) const AUDIO_VOLUME_DOWN: Opcode = Opcode(0x10);
    pub(crate) const AUDIO_CLIP: Opcode = Opcode(0x11);
    pub(crate) const AUDIO_VOLUME_MUTE: Opcode = Opcode(0x12);
    pub(crate) const AUDIO_SET_ROUTE: Opcode = Opcode(0x13);
    pub(crate) const AUDIO_SET_VOLUME: Opcode = Opcode(0x14);
    pub(crate) const SYS_TEMPERATURE: Opcode = Opcode(0x15);
    pub(crate) const PDU_STATE: Opcode = Opcode(0x16);
    pub(crate) const V2IP_SOURCE_SWITCH: Opcode = Opcode(0x1F);
    pub(crate) const V2IP_LINK_REMOTE: Opcode = Opcode(0x20);
    pub(crate) const V2IP_DETECT_BAYS: Opcode = Opcode(0x21);
    pub(crate) const CHANGE_BAY_NAME: Opcode = Opcode(0x22);
    pub(crate) const SYS_BAY_CONFIG_SECONDARY: Opcode = Opcode(0x23);
    pub(crate) const V2IP_MANUAL_SRC_SWITCH: Opcode = Opcode(0x24);
    pub(crate) const SYS_BAY_V2IP_SOURCES: Opcode = Opcode(0x26);
    pub(crate) const BAY_HIDE: Opcode = Opcode(0x27);
    pub(crate) const SYS_REBOOT: Opcode = Opcode(0x28);
    pub(crate) const NET_LINK_STATUS: Opcode = Opcode(0x29);
    pub(crate) const FIRMWARE_VERSION: Opcode = Opcode(0x2A);
    pub(crate) const SYS_MONITORING_PULSE: Opcode = Opcode(0x2B);
    pub(crate) const V2IP_UPGRADE_FPGA: Opcode = Opcode(0x2C);
    pub(crate) const V2IP_BLIST_REGISTER: Opcode = Opcode(0x2E);
    pub(crate) const V2IP_BLIST_UNREGISTER: Opcode = Opcode(0x2F);
    pub(crate) const TOPOLOGY: Opcode = Opcode(0x30);
    pub(crate) const BAY_SIGNAL_STATUS: Opcode = Opcode(0x31);
    pub(crate) const BAY_MIRROR_STATUS: Opcode = Opcode(0x32);
    pub(crate) const BAY_EDID_PROFILE: Opcode = Opcode(0x34);
    pub(crate) const SETUP_STATUS: Opcode = Opcode(0x35);
    pub(crate) const SET_MASTER: Opcode = Opcode(0x36);
    pub(crate) const SET_INSTALLER: Opcode = Opcode(0x37);
    pub(crate) const BAY_FILTER_STATUS: Opcode = Opcode(0x38);
    pub(crate) const BAY_STATUS: Opcode = Opcode(0x39);
    pub(crate) const SYS_FACTORY_RESET: Opcode = Opcode(0x3A);
    pub(crate) const MESH_OPERATION: Opcode = Opcode(0x3B);
    pub(crate) const V2IP_DEVICE_CFG: Opcode = Opcode(0x3C);
    pub(crate) const AMP_ZONE_SETTINGS: Opcode = Opcode(0x3D);
    pub(crate) const AMP_DOLBY_STATE: Opcode = Opcode(0x3E);
    pub(crate) const V2IP_STATS: Opcode = Opcode(0x3F);
    pub(crate) const V2IP_TILING: Opcode = Opcode(0x40);
    pub(crate) const V2IP_POWER_SAVE: Opcode = Opcode(0x41);
    pub(crate) const V2IP_MULTIVIEWER: Opcode = Opcode(0x42);
    pub(crate) const V2IP_AUDIO: Opcode = Opcode(0x43);
    pub(crate) const V2IP_BAY_MAPPINGS: Opcode = Opcode(0x44);
    pub(crate) const RC_SETTINGS: Opcode = Opcode(0x45);
    pub(crate) const SYS_STATUS: Opcode = Opcode(0x46);
    pub(crate) const DEBUG: Opcode = Opcode(0x47);
    pub(crate) const RC_IR_TX: Opcode = Opcode(0x48);
    pub(crate) const V2IP_VIDEO_WALL: Opcode = Opcode(0x49);
}

/// Returns the header protocol version to stamp on a frame carrying `opcode`.
///
/// This is the minimum version a receiver needs to decode the opcode, as the
/// firmware's opcode table declares it, not the version this library speaks. A
/// receiver drops any frame whose header protocol exceeds its own cap, so
/// stamping [`PROTOCOL_VERSION`] on everything would make a device with a lower
/// cap - a ProAmp8 caps at 0x22 - silently drop every frame we send.
///
/// These stay deliberately low. An opcode whose payload only ever grew trailing
/// fields keeps its original version, and a receiver tells the formats apart by
/// payload length instead.
pub(crate) fn protocol_for(opcode: Opcode) -> u16 {
    match opcode {
        op::SYS_HELLO => 0x01,
        op::SYS_DISCOVER => 0x01,
        op::SYS_BAY_CONFIG => 0x01,
        op::SYS_LINKS => 0x01,
        op::DEV_CONNECT => 0x1B,
        op::DEV_POWER_CHANGE => 0x01,
        op::DEV_SIGNAL_OLD => 0x01,
        op::DEV_EDID => 0x01,
        op::MX_ROUTE => 0x01,
        op::MX_SET_ROUTE => 0x01,
        op::RC_IR => 0x19,
        op::RC_KEY => 0x01,
        op::RC_TX_KEY => 0x0C,
        op::RC_ACTION => 0x01,
        op::RC_TX_ACTION => 0x0C,
        op::AUDIO_VOLUME_UP => 0x01,
        op::AUDIO_VOLUME_DOWN => 0x01,
        op::AUDIO_VOLUME_MUTE => 0x01,
        op::AUDIO_CLIP => 0x01,
        op::AUDIO_SET_ROUTE => 0x01,
        op::AUDIO_SET_VOLUME => 0x11,
        op::SYS_TEMPERATURE => 0x01,
        op::PDU_STATE => 0x01,
        op::V2IP_SOURCE_SWITCH => 0x06,
        op::V2IP_LINK_REMOTE => 0x06,
        op::V2IP_DETECT_BAYS => 0x06,
        op::CHANGE_BAY_NAME => 0x06,
        op::SYS_BAY_CONFIG_SECONDARY => 0x07,
        op::V2IP_MANUAL_SRC_SWITCH => 0x07,
        op::SYS_BAY_V2IP_SOURCES => 0x09,
        op::BAY_HIDE => 0x06,
        op::SYS_REBOOT => 0x01,
        op::NET_LINK_STATUS => 0x22,
        op::FIRMWARE_VERSION => 0x06,
        op::SYS_MONITORING_PULSE => 0x01,
        op::V2IP_UPGRADE_FPGA => 0x06,
        op::V2IP_BLIST_REGISTER => 0x06,
        op::V2IP_BLIST_UNREGISTER => 0x06,
        op::TOPOLOGY => 0x06,
        op::BAY_SIGNAL_STATUS => 0x06,
        op::BAY_MIRROR_STATUS => 0x06,
        op::BAY_EDID_PROFILE => 0x08,
        op::SETUP_STATUS => 0x0A,
        op::SET_MASTER => 0x0B,
        op::SET_INSTALLER => 0x0C,
        op::BAY_FILTER_STATUS => 0x0E,
        op::BAY_STATUS => 0x0F,
        op::SYS_FACTORY_RESET => 0x0F,
        op::MESH_OPERATION => 0x1D,
        op::V2IP_DEVICE_CFG => 0x11,
        op::AMP_ZONE_SETTINGS => 0x1C,
        op::AMP_DOLBY_STATE => 0x1C,
        op::V2IP_STATS => 0x13,
        op::V2IP_TILING => 0x14,
        op::V2IP_POWER_SAVE => 0x15,
        op::V2IP_MULTIVIEWER => 0x16,
        op::V2IP_AUDIO => 0x1A,
        op::V2IP_BAY_MAPPINGS => 0x1C,
        op::RC_SETTINGS => 0x1D,
        op::SYS_STATUS => 0x1E,
        op::DEBUG => 0x1F,
        op::RC_IR_TX => 0x23,
        op::V2IP_VIDEO_WALL => 0x28,
        _ => PROTOCOL_VERSION,
    }
}

/// The protocol version this library stamps on a frame carrying `opcode`.
///
/// A receiver drops any frame stamped above its own version, so this is also
/// the version the addressee has to report before [`Tx::send`] will let the
/// frame out.
///
/// It is [`protocol_for`] everywhere but `V2IP_MULTIVIEWER`, which is stamped
/// at [`MULTIVIEWER_PROTOCOL`] - above the 0x16 its opcode table declares -
/// matching the reference Python library. MatrixOS has no handler for that
/// opcode, since the multiviewer module owns it, so what a 0x16..0x1F receiver
/// expects is unverified and matching the reference is the only evidence there
/// is.
///
/// [`Tx::send`]: super::tx::Tx::send
pub(crate) fn stamp_for(opcode: Opcode) -> u16 {
    match opcode {
        op::V2IP_MULTIVIEWER => MULTIVIEWER_PROTOCOL,
        _ => protocol_for(opcode),
    }
}

/// Protocol version stamped on a `V2IP_MULTIVIEWER` (0x42) frame. See
/// [`stamp_for`].
pub(crate) const MULTIVIEWER_PROTOCOL: u16 = 0x20;

/// Sub-opcodes of `V2IP_AUDIO` (0x43), multiplexed on the `u16` at payload
/// offset 0.
pub(crate) mod audio_sub {
    pub(crate) const FEATURES: u16 = 0;
    pub(crate) const MUTE: u16 = 1;
    pub(crate) const TRIGGER: u16 = 2;
    pub(crate) const SELECT_INPUT: u16 = 3;
    pub(crate) const VOLUME: u16 = 4;
    pub(crate) const LINKS: u16 = 5;
}

/// Sub-opcodes of `V2IP_MULTIVIEWER` (0x42), on the byte at payload offset 16.
///
/// Only `STATUS` reports state; the other fifteen are requests, and what they
/// change comes back on the following `STATUS`.
pub(crate) mod mv_sub {
    pub(crate) const STATUS: u8 = 0;
    pub(crate) const VIEW_MODE: u8 = 1;
    pub(crate) const VIDEO_SOURCE: u8 = 2;
    pub(crate) const AUDIO_SOURCE: u8 = 3;
    pub(crate) const AUDIO_VOLUME: u8 = 4;
    pub(crate) const EDID_TEMPLATE: u8 = 5;
    pub(crate) const ROUTE_RC: u8 = 6;
    pub(crate) const PIP_SIZE: u8 = 7;
    pub(crate) const PIP_POSITION: u8 = 8;
    pub(crate) const ASPECT: u8 = 9;
    pub(crate) const AUTO_SWITCH: u8 = 10;
    pub(crate) const OUTPUT_MODE: u8 = 11;
    pub(crate) const OUTPUT_ITC_MODE: u8 = 12;
    pub(crate) const HDCP_MODE: u8 = 13;
    pub(crate) const CONFIG_SOURCE: u8 = 14;
    pub(crate) const AUTO_ROUTE: u8 = 15;
}
