// Author: Lars Op den Kamp (lars@opdenkamp-it.nl)
// Copyright (c) 2026 Op den Kamp IT Solutions

//! V2IP stream configuration, statistics and the sink-side route.

use core::fmt;
use std::net::Ipv4Addr;

use crate::wire::{
    DeviceUid, MxrSignalType, V2IP_AUDIO_DEFAULT_CHANNELS, V2IP_AUDIO_DEFAULT_SAMPLE_RATE,
    V2IP_DSCP_MAX, V2IP_DSCP_SET,
};

/// Which of a V2IP device's streams an address describes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum StreamKind {
    /// The video stream.
    #[default]
    Video,
    /// The audio stream.
    Audio,
    /// The ancillary-data stream.
    Anc,
    /// The audio-return stream.
    Arc,
}

impl fmt::Display for StreamKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Video => "video",
            Self::Audio => "audio",
            Self::Anc => "anc",
            Self::Arc => "arc",
        })
    }
}

/// A single multicast stream address.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct V2ipStreamSource {
    /// Which stream this address is for.
    pub kind: StreamKind,
    /// The multicast group.
    pub ip: Ipv4Addr,
    /// The destination UDP port.
    pub port: u16,
}

impl Default for V2ipStreamSource {
    fn default() -> Self {
        Self {
            kind: StreamKind::default(),
            ip: Ipv4Addr::UNSPECIFIED,
            port: 0,
        }
    }
}

impl V2ipStreamSource {
    /// Reports whether this carries a usable address: a multicast group and a
    /// non-zero port, both, matching firmware `mxr_v2ip_stream_valid`.
    pub const fn is_valid(&self) -> bool {
        self.ip.is_multicast() && self.port != 0
    }
}

impl fmt::Display for V2ipStreamSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}={}:{}", self.kind, self.ip, self.port)
    }
}

/// The streams advertised by a single V2IP source.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct V2ipStreamSources {
    /// The originating device, or the zero UID when it is not known.
    pub uid: DeviceUid,
    /// The video stream.
    pub video: V2ipStreamSource,
    /// The audio stream.
    pub audio: V2ipStreamSource,
    /// The ancillary-data stream.
    pub anc: V2ipStreamSource,
    /// The audio-return stream, when one is advertised.
    pub arc: Option<V2ipStreamSource>,
}

impl fmt::Display for V2ipStreamSources {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "video:{} audio:{} anc:{}",
            self.video, self.audio, self.anc
        )
    }
}

/// One multicast destination in a route the caller assembles.
///
/// The unspecified address sends the slot zeroed, naming no group for that
/// stream. It is not a way to leave one stream alone: the firmware decides
/// whether a sink has a manual route at all by reading the video and
/// ancillary slots, so an empty one of those disqualifies the whole route
/// rather than preserving anything - see
/// [`crate::Remote::select_source_addr`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct V2ipRouteTarget {
    /// The multicast group.
    pub ip: Ipv4Addr,
    /// The destination UDP port. Zero means the standard port for the stream
    /// this target is given as.
    pub port: u16,
}

impl Default for V2ipRouteTarget {
    fn default() -> Self {
        Self {
            ip: Ipv4Addr::UNSPECIFIED,
            port: 0,
        }
    }
}

impl V2ipRouteTarget {
    /// A target at the standard port for its stream.
    pub const fn new(ip: Ipv4Addr) -> Self {
        Self { ip, port: 0 }
    }

    /// The port to send, substituting `standard` for an unset one.
    pub(crate) const fn port_or(self, standard: u16) -> u16 {
        if self.port == 0 {
            standard
        } else {
            self.port
        }
    }
}

impl fmt::Display for V2ipRouteTarget {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.ip, self.port)
    }
}

