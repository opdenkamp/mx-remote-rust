// Author: Lars Op den Kamp (lars@opdenkamp-it.nl)
// Copyright (c) 2026 Op den Kamp IT Solutions

//! Wire enumerations and bitmasks.
//!
//! Each type is a newtype over the integer that travels on the wire, with
//! named constants rather than a closed set of variants. A value this library
//! has no name for reaches the caller as it arrived: zero is a valid value for
//! most of these, so a confidently wrong reading is worse than an unrecognised
//! one.

use core::fmt;
use core::ops::{BitAnd, BitOr, BitOrAssign};

/// Declares a bitmask newtype over `u32` with the given named bit constants.
macro_rules! bitmask {
    (
        $(#[$meta:meta])*
        $name:ident { $( $(#[$cmeta:meta])* $cname:ident = $value:expr; )* }
    ) => {
        $(#[$meta])*
        #[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(u32);

        impl $name {
            /// No bits set.
            pub const NONE: Self = Self(0);

            $( $(#[$cmeta])* pub const $cname: Self = Self($value); )*

            /// Wraps a raw wire value, including bits this library has no name for.
            pub const fn from_bits(bits: u32) -> Self {
                Self(bits)
            }

            /// Returns the raw wire value.
            pub const fn bits(self) -> u32 {
                self.0
            }

            /// Reports whether every bit in `other` is set.
            pub const fn has(self, other: Self) -> bool {
                self.0 & other.0 == other.0
            }

            /// Reports whether no bit is set.
            pub const fn is_empty(self) -> bool {
                self.0 == 0
            }
        }

        impl BitOr for $name {
            type Output = Self;
            fn bitor(self, rhs: Self) -> Self {
                Self(self.0 | rhs.0)
            }
        }

        impl BitOrAssign for $name {
            fn bitor_assign(&mut self, rhs: Self) {
                self.0 |= rhs.0;
            }
        }

        impl BitAnd for $name {
            type Output = Self;
            fn bitand(self, rhs: Self) -> Self {
                Self(self.0 & rhs.0)
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "{:#010x}", self.0)
            }
        }
    };
}

/// Declares an enumeration newtype over `$repr` with the given named constants.
macro_rules! wire_enum {
    (
        $(#[$meta:meta])*
        $name:ident: $repr:ty { $( $(#[$cmeta:meta])* $cname:ident = $value:expr; )* }
    ) => {
        $(#[$meta])*
        #[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name($repr);

        impl $name {
            $( $(#[$cmeta])* pub const $cname: Self = Self($value); )*

            /// Wraps a raw wire value, including one this library has no name for.
            pub const fn from_wire(value: $repr) -> Self {
                Self(value)
            }

            /// Returns the raw wire value.
            pub const fn to_wire(self) -> $repr {
                self.0
            }
        }
    };
}

bitmask! {
    /// Capabilities a device reports in its hello frame.
    DeviceFeature {
        /// Receives infrared.
        IR_RX = 1 << 0;
        /// Transmits infrared.
        IR_TX = 1 << 1;
        /// Speaks CEC.
        CEC = 1 << 2;
        /// Acts as a V2IP stream source.
        V2IP_SOURCE = 1 << 3;
        /// Acts as a V2IP stream sink.
        V2IP_SINK = 1 << 4;
        /// Routes video.
        VIDEO_ROUTING = 1 << 5;
        /// Routes audio.
        AUDIO_ROUTING = 1 << 6;
        /// Controls volume.
        VOLUME_CONTROL = 1 << 7;
        /// Supports audio return.
        AUDIO_RETURN = 1 << 8;
        /// Passes remote-control commands through.
        REMOTE_CONTROL = 1 << 9;
        /// Installer setup has been completed.
        SETUP_COMPLETED = 1 << 10;
        /// Is the master of its mesh.
        MESH_MASTER = 1 << 11;
        /// Has a notification pending.
        STATUS_NOTIFY = 1 << 12;
        /// Has a warning pending.
        STATUS_WARNING = 1 << 13;
        /// Has an error pending.
        STATUS_ERROR = 1 << 14;
        /// Is about to reboot.
        STATUS_REBOOT = 1 << 15;
        /// Is a member of a mesh.
        MESH_MEMBER = 1 << 16;
        /// Is an audio amplifier.
        AUDIO_AMPLIFIER = 1 << 17;
        /// Is still booting.
        BOOTING = 1 << 18;
        /// Is a management client rather than a device.
        MANAGER = 1 << 19;
        /// Is in power-save mode.
        STATUS_POWER_SAVE = 1 << 20;
        /// Supports meshing.
        MESH = 1 << 21;
        /// Is a multiviewer.
        MULTIVIEWER = 1 << 22;
        /// Has crashed since it last booted.
        STATUS_CRASHED = 1 << 23;
        /// Supports video walls.
        VIDEO_WALL = 1 << 24;
        /// Initialises the configuration it broadcasts.
        ///
        /// Firmware without this bit sends a device configuration built over
        /// uninitialised memory, so fields it did not mean to write carry junk.
        CONFIG_INITIALISED = 1 << 25;
        /// Set while the device is in its boot loader.
        BOOT_BIT = 1 << 31;
    }
}

bitmask! {
    /// Capabilities of a single bay.
    BayFeatures {
        /// HDMI output.
        HDMI_OUT = 1 << 0;
        /// HDMI input.
        HDMI_IN = 1 << 1;
        /// Digital audio output.
        AUDIO_DIG_OUT = 1 << 2;
        /// Digital audio input.
        AUDIO_DIG_IN = 1 << 3;
        /// Analogue audio output.
        AUDIO_ANA_OUT = 1 << 4;
        /// Analogue audio input.
        AUDIO_ANA_IN = 1 << 5;
        /// Infrared input.
        IR_IN = 1 << 6;
        /// Infrared output.
        IR_OUT = 1 << 7;
        /// Amplified audio output.
        AUDIO_AMP_OUT = 1 << 8;
        /// Remote-control output.
        RC_OUT = 1 << 9;
        /// Remote-control input.
        RC_IN = 1 << 10;
        /// Dolby decoding.
        DOLBY = 1 << 11;
        /// Switches itself off when idle.
        AUTO_OFF = 1 << 12;
        /// Is a remote V2IP source.
        V2IP_SOURCE_REMOTE = 1 << 13;
        /// Is a remote V2IP sink.
        V2IP_SINK_REMOTE = 1 << 14;
        /// Is a local V2IP source.
        V2IP_SOURCE_LOCAL = 1 << 15;
        /// Is a local V2IP sink.
        V2IP_SINK_LOCAL = 1 << 16;
    }
}

bitmask! {
    /// Live status flags of a single bay.
    ///
    /// Bits 16-19 and 22-23 are bit-fields rather than flags; read them with
    /// [`BayStatus::rc_type`] and [`BayStatus::hdcp`].
    BayStatus {
        /// The bay reports a fault.
        FAULT = 1 << 0;
        /// The bay is hidden from the user interface.
        HIDDEN = 1 << 1;
        /// The bay has power.
        POWERED = 1 << 2;
        /// A signal is present.
        SIGNAL_DETECTED = 1 << 3;
        /// Hot-plug detect is asserted.
        HPD_DETECTED = 1 << 4;
        /// The signal is scrambled.
        SIGNAL_SCRAMBLE = 1 << 5;
        /// An HDBaseT link is up.
        HDBT_CONNECTED = 1 << 6;
        /// A CEC device answered.
        CEC_DETECTED = 1 << 7;
        /// The attached device was powered on.
        POWERED_ON = 1 << 8;
        /// The attached device was powered off.
        POWERED_OFF = 1 << 9;
        /// Audio return over HDMI is active.
        AUDIO_ARC_HDMI = 1 << 10;
        /// Audio return over optical is active.
        AUDIO_ARC_OPTIC = 1 << 11;
        /// Audio return over analogue is active.
        AUDIO_ARC_ANALOG = 1 << 12;
        /// The bay is offline.
        OFFLINE = 1 << 13;
        /// The V2IP decoder is disabled.
        DECODER_DISABLE = 1 << 14;
        /// The V2IP encoder is disabled.
        ENCODER_DISABLE = 1 << 15;
        /// CEC is switched off for this bay.
        CEC_DISABLED = 1 << 20;
        /// The V2IP encoder reports an error.
        ENCODER_ERROR = 1 << 21;
    }
}

impl BayStatus {
    const RC_TYPE_SHIFT: u32 = 16;
    const RC_TYPE_MASK: u32 = 0xF << Self::RC_TYPE_SHIFT;
    const HDCP_SHIFT: u32 = 22;
    const HDCP_MASK: u32 = 0x3 << Self::HDCP_SHIFT;

    /// Extracts the remote-control type carried in bits 16-19.
    pub const fn rc_type(self) -> RcType {
        RcType(((self.0 & Self::RC_TYPE_MASK) >> Self::RC_TYPE_SHIFT) as u8)
    }

    /// Extracts the HDCP version carried in bits 22-23.
    pub const fn hdcp(self) -> u8 {
        ((self.0 & Self::HDCP_MASK) >> Self::HDCP_SHIFT) as u8
    }
}

bitmask! {
    /// Media carried by a virtual link.
    LinkFeature {
        /// Video over HDMI.
        VIDEO_HDMI = 1 << 0;
        /// Audio over optical.
        AUDIO_OPTICAL = 1 << 1;
        /// Audio over analogue.
        AUDIO_ANALOG = 1 << 2;
        /// Infrared.
        IR = 1 << 3;
        /// Remote control.
        RC = 1 << 4;
    }
}

wire_enum! {
    /// A remote-control action.
    RcAction: u16 {
        /// Toggle power.
        POWER_TOGGLE = 0;
        /// Power on.
        POWER_ON = 1;
        /// Power off.
        POWER_OFF = 2;
        /// Volume down.
        VOLUME_DOWN = 3;
        /// Volume up.
        VOLUME_UP = 4;
        /// Toggle mute.
        VOLUME_MUTE = 5;
    }
}

wire_enum! {
    /// A remote-control key code (CEC or IR).
    RcKey: u16 {
        /// Digit 0.
        NUM0 = 0;
        /// Digit 1.
        NUM1 = 1;
        /// Digit 2.
        NUM2 = 2;
        /// Digit 3.
        NUM3 = 3;
        /// Digit 4.
        NUM4 = 4;
        /// Digit 5.
        NUM5 = 5;
        /// Digit 6.
        NUM6 = 6;
        /// Digit 7.
        NUM7 = 7;
        /// Digit 8.
        NUM8 = 8;
        /// Digit 9.
        NUM9 = 9;
        /// Confirm the highlighted item.
        SELECT = 10;
        /// Go back one step.
        BACK = 11;
        /// Navigate up.
        UP = 12;
        /// Navigate down.
        DOWN = 13;
        /// Navigate left.
        LEFT = 14;
        /// Navigate right.
        RIGHT = 15;
        /// Open the main menu.
        MENU = 16;
        /// Open the content menu.
        CONTENT_MENU = 17;
        /// Next channel.
        CHANNEL_UP = 18;
        /// Previous channel.
        CHANNEL_DOWN = 19;
        /// Start playback.
        PLAY = 20;
        /// Pause playback.
        PAUSE = 21;
        /// Stop playback.
        STOP = 22;
        /// Start recording.
        RECORD = 23;
        /// Fast forward.
        FAST_FORWARD = 24;
        /// Rewind.
        REWIND = 25;
        /// Red colour key.
        RED = 26;
        /// Green colour key.
        GREEN = 27;
        /// Yellow colour key.
        YELLOW = 28;
        /// Blue colour key.
        BLUE = 29;
        /// Open help.
        HELP = 30;
        /// Show information.
        INFORMATION = 31;
        /// Open teletext.
        TEXT = 32;
        /// Open the programme guide.
        GUIDE = 33;
        /// Open video on demand.
        VIDEO_ON_DEMAND = 34;
        /// Return to the previous channel.
        PREVIOUS_CHANNEL = 80;
        /// Toggle 3D mode.
        MODE_3D = 81;
        /// Toggle subtitles.
        SUBTITLE = 82;
        /// Select an audio track.
        SOUND_SELECT = 83;
        /// Select an input.
        INPUT_SELECT = 84;
        /// Eject the medium.
        EJECT = 85;
        /// Next chapter.
        NEXT_CHAPTER = 86;
        /// Previous chapter.
        PREV_CHAPTER = 87;
        /// Open interactive services.
        INTERACTIVE = 128;
        /// Open search.
        SEARCH = 129;
        /// Sky home key.
        SKY = 130;
        /// Base of the range carrying a raw CEC user-control code.
        CUSTOM_CEC = 1280;
        /// Base of the range carrying a raw Sky key code.
        CUSTOM_SKY = 2048;
    }
}

wire_enum! {
    /// The remote-control protocol of a connected sink or source.
    RcType: u8 {
        /// Infrared.
        IR = 0;
        /// HDMI CEC.
        CEC = 1;
        /// Sky UK over IP.
        SKY_UK = 2;
        /// TiVo.
        TIVO = 3;
        /// Kodi.
        KODI = 4;
        /// Dish.
        DISH = 5;
        /// DirecTV.
        DIRECTV = 6;
        /// Another MX Remote device.
        MX_REMOTE = 7;
    }
}

impl fmt::Display for RcType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match *self {
            Self::IR => "IR",
            Self::CEC => "CEC",
            Self::SKY_UK => "Sky",
            Self::TIVO => "TiVo",
            Self::KODI => "Kodi",
            Self::DISH => "Dish",
            Self::DIRECTV => "DirecTV",
            Self::MX_REMOTE => "MX-Remote",
            _ => "Unknown",
        };
        f.write_str(name)
    }
}

wire_enum! {
    /// An EDID preset selectable on an HDMI input.
    EdidProfile: u16 {
        /// 1080p with stereo audio.
        STEREO_1080P = 0;
        /// A fixed EDID stored on the device.
        FIXED = 1;
        /// 4K.
        UHD_4K = 2;
        /// 1080p with 5.1 audio.
        SURROUND51_1080P = 3;
        /// 720p.
        HD_720P = 4;
        /// 1080p with 7.1 audio.
        SURROUND71_1080P = 5;
        /// 4K with 7.1 audio.
        SURROUND71_4K = 6;
        /// 4K HDR with stereo audio.
        HDR_STEREO_4K = 7;
        /// 4K HDR with 7.1 audio.
        HDR_SURROUND71_4K = 8;
        /// 4K HDR, audio to the AVR only.
        HDR_AVR_ONLY_4K = 9;
        /// The lowest common denominator of the connected sinks.
        LOWEST_COMMON = 10;
        /// The lowest common denominator of every sink, connected or not.
        LOWEST_COMMON_ALL = 11;
        /// 4K HDR with Dolby Atmos.
        HDR_ATMOS_4K = 12;
        /// Copy the EDID of sink 1; the range runs to [`EdidProfile::SINK_32`].
        SINK_1 = 101;
        /// Copy the EDID of sink 32; the range starts at [`EdidProfile::SINK_1`].
        SINK_32 = 132;
        /// Base of the range carrying a user-supplied EDID.
        CUSTOM_0 = 500;
        /// The device reports no profile.
        UNKNOWN = 0xFFF;
    }
}

impl fmt::Display for EdidProfile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match *self {
            Self::STEREO_1080P => "1080p stereo",
            Self::FIXED => "fixed",
            Self::UHD_4K => "4K",
            Self::SURROUND51_1080P => "1080p 5.1",
            Self::HD_720P => "720p",
            Self::SURROUND71_1080P => "1080p 7.1",
            Self::SURROUND71_4K => "4K 7.1",
            Self::HDR_STEREO_4K => "4K HDR Stereo",
            Self::HDR_SURROUND71_4K => "4K HDR 7.1",
            Self::HDR_AVR_ONLY_4K => "4K HDR AVR",
            Self::LOWEST_COMMON => "lowest common denominator",
            Self::LOWEST_COMMON_ALL => "lowest common denominator (all sinks)",
            Self::HDR_ATMOS_4K => "4K HDR Dolby Atmos",
            _ => {
                if self.0 >= Self::SINK_1.0 && self.0 <= Self::SINK_32.0 {
                    return write!(f, "copy from sink #{}", self.0 - Self::SINK_1.0 + 1);
                }
                return write!(f, "custom #{}", self.0);
            }
        };
        f.write_str(name)
    }
}

wire_enum! {
    /// A firmware component.
    FirmwareType: u8 {
        /// The component is not known.
        UNKNOWN = 0;
        /// The FPGA bitstream.
        FPGA = 1;
        /// The Linux system image.
        LINUX = 2;
        /// A loadable overlay.
        LOADING_OVERLAY = 3;
    }
}

impl fmt::Display for FirmwareType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match *self {
            Self::FPGA => "FPGA",
            Self::LINUX => "Linux",
            Self::LOADING_OVERLAY => "Loading Overlay",
            _ => "Unknown",
        };
        f.write_str(name)
    }
}

wire_enum! {
    /// The negotiated speed of a network port.
    UtpLinkSpeed: u8 {
        /// The device reports no speed.
        UNKNOWN = 0;
        /// 10Mbit/s.
        SPEED_10M = 1;
        /// 100Mbit/s.
        SPEED_100M = 2;
        /// 200Mbit/s.
        SPEED_200M = 3;
        /// 1Gbit/s.
        SPEED_1G = 4;
    }
}

impl fmt::Display for UtpLinkSpeed {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match *self {
            Self::SPEED_10M => "10Mbit/s",
            Self::SPEED_100M => "100Mbit/s",
            Self::SPEED_200M => "200Mbit/s",
            Self::SPEED_1G => "1Gbit/s",
            _ => "Unknown",
        };
        f.write_str(name)
    }
}

wire_enum! {
    /// The window layout of a multiviewer.
    MultiviewerViewMode: u8 {
        /// The device reports no layout.
        UNKNOWN = 0;
        /// One full-screen window.
        SINGLE = 1;
        /// Picture in picture.
        PIP = 2;
        /// Two windows, large.
        TWO_SCREEN_LARGE = 3;
        /// Two windows, small.
        TWO_SCREEN_SMALL = 4;
        /// Three windows, large.
        THREE_SCREEN_LARGE = 5;
        /// Three windows, small.
        THREE_SCREEN_SMALL = 6;
        /// Four windows, large.
        FOUR_SCREEN_LARGE = 7;
        /// Four windows, small.
        FOUR_SCREEN_SMALL = 8;
    }
}

wire_enum! {
    /// The corner a multiviewer places its picture-in-picture window in.
    MultiviewerPipPosition: u8 {
        /// The device reports no position.
        UNKNOWN = 0;
        /// Top left.
        LEFT_TOP = 1;
        /// Bottom left.
        LEFT_BOTTOM = 2;
        /// Top right.
        RIGHT_TOP = 3;
        /// Bottom right.
        RIGHT_BOTTOM = 4;
    }
}

wire_enum! {
    /// The size of a multiviewer's picture-in-picture window.
    MultiviewerPipSize: u8 {
        /// The device reports no size.
        UNKNOWN = 0;
        /// Small.
        SMALL = 1;
        /// Medium.
        MEDIUM = 2;
        /// Large.
        LARGE = 3;
    }
}

wire_enum! {
    /// The resolution and refresh rate a multiviewer drives its output at.
    MultiviewerOutputMode: u8 {
        /// The device reports no output mode.
        UNKNOWN = 0;
        /// 4096x2160p60.
        DCI4K_P60 = 1;
        /// 4096x2160p50.
        DCI4K_P50 = 2;
        /// 3840x2160p60.
        UHD_P60 = 3;
        /// 3840x2160p50.
        UHD_P50 = 4;
        /// 3840x2160p30.
        UHD_P30 = 5;
        /// 3840x2160p25.
        UHD_P25 = 6;
        /// 1920x1200p60, reduced blanking.
        WUXGA_P60_RB = 7;
        /// 1920x1080p60.
        HD1080_P60 = 8;
        /// 1920x1080p50.
        HD1080_P50 = 9;
        /// 1360x768p60.
        WXGA_P60 = 10;
        /// 1280x800p60.
        WXGA800_P60 = 11;
        /// 1280x720p60.
        HD720_P60 = 12;
        /// 1280x720p50.
        HD720_P50 = 13;
        /// 1024x768p60.
        XGA_P60 = 14;
    }
}

wire_enum! {
    /// The HDCP version a multiviewer output negotiates.
    MultiviewerHdcpMode: u8 {
        /// The device reports no HDCP mode.
        UNKNOWN = 0;
        /// HDCP 1.4.
        V14 = 1;
        /// HDCP 2.2.
        V22 = 2;
    }
}

wire_enum! {
    /// The IT-content flag a multiviewer sets on its output.
    MultiviewerItcMode: u8 {
        /// The device reports no IT-content mode.
        UNKNOWN = 0;
        /// Video content.
        VIDEO = 1;
        /// PC content.
        PC = 2;
    }
}

wire_enum! {
    /// The EDID template a multiviewer presents to its sources.
    MultiviewerEdidTemplate: u8 {
        /// The device reports no template.
        UNKNOWN = 0;
    }
}

wire_enum! {
    /// The aspect ratio a multiviewer scales its windows to.
    MultiviewerAspectRatio: u8 {
        /// The device reports no aspect ratio.
        UNKNOWN = 0;
        /// Fill the window.
        FULL = 1;
        /// 16:9.
        RATIO_16_9 = 2;
    }
}

wire_enum! {
    /// A multiviewer setting that is on, off, or not reported.
    MultiviewerBool: u8 {
        /// Off.
        OFF = 0;
        /// On.
        ON = 1;
        /// The device reports no value.
        UNKNOWN = 0xFF;
    }
}

wire_enum! {
    /// One of a multiviewer's four inputs.
    ///
    /// The wire numbers the inputs from zero and this type from one, so that
    /// zero can mean "not reported" the way it does for every other
    /// multiviewer setting.
    MultiviewerSource: u8 {
        /// The device reports no source.
        UNKNOWN = 0;
        /// Input 1.
        INPUT_1 = 1;
        /// Input 2.
        INPUT_2 = 2;
        /// Input 3.
        INPUT_3 = 3;
        /// Input 4.
        INPUT_4 = 4;
    }
}

impl MultiviewerSource {
    /// Reads a zero-based wire value, mapping anything past input 4 to
    /// [`MultiviewerSource::UNKNOWN`].
    pub(crate) const fn from_zero_based(value: u8) -> Self {
        if value > 3 {
            Self::UNKNOWN
        } else {
            Self(value + 1)
        }
    }
}

impl MultiviewerBool {
    /// Reads a wire value, mapping anything but 0 and 1 to
    /// [`MultiviewerBool::UNKNOWN`].
    pub(crate) const fn from_wire_tristate(value: u8) -> Self {
        if value > 1 {
            Self::UNKNOWN
        } else {
            Self(value)
        }
    }
}

wire_enum! {
    /// The 2-byte `mxr_signal_type` carried in scaling configs and bay signal
    /// reports.
    ///
    /// Byte 0 is the CTA-861 short video descriptor, 0 when the signal is not
    /// HDMI. Byte 1 packs `color:4` in the low nibble, then `non_int:1` and
    /// `bpp:3` above it.
    MxrSignalType: u16 {
        /// No signal format was reported.
        NONE = 0;
    }
}

/// The bpp index a sender writes when it has no bit depth to report.
const SIG_BPP_UNSET: u16 = 5;

impl MxrSignalType {
    /// The CTA-861 short video descriptor, 0 when the signal is not HDMI.
    pub const fn svd(self) -> u8 {
        (self.0 & 0xFF) as u8
    }

    /// The colour space.
    pub const fn colour_space(self) -> u8 {
        ((self.0 >> 8) & 0xF) as u8
    }

    /// Whether the frame rate carries a 1000/1001 clock.
    pub const fn is_non_integer(self) -> bool {
        self.0 & (1 << 12) != 0
    }

    /// The raw bpp index as carried on the wire. The field is an index, not a
    /// bit depth; [`MxrSignalType::bpp`] converts it.
    pub const fn bpp_index(self) -> u8 {
        ((self.0 >> 13) & 0x7) as u8
    }

    /// The bit depth the bpp index stands for, `None` when unknown or unset.
    pub const fn bpp(self) -> Option<u8> {
        match self.bpp_index() {
            1 => Some(8),
            2 => Some(10),
            3 => Some(12),
            4 => Some(16),
            _ => None,
        }
    }

    /// Reports whether the signal type carries anything but the unset sentinel.
    pub const fn is_set(self) -> bool {
        self.bpp_index() as u16 != SIG_BPP_UNSET
    }
}

impl fmt::Display for MxrSignalType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if !self.is_set() {
            return f.write_str("unset");
        }
        match self.bpp() {
            Some(bpp) => write!(
                f,
                "svd {}, color {}, {}bpp",
                self.svd(),
                self.colour_space(),
                bpp
            ),
            None => write!(f, "svd {}, color {}", self.svd(), self.colour_space()),
        }
    }
}
