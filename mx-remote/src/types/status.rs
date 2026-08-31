// Author: Lars Op den Kamp (lars@opdenkamp-it.nl)
// Copyright (c) 2026 Op den Kamp IT Solutions

//! Device and bay state.

use core::fmt;

use crate::wire::{BayStatus, BayUid, FirmwareType, MxrSignalType};

/// The high-level state of a device or bay on the network.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum DeviceStatus {
    /// Reachable and reporting.
    #[default]
    Online,
    /// Has stopped answering.
    Offline,
    /// Announced a reboot.
    Rebooting,
    /// Still coming up.
    Booting,
    /// Present but not participating.
    Inactive,
}

impl fmt::Display for DeviceStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Online => "Online",
            Self::Offline => "Offline",
            Self::Rebooting => "Rebooting",
            Self::Booting => "Booting",
            Self::Inactive => "Inactive",
        })
    }
}

/// The CEC power state of a device connected to a bay.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PowerStatus {
    /// The device has not reported a power state.
    #[default]
    Unknown,
    /// Powered on.
    On,
    /// Powered off.
    Off,
}

impl fmt::Display for PowerStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::On => "on",
            Self::Off => "off",
            Self::Unknown => "unknown",
        })
    }
}

/// The connect / signal-detect state reported for a bay.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ConnectStatus {
    /// The device has not reported a connect state.
    #[default]
    Unknown,
    /// Something is attached.
    Connected,
    /// Nothing is attached.
    Disconnected,
}

impl fmt::Display for ConnectStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Connected => "connected",
            Self::Disconnected => "disconnected",
            Self::Unknown => "unknown",
        })
    }
}

/// The visibility state of a bay.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum HiddenStatus {
    /// The device has not reported a visibility.
    #[default]
    Unknown,
    /// Hidden from the user interface.
    Hidden,
    /// Shown in the user interface.
    Visible,
}

impl fmt::Display for HiddenStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Hidden => "hidden",
            Self::Visible => "visible",
            Self::Unknown => "unknown",
        })
    }
}

/// The per-channel mute bitfield: bit 0 is left, bit 1 is right.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct MuteStatus(u8);

impl MuteStatus {
    /// Wraps the raw wire byte.
    pub const fn from_wire(value: u8) -> Self {
        Self(value)
    }

    /// Returns the raw wire byte.
    pub const fn to_wire(self) -> u8 {
        self.0
    }

    /// Whether the left channel is muted.
    pub const fn left(self) -> bool {
        self.0 & (1 << 0) != 0
    }

    /// Whether the right channel is muted.
    pub const fn right(self) -> bool {
        self.0 & (1 << 1) != 0
    }

    /// Whether either channel is muted.
    pub const fn muted(self) -> bool {
        self.0 != 0
    }
}

/// The volume and mute state of a bay.
///
/// A `None` field is one the device did not report, which is distinct from a
/// reported zero.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct VolumeMuteStatus {
    /// Left channel volume, as a percentage.
    pub volume_left: Option<u8>,
    /// Right channel volume, as a percentage.
    pub volume_right: Option<u8>,
    /// Whether the left channel is muted.
    pub muted_left: Option<bool>,
    /// Whether the right channel is muted.
    pub muted_right: Option<bool>,
}

impl VolumeMuteStatus {
    /// The combined left/right volume percentage, or zero when neither channel
    /// reported one.
    pub fn volume(&self) -> u8 {
        match (self.volume_left, self.volume_right) {
            (Some(l), Some(r)) => ((u16::from(l) + u16::from(r)) / 2) as u8,
            (Some(l), None) => l,
            (None, Some(r)) => r,
            (None, None) => 0,
        }
    }

    /// The combined mute state, or `None` when neither channel reported one.
    pub fn muted(&self) -> Option<bool> {
        match (self.muted_left, self.muted_right) {
            (None, None) => None,
            (l, r) => Some(l.unwrap_or(false) || r.unwrap_or(false)),
        }
    }

    /// Encodes the 3-byte `[volume_left, volume_right, muted]` field. A channel
    /// that reported no volume is sent the combined one.
    pub(crate) fn wire(&self) -> [u8; 3] {
        [
            self.volume_left.unwrap_or_else(|| self.volume()),
            self.volume_right.unwrap_or_else(|| self.volume()),
            self.muted_value(),
        ]
    }

    /// Encodes the mute field as `MXR_AUDIO_MUTE_*`: a per-channel bitmask.
    fn muted_value(&self) -> u8 {
        if self.muted() != Some(true) {
            return 0;
        }
        match (
            self.muted_left.unwrap_or(false),
            self.muted_right.unwrap_or(false),
        ) {
            (true, true) => 3,
            (true, false) => 1,
            _ => 2,
        }
    }
}

/// What a bay signal status report carries beyond the signal-detected flag and
/// the human-readable signal type.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct BaySignalDetails {
    /// Frame rate in Hz, already corrected for a 1000/1001 clock.
    pub frame_rate: f64,
    /// TMDS clock rate in Hz.
    pub tmds_clock: u32,
    /// The bay status word from the report's bay block.
    pub status: BayStatus,
    /// The signal type the bay is scaling to.
    pub scaling: MxrSignalType,
    /// Video clock rate in Hz.
    pub clock_rate: u32,
    /// The audio alongside the video, absent when the report carried no audio
    /// block.
    pub audio: Option<BayAudioDetails>,
}

/// The audio a bay signal report describes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BayAudioDetails {
    /// How the stream is encoded: 0 unknown, 1 L-PCM, 2 high bit rate.
    pub format: u8,
    /// Channel count.
    pub channels: u8,
    /// Sample rate in Hz.
    pub sample_rate: u32,
    /// The coding type the source claims in its CTA-861 audio infoframe, and
    /// `None` where it sent no infoframe at all.
    ///
    /// The two are worth keeping apart: a source sending no infoframe leaves
    /// this field zero, and zero is also what a source claiming "refer to the
    /// stream header" writes into it.
    pub coding: Option<u8>,
}

/// A firmware component reported by a device.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FirmwareVersion {
    /// Which component this describes.
    pub firmware_type: FirmwareType,
    /// Build timestamp, in seconds since the Unix epoch.
    pub timestamp: u32,
    /// Human-readable version string.
    pub version: String,
    /// Source revision hash.
    pub hash: u32,
}

impl fmt::Display for FirmwareVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "firmware {} version {} hash {}",
            self.firmware_type, self.version, self.hash
        )
    }
}

/// Whether an output bay mirrors another device's output, and which one.
///
/// The default is "not mirroring".
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BayMirrorStatus {
    /// The bay being mirrored, or `None` when this bay mirrors nothing.
    pub target: Option<BayUid>,
}

impl BayMirrorStatus {
    /// Reports whether this bay mirrors another.
    pub const fn is_mirroring(&self) -> bool {
        self.target.is_some()
    }
}

/// One device in a topology report.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TopologyEntry {
    /// The device this entry describes.
    pub uid: crate::wire::DeviceUid,
    /// Bitmask of the devices it is connected to.
    pub mask: u32,
}

/// The audio return channel a bay is carrying.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ArcStatus {
    /// No audio is being returned.
    #[default]
    Inactive,
    /// Returned over HDMI.
    Hdmi,
    /// Returned over optical.
    Optical,
    /// Returned over analogue.
    Analog,
}

impl fmt::Display for ArcStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Inactive => "Inactive",
            Self::Hdmi => "HDMI",
            Self::Optical => "optical",
            Self::Analog => "analog",
        })
    }
}
