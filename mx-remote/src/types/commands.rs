// Author: Lars Op den Kamp (lars@opdenkamp-it.nl)
// Copyright (c) 2026 Op den Kamp IT Solutions

//! Payloads of the command and notification opcodes.
//!
//! These frames are addressed to a device rather than reporting its state, so
//! most surface as events rather than cached state. A frame addressed to
//! another unit still reaches every client on the group: the target field says
//! who it was for, and that is neither necessarily this client nor the sender.

use core::fmt;
use std::net::Ipv4Addr;

use crate::wire::{DeviceUid, EdidProfile, RcAction, RcKey};

/// Asks a device, addressed by serial, to switch a sink.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SetRouteRequest {
    /// Serial of the device to act on.
    pub serial: String,
    /// Output bay to switch.
    pub sink_bay: u16,
    /// Source bay to switch it to.
    pub source_bay: u16,
    /// Whether to skip the power-on commands that normally accompany a switch.
    pub no_power_on: bool,
    /// True when this arrived on `AUDIO_SET_ROUTE` rather than `MX_SET_ROUTE`.
    pub audio_only: bool,
}

impl fmt::Display for SetRouteRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "set route on {}: sink={} source={}",
            self.serial, self.sink_bay, self.source_bay
        )
    }
}

/// One EDID block from a `DEV_EDID` reply.
///
/// A reply carries one record per bay mode, so a combined reply produces two.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct EdidRecord {
    /// True for a sink's EDID, false for a source's.
    pub output: bool,
    /// A 256-byte EDID: a base block plus exactly one extension block. A
    /// display publishing further extension blocks yields only the first.
    pub data: Vec<u8>,
}

/// Asks one device for its EDID.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct EdidRequest {
    /// The device being asked.
    pub target: DeviceUid,
    /// Whether the sink's EDID is wanted rather than the source's.
    pub output: bool,
}

/// Asks a device to rename one of its bays.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BayNameChange {
    /// The device to act on.
    pub target: DeviceUid,
    /// The bay to rename.
    pub port: u16,
    /// The new name.
    pub name: String,
}

/// Asks a device to switch its input EDID profile.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct EdidProfileChange {
    /// The device to act on.
    pub target: DeviceUid,
    /// The profile to switch to.
    pub profile: EdidProfile,
}

/// Asks peers to factory-reset.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FactoryResetRequest {
    /// Set by the broadcast form, which targets every peer.
    pub all: bool,
    /// Set by the single-uid form. With neither this nor `all`, the request
    /// addresses only the sender.
    pub target: Option<DeviceUid>,
}

/// Asks one device to reboot.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RebootRequest {
    /// The device to reboot.
    pub target: DeviceUid,
}

/// The window a sink is currently told to show.
///
/// This is the readable, pollable view of a sink's window. It is not the
/// persisted video wall setting: on a sink running the v2ipwall module a write
/// here is transient, because that module's reconciler pushes its own target
/// window back within about a second. [`VideoWallCommand`] carries intent.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct V2ipTilingConfig {
    /// The sink this window belongs to.
    pub target: DeviceUid,
    /// Window origin, horizontal.
    pub pos_x: u16,
    /// Window origin, vertical.
    pub pos_y: u16,
    /// Window width.
    pub width: u16,
    /// Window height.
    pub height: u16,
}

impl fmt::Display for V2ipTilingConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "tiling x={} y={} {}x{}",
            self.pos_x, self.pos_y, self.width, self.height
        )
    }
}

/// Asks a sink to enter or leave power save.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct V2ipPowerSaveRequest {
    /// The sink to act on, or `None` on the broadcast form.
    pub target: Option<DeviceUid>,
    /// Whether power save is being entered.
    pub enabled: bool,
}