/// The three streams a manual route points a V2IP sink at.
///
/// Fill in all three. The firmware decides whether a sink has a manual route
/// at all by looking at the video and ancillary groups, so a route carrying
/// only audio does not register as one and the sink falls back to the audio
/// source its mesh picks.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct V2ipRoute {
    /// The video stream, at [`crate::V2IP_PORT_VIDEO`] unless the port says otherwise.
    pub video: V2ipRouteTarget,
    /// The audio stream, at [`crate::V2IP_PORT_AUDIO`] unless the port says otherwise.
    pub audio: V2ipRouteTarget,
    /// The ancillary-data stream, at [`crate::V2IP_PORT_ANC`] unless the port says
    /// otherwise.
    pub anc: V2ipRouteTarget,
}

impl V2ipRoute {
    /// The three streams of one source, at the ports it advertises them on.
    pub fn of(sources: &V2ipStreamSources) -> Self {
        let target = |s: &V2ipStreamSource| V2ipRouteTarget {
            ip: s.ip,
            port: s.port,
        };
        Self {
            video: target(&sources.video),
            audio: target(&sources.audio),
            anc: target(&sources.anc),
        }
    }
}

impl fmt::Display for V2ipRoute {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "video:{} audio:{} anc:{}",
            self.video, self.audio, self.anc
        )
    }
}

/// The sample rate and channel count a V2IP audio stream is decoded at.
///
/// Fill both in. The firmware header calls zero "use the default", but the
/// path that applies a manual route substitutes nothing: it hands the pair to
/// the FPGA as it arrived, and the FPGA rejects a zero rate and takes the
/// whole switch down with it. [`V2ipAudioFormat::STANDARD`] is the pair the
/// header documents as the default.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct V2ipAudioFormat {
    /// Sample rate in Hz.
    pub sample_rate: u32,
    /// Channel count.
    pub channels: u8,
}

impl V2ipAudioFormat {
    /// 48kHz stereo: the rate and channel count the firmware header names as
    /// its default, which a caller has to send because firmware does not
    /// substitute it.
    pub const STANDARD: Self = Self {
        sample_rate: V2IP_AUDIO_DEFAULT_SAMPLE_RATE,
        channels: V2IP_AUDIO_DEFAULT_CHANNELS,
    };

    /// Encodes `v2ip_audio_format`: a `u32` rate, a channel byte and three
    /// reserved bytes, padded to the struct's 8-byte alignment.
    pub(crate) fn wire(&self) -> [u8; 8] {
        let r = self.sample_rate.to_le_bytes();
        [r[0], r[1], r[2], r[3], self.channels, 0, 0, 0]
    }
}

impl fmt::Display for V2ipAudioFormat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}Hz/{}ch", self.sample_rate, self.channels)
    }
}

/// A V2IP output's scaling mode, refresh rate and flags.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct V2ipScalingSettings {
    /// The signal type the output scales to.
    pub mode: MxrSignalType,
    /// Refresh rate in Hz.
    pub refresh: u16,
    /// The flag bits below.
    pub flags: u8,
}

/// Set when the frame carries a scaling mode and refresh rate.
pub const SCALING_FLAG_MODE_VALID: u8 = 1 << 0;

/// Set when the frame carries the scaling options.
pub const SCALING_FLAG_OPTIONS_VALID: u8 = 1 << 1;

/// Set when the output scales automatically.
pub const SCALING_FLAG_AUTO_SCALING: u8 = 1 << 7;

/// The flag bits that carry meaning.
///
/// Bits 2..6 are undefined and are not reliably zero on the wire: firmware
/// predating the fix builds this frame from an uninitialised stack local and
/// ORs its flags onto whatever was there.
pub const SCALING_FLAGS_DEFINED: u8 =
    SCALING_FLAG_MODE_VALID | SCALING_FLAG_OPTIONS_VALID | SCALING_FLAG_AUTO_SCALING;

