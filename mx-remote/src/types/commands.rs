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
    /// The control method.
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
    /// The driver state on the source.
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
    /// Tick length the sender says its timing values are in.
    ///
    /// Reported and then ignored: a device replaying a burst applies its own
    /// tick length rather than this one, and the two differ between products.
    /// So it describes the sender, not the timings as the receiver will read
    /// them, and converting with it does not make a burst portable.
    pub timer_resolution: u16,
    /// Carrier frequency in Hz.
    pub frequency: u16,
    /// How many timing values the sender says it appended.
    ///
    /// A declaration, not a measurement: nothing on the wire ties it to the
    /// bytes that arrived, and a device replaying the burst indexes this many
    /// rather than counting. Bound any read by the timing list itself, which is
    /// the only part of a frame that cannot claim more than it carries.
    pub nb_timings: u16,
    /// Index at which the repeat section starts, declared on the same terms as
    /// `nb_timings` and equally unbounded by the list.
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
    ///
    /// Index 0 is never replayed. A device blasting this list starts at the
    /// second timing, so the first holds the gap captured ahead of the burst
    /// and goes nowhere. A capture of a single timing therefore carries no
    /// burst at all, and a caller counting pulses is counting one fewer than
    /// this holds.
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
    ///
    /// Laid out as a capture is, and index 0 is discarded the same way, so a
    /// capture is replayed by passing its timings through unchanged. A request
    /// carrying one timing asks for nothing.
    ///
    /// Rebuild the rest of the request rather than forwarding a capture's
    /// header: `timestamp` must be the sending client's own clock at send
    /// time, because the addressed device measures the gap ahead of the burst
    /// from it rather than from anything in this list.
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

/// Where a video-wall sink's window sits, and the picture it was measured
/// against.
///
/// The raster travels with the window because only the sender knows what the
/// installer drew against; a sink deriving it from what it happens to be
/// showing would place the window against the wrong picture.
///
/// Nothing on the receiving side is guaranteed to check any of this, so
/// [`VideoWallWindow::validate`] runs before every send. See
/// [`crate::Remote::store_video_wall`] for why that matters.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct VideoWallWindow {
    /// Window origin, horizontal. A multiple of [`VIDEO_WALL_POS_ALIGN`].
    pub pos_x: u16,
    /// Window origin, vertical. No alignment constraint.
    pub pos_y: u16,
    /// Window width. A multiple of [`VIDEO_WALL_WIDTH_ALIGN`], and at least
    /// [`VIDEO_WALL_MIN_SIZE`] unless it is zero.
    pub width: u16,
    /// Window height, at least [`VIDEO_WALL_MIN_SIZE`] unless it is zero. No
    /// alignment constraint.
    pub height: u16,
    /// Active picture width the window was measured against.
    pub raster_w: u16,
    /// Active picture height the window was measured against.
    pub raster_h: u16,
}

/// The window that clears a wall, leaving the sink showing the whole frame.
///
/// A zero width or height is how the protocol spells "clear", so this is a
/// legitimate window rather than one that fails [`VideoWallWindow::validate`].
pub const VIDEO_WALL_CLEARED: VideoWallWindow = VideoWallWindow {
    pos_x: 0,
    pos_y: 0,
    width: 0,
    height: 0,
    raster_w: 0,
    raster_h: 0,
};

/// The horizontal origin must be a multiple of this: the sink's buffer start
/// has to be aligned.
pub const VIDEO_WALL_POS_ALIGN: u16 = 64;

/// The width must be a multiple of this: the sink's pipeline moves four pixels
/// per clock.
pub const VIDEO_WALL_WIDTH_ALIGN: u16 = 4;

/// Neither side of a window may be smaller than this, the scaler's minimum.
pub const VIDEO_WALL_MIN_SIZE: u16 = 64;

impl VideoWallWindow {
    /// Reports whether this window clears the wall rather than placing one.
    pub const fn is_cleared(&self) -> bool {
        self.width == 0 || self.height == 0
    }

    /// Checks the geometry the sink is not guaranteed to check itself.
    ///
    /// The three alignments follow the sink's hardware and a live unit reports
    /// them over HTTP, so a caller with access to one can read them rather
    /// than trust the constants here. The containment rule has no such source:
    /// it is checked by the sink's own HTTP path and by nothing on the mesh
    /// path, at any version.
    ///
    /// A cleared window passes: zero is the protocol's word for "clear", not a
    /// window too small to draw.
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.is_cleared() {
            return Ok(());
        }
        if self.pos_x % VIDEO_WALL_POS_ALIGN != 0 {
            return Err("a video wall window's horizontal origin must be a multiple of 64");
        }
        if self.width % VIDEO_WALL_WIDTH_ALIGN != 0 {
            return Err("a video wall window's width must be a multiple of 4");
        }
        if self.width < VIDEO_WALL_MIN_SIZE || self.height < VIDEO_WALL_MIN_SIZE {
            return Err("neither side of a video wall window may be smaller than 64");
        }
        // Widened, because a window running off the raster is exactly the case
        // where a u16 sum would wrap and read as containment.
        if u32::from(self.pos_x) + u32::from(self.width) > u32::from(self.raster_w)
            || u32::from(self.pos_y) + u32::from(self.height) > u32::from(self.raster_h)
        {
            return Err("a video wall window must fit inside the raster it names");
        }
        Ok(())
    }
}

impl fmt::Display for VideoWallWindow {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.is_cleared() {
            return f.write_str("cleared");
        }
        write!(
            f,
            "{}x{}+{}+{} of {}x{}",
            self.width, self.height, self.pos_x, self.pos_y, self.raster_w, self.raster_h
        )
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
    /// Active picture width the window was authored against, as on
    /// [`VideoWallWindow`].
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