/// The remote-control configuration of a source bay.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RcSettings {
    /// The device this configuration belongs to.
    pub target: DeviceUid,
    /// The control method (`rc_target_t`).
    ///
    /// A single byte: the enum is plain and Cortex-M builds with
    /// `-fshort-enums`, so three bytes of padding follow it before the address.
    /// That padding is not zero - firmware copies an uncleared stack local over
    /// the payload - so widening this to a `u32` makes one unchanged setting
    /// decode differently on every frame.
    pub rc_target: u8,
    /// The control target's address, `None` when unset.
    pub ip: Option<Ipv4Addr>,
    /// Whether CEC is enabled.
    pub cec_enabled: bool,
    /// Whether CEC powers the sink on automatically.
    pub cec_auto_on: bool,
    /// Whether remote-control commands are forwarded.
    pub forward_rc: bool,
    /// Whether infrared is forwarded.
    pub forward_ir: bool,
    /// The driver state on the source (`mxr_rc_status_t`).
    ///
    /// A value above the last one this library knows is passed through as it
    /// arrived rather than clamped, so a firmware update cannot make it read as
    /// a known state.
    pub rc_status: u8,
    /// The driver-reported status string, empty when unknown.
    pub status_name: String,
}

/// The raw-IR metadata shared by the IR capture and transmit frames.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct IrMeta {
    /// Tick length of the timing values.
    pub timer_resolution: u16,
    /// Carrier frequency in Hz.
    pub frequency: u16,
    /// Number of timing values that follow.
    pub nb_timings: u16,
    /// Index at which the repeat section starts.
    pub repeat_offset: u16,
    /// Capture status.
    pub status: u8,
}

/// Raw IR captured on a bay of the sending device.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct IrCapture {
    /// The bay that captured it.
    pub port: u16,
    /// Sender clock at capture time.
    pub timestamp: u32,
    /// Sender clock at the last signal change.
    pub last_change: u32,
    /// Metadata for the timings.
    pub meta: IrMeta,
    /// The raw on/off timing blob following the header.
    pub timings: Vec<u8>,
}

/// Asks one device to blast raw IR on one of its local bays.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct IrTransmitRequest {
    /// The device to act on.
    pub target: DeviceUid,
    /// Bay mode in the target's own numbering, not a port.
    pub local_mode: u8,
    /// Bay number in the target's own numbering, not a port.
    pub local_bay: u8,
    /// Sender clock at send time.
    pub timestamp: u32,
    /// Metadata for the timings.
    pub meta: IrMeta,
    /// The raw on/off timing blob following the header.
    pub timings: Vec<u8>,
}

/// Asks one device to send a remote-control key on a bay.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct KeyTransmitRequest {
    /// The device to act on.
    pub target: DeviceUid,
    /// Bay in the target's own numbering.
    pub local_bay: u16,
    /// The key to send.
    pub key: RcKey,
}

/// Asks one device to perform a remote-control action.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ActionTransmitRequest {
    /// The device to act on.
    pub target: DeviceUid,
    /// Bay in the target's own numbering.
    pub local_bay: u16,
    /// The action to perform.
    pub action: RcAction,
}

/// Reports that a bay detected audio clipping.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AudioClip {
    /// The bay that clipped.
    pub port: u16,
    /// The clip level reported.
    pub clip: u8,
}

/// The electrical state a PDU reports.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PduState {
    /// Current in amperes.
    pub current: f64,
    /// Voltage in volts.
    pub voltage: f64,
    /// Real power in watts.
    pub power: f64,
    /// Dissipation in watts.
    pub dissipation: f64,
    /// Mains frequency in Hz.
    pub frequency: f64,
    /// Per-outlet state.
    pub outlets: [u8; 8],
}

impl fmt::Display for PduState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{:.2}A {:.2}V {:.2}W",
            self.current, self.voltage, self.power
        )
    }
}

/// Registers or unregisters a device on the source blacklist.
///
/// The firmware guards this opcode behind `V2IP_SUPPORT_BLACKLIST`, which is 0
/// in shipping builds, so nothing in current firmware emits it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct V2ipBlacklistChange {
    /// The device being listed.
    pub target: DeviceUid,
    /// Whether it is being registered rather than removed.
    pub registered: bool,
}

/// What a [`VideoWallCommand`] asks the sink to do with the window.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct VideoWallOp(u8);