impl V2ipScalingSettings {
    /// Folds a received scaling config onto the cached one, field by field.
    ///
    /// A write carries the mode or the options alone, so taking the block
    /// wholesale would drop whichever half was not being written. The options
    /// branch replaces the option bit rather than adding to it, which is what
    /// lets an options-only write clear [`SCALING_FLAG_AUTO_SCALING`].
    #[must_use]
    pub fn merge(self, previous: Self) -> Self {
        let mut out = previous;
        if self.flags & SCALING_FLAG_MODE_VALID != 0 {
            out.mode = self.mode;
            out.refresh = self.refresh;
            out.flags |= SCALING_FLAG_MODE_VALID;
        }
        if self.flags & SCALING_FLAG_OPTIONS_VALID != 0 {
            out.flags &= !SCALING_FLAG_AUTO_SCALING;
            out.flags |= SCALING_FLAG_OPTIONS_VALID;
            out.flags |= self.flags & SCALING_FLAG_AUTO_SCALING;
        }
        out
    }
}

/// The per-stream DSCP marking in a V2IP device configuration.
///
/// A stream whose wire byte carries no [`V2IP_DSCP_SET`] bit reads back as
/// `None`. Firmware treats the marking as all-or-nothing: it applies one only
/// when all three streams carry a value and otherwise falls back to the
/// default, so [`V2ipDscpConfig::is_complete`] reports which case a frame is in.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct V2ipDscpConfig {
    /// Marking for the video stream.
    pub video: Option<u8>,
    /// Marking for the audio stream.
    pub audio: Option<u8>,
    /// Marking for the ancillary-data stream.
    pub anc: Option<u8>,
}

impl V2ipDscpConfig {
    /// Reports whether all three streams carry a marking, which is what
    /// firmware requires before it applies one.
    pub const fn is_complete(&self) -> bool {
        self.video.is_some() && self.audio.is_some() && self.anc.is_some()
    }
}

impl fmt::Display for V2ipDscpConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match (self.video, self.audio, self.anc) {
            (Some(v), Some(a), Some(n)) => write!(f, "video:{v} audio:{a} anc:{n}"),
            _ => f.write_str("no marking"),
        }
    }
}

/// Decodes one `dscp` byte, or `None` when the byte carries no marking.
pub(crate) fn parse_dscp(raw: u8) -> Option<u8> {
    (raw & V2IP_DSCP_SET != 0).then_some(raw & V2IP_DSCP_MAX)
}

/// The local encoder/decoder configuration of a V2IP device.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DeviceV2ipDetails {
    /// The video stream this device sources.
    pub video: V2ipStreamSource,
    /// The audio stream this device sources.
    pub audio: V2ipStreamSource,
    /// The ancillary-data stream this device sources.
    pub anc: V2ipStreamSource,
    /// The audio-return stream this device sources.
    pub arc: V2ipStreamSource,

    /// Encoder rate in units of 10Mb/s, or `None` when the sender offered no
    /// rate.
    ///
    /// A rate-only write carries the rate on its own; every other controller
    /// write puts a value outside the valid range here, which firmware drops as
    /// invalid so that address-only and scaling writes leave the peer's rate
    /// alone.
    pub tx_rate: Option<u8>,

    /// Per-stream DSCP marking.
    pub dscp: V2ipDscpConfig,
    /// Scaling mode, refresh rate and flags.
    pub scaling: V2ipScalingSettings,
}

impl DeviceV2ipDetails {
    /// Reports whether the source block carries usable addresses.
    ///
    /// Firmware requires video and anc; audio is optional and is carried with
    /// them.
    pub const fn source_is_valid(&self) -> bool {
        self.video.is_valid() && self.anc.is_valid()
    }

    /// Folds a received device configuration onto the cached one.
    ///
    /// Every field is optional behind its own validity marker: the payload is
    /// zeroed before a sender fills in the one field it is writing, so a
    /// controller writing a TX rate sends zeroed addresses and a controller
    /// writing addresses sends an out-of-range rate. Firmware applies each
    /// field only behind its own test, so replacing the whole cached config on
    /// every frame would make the peer read back with its addresses, rate or
    /// marking gone.
    #[must_use]
    pub fn merge(mut self, previous: Option<Self>) -> Self {
        let Some(previous) = previous else {
            return self;
        };
        if !self.source_is_valid() {
            self.video = previous.video;
            self.audio = previous.audio;
            self.anc = previous.anc;
        }
        if !self.arc.is_valid() {
            self.arc = previous.arc;
        }
        if self.tx_rate.is_none() {
            self.tx_rate = previous.tx_rate;
        }
        // Firmware gates all three dscp bytes on the video byte's set bit
        // alone, and stores whatever the other two carry.
        if self.dscp.video.is_none() {
            self.dscp = previous.dscp;
        }
        self.scaling = self.scaling.merge(previous.scaling);
        self
    }
}

/// The sink-side route a V2IP device is currently subscribed to.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DeviceV2ipSink {
    /// The streams the sink subscribes to.
    pub addresses: V2ipStreamSources,
    /// The resolved audio format, when the sender reported one.
    pub audio_fmt: Option<V2ipAudioFormat>,
}

/// Transmitter stream statistics.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct V2ipTxStats {
    /// Video packets sent.
    pub video: u32,
    /// Audio packets sent.
    pub audio: u32,
    /// Ancillary-data packets sent.
    pub anc: u32,
    /// Times the stream went down.
    pub stream_down: u32,
    /// Transmit overflows.
    pub overflow: u32,
}

/// The health state of a V2IP decoder.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct V2ipDecoderState(u8);

impl V2ipDecoderState {
    /// The sink has not reported a state.
    pub const UNKNOWN: Self = Self(0);
    /// Decoding normally.
    pub const HEALTHY: Self = Self(1);
    /// Failed to decode.
    pub const BAD: Self = Self(2);
    /// Still coming up, which any sink subscribed to during a route change
    /// reports.
    pub const STARTING: Self = Self(3);

    /// Wraps a raw wire value, including one this library has no name for.
    pub const fn from_wire(value: u8) -> Self {
        Self(value)
    }

    /// Returns the raw wire value.
    pub const fn to_wire(self) -> u8 {
        self.0
    }

    /// Reports whether the decoder has reached a verdict.
    ///
    /// Only healthy and bad are verdicts. Testing for failure as "not healthy"
    /// reads a receiver that is merely coming up as one that failed to decode,
    /// which is what a sink reports for a moment after every route change.
    pub const fn is_settled(self) -> bool {
        matches!(self, Self::HEALTHY | Self::BAD)
    }
}

impl fmt::Display for V2ipDecoderState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Self::UNKNOWN => f.write_str("Unknown"),
            Self::HEALTHY => f.write_str("Healthy"),
            Self::BAD => f.write_str("Bad"),
            Self::STARTING => f.write_str("Starting"),
            Self(v) => write!(f, "state {v}"),
        }
    }
}

/// Receiver stream statistics.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct V2ipRxStats {
    /// Video packets received.
    pub video_total: u32,
    /// Video packets dropped.
    pub video_dropped: u32,
    /// Video sequence errors.
    pub video_seq_errors: u32,
    /// Watchdog timeouts.
    pub wdt_timeout: u32,
    /// Audio packets received.
    pub audio_total: u32,
    /// Audio packets dropped.
    pub audio_dropped: u32,
    /// Audio sequence errors.
    pub audio_seq_errors: u32,
    /// Ancillary-data packets received.
    pub anc_total: u32,
    /// Ancillary-data packets dropped.
    pub anc_dropped: u32,
    /// Ancillary-data sequence errors.
    pub anc_seq_errors: u32,
    /// The decoder's health state.
    pub decoder_state: V2ipDecoderState,
}

/// The cumulative and per-minute transmit and receive statistics.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct V2ipDeviceStats {
    /// Transmit totals since boot.
    pub tx: V2ipTxStats,
    /// Transmit counts over the last minute.
    pub tx_per_minute: V2ipTxStats,
    /// Receive totals since boot.
    pub rx: V2ipRxStats,
    /// Receive counts over the last minute.
    pub rx_per_minute: V2ipRxStats,
}