impl VideoWallOp {
    /// Applies the window without persisting it.
    pub const PREVIEW: Self = Self(0);
    /// Persists the window as the sink's wall setting.
    pub const STORE: Self = Self(1);
    /// Restores the persisted setting; carries no window.
    pub const REVERT: Self = Self(2);

    /// Wraps a raw wire value, including one this library has no name for.
    pub const fn from_wire(value: u8) -> Self {
        Self(value)
    }

    /// Returns the raw wire value.
    pub const fn to_wire(self) -> u8 {
        self.0
    }
}

impl fmt::Display for VideoWallOp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match *self {
            Self::PREVIEW => "preview",
            Self::STORE => "store",
            Self::REVERT => "revert",
            _ => "unknown",
        })
    }
}

/// Asks one sink to crop its source to a wall window.
///
/// This replaces the sink's window outright: unlike a V2IP device config, no
/// field carries a validity marker, and a zero width or height is the wire
/// spelling of "clear the wall and show the full frame" rather than "unset".
///
/// The opcode belongs to the loadable v2ipwall module rather than MatrixOS, and
/// a wall has no object of its own on the wire: it is a set of sinks each
/// holding one rectangle, one frame each. It is a command with no reply, so
/// nothing here is ever a status readback.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct VideoWallCommand {
    /// The sink to act on.
    pub target: DeviceUid,
    /// Window origin, horizontal.
    pub pos_x: u16,
    /// Window origin, vertical.
    pub pos_y: u16,
    /// Window width.
    pub width: u16,
    /// Window height.
    pub height: u16,
    /// Active picture width the window was authored against.
    ///
    /// The raster travels with the window because only the sender knows what
    /// the installer drew against; a sink deriving it from what it happens to
    /// be showing would store the window against the wrong picture.
    pub raster_w: u16,
    /// Active picture height the window was authored against.
    pub raster_h: u16,
    /// What to do with the window.
    pub op: VideoWallOp,
}

impl VideoWallCommand {
    /// Reports whether the geometry in this command is meaningful.
    ///
    /// A revert zeroes the window and raster and the receiver ignores those
    /// bytes, so its zeros are not a clear.
    pub fn has_window(&self) -> bool {
        self.op != VideoWallOp::REVERT
    }

    /// Reports a command that clears the wall and shows the full frame.
    pub fn is_cleared(&self) -> bool {
        self.has_window() && (self.width == 0 || self.height == 0)
    }
}

impl fmt::Display for VideoWallCommand {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if !self.has_window() {
            return f.write_str("video wall revert");
        }
        if self.is_cleared() {
            return write!(f, "video wall {}: clear", self.op);
        }
        write!(
            f,
            "video wall {}: {}x{}+{}+{} of {}x{}",
            self.op, self.width, self.height, self.pos_x, self.pos_y, self.raster_w, self.raster_h
        )
    }
}

/// A command addressed to a multiviewer.
///
/// The parameters past the envelope are exposed as raw bytes: the opcode
/// belongs to the multiviewer module rather than MatrixOS, so beyond the
/// envelope there is no firmware source here to pin per-sub-command field
/// semantics against.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MultiviewerCommand {
    /// The multiviewer being addressed.
    pub target: DeviceUid,
    /// The sub-opcode. A value this library has no name for still arrives.
    pub op: u8,
    /// Everything after the envelope, empty when the frame carries none.
    pub params: Vec<u8>,
}

impl fmt::Display for MultiviewerCommand {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "multiviewer command {} for {} ({} param bytes)",
            self.op,
            self.target,
            self.params.len()
        )
    }
}

/// An audio input-selection change: which source endpoint a sink endpoint was
/// switched to.
///
/// The sink is named twice on the wire, once as the command header's target and
/// again at the head of the body; the body's second uid is the source.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AudioChangeSource {
    /// The device whose endpoint is being listened to.
    pub source_uid: DeviceUid,
    /// The endpoint being listened to.
    pub source_id: u16,
    /// The device doing the listening.
    pub target_uid: DeviceUid,
    /// The endpoint doing the listening.
    pub target_id: u16,
}

impl fmt::Display for AudioChangeSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "audio source change {}:{} -> {}:{}",
            self.source_uid, self.source_id, self.target_uid, self.target_id
        )
    }
}
