// Author: Lars Op den Kamp (lars@opdenkamp-it.nl)
// Copyright (c) 2026 Op den Kamp IT Solutions

//! V2IP stream configuration, statistics and the sink-side route.

use core::fmt;
use std::net::Ipv4Addr;

use crate::wire::{
    DeviceUid, MxrSignalType, V2ipColourSpace, V2IP_AUDIO_DEFAULT_CHANNELS,
    V2IP_AUDIO_DEFAULT_SAMPLE_RATE, V2IP_DSCP_MAX, V2IP_DSCP_SET,
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
/// that does not initialise the configuration it broadcasts builds this frame
/// from an uninitialised stack local and ORs its flags onto whatever was
/// there.
pub const SCALING_FLAGS_DEFINED: u8 =
    SCALING_FLAG_MODE_VALID | SCALING_FLAG_OPTIONS_VALID | SCALING_FLAG_AUTO_SCALING;

/// Lowest refresh rate a V2IP output stage accepts, in Hz.
///
/// A receiver replaces anything outside
/// [`V2IP_SCALING_REFRESH_MIN`]..=[`V2IP_SCALING_REFRESH_MAX`] with 50 rather
/// than refusing the write, so 0 asks for 50Hz here instead of asking for
/// nothing.
pub const V2IP_SCALING_REFRESH_MIN: u16 = 24;

/// Highest refresh rate a V2IP output stage accepts, in Hz. See
/// [`V2IP_SCALING_REFRESH_MIN`].
pub const V2IP_SCALING_REFRESH_MAX: u16 = 120;

/// The output format to scale a V2IP sink to.
///
/// Built from a depth and a colour space rather than from a packed signal-type
/// word, so the word a caller sends cannot carry the unset bpp index a sink
/// reports while it has no mode configured.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct V2ipOutputMode {
    /// The CTA-861 short video descriptor to output.
    pub svd: u8,
    /// Bit depth: 8, 10 or 12.
    pub depth: u8,
    /// The colour space to output.
    pub colour: V2ipColourSpace,
    /// Refresh rate in Hz, [`V2IP_SCALING_REFRESH_MIN`] to
    /// [`V2IP_SCALING_REFRESH_MAX`].
    pub refresh: u16,
}

impl V2ipOutputMode {
    /// Reports whether a sink will take this mode, or why it will not.
    ///
    /// Checked here because a sink checks it and then says nothing: every value
    /// this rejects is one the receiver decodes cleanly and drops, leaving a
    /// caller with a send that succeeded and a setting that did not move.
    ///
    /// Passing is not a guarantee. A sink also weighs the format against the
    /// EDID of the display attached to it and against what its own clock and
    /// output stage can produce, and none of that is knowable from here.
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.svd == 0 {
            return Err("svd 0 is how a mode is cleared, not a mode to set");
        }
        if crate::lookup_svd(u16::from(self.svd)).is_none() {
            return Err("the svd names no known video descriptor");
        }
        if MxrSignalType::bpp_index_for_depth(self.depth).is_none() {
            return Err("a V2IP output stage takes 8, 10 or 12 bits per pixel");
        }
        if self.colour > V2ipColourSpace::YCBCR420 {
            return Err("the colour space names none of RGB, 4:4:4, 4:2:2 or 4:2:0");
        }
        if !(V2IP_SCALING_REFRESH_MIN..=V2IP_SCALING_REFRESH_MAX).contains(&self.refresh) {
            return Err("the refresh rate is outside 24..=120Hz");
        }
        Ok(())
    }

    /// The packed signal type a scaling write carries for this mode.
    ///
    /// Call [`V2ipOutputMode::validate`] first: an unvalidated depth packs as
    /// the index for "no depth", which a receiver drops.
    pub(crate) fn to_signal_type(self) -> MxrSignalType {
        MxrSignalType::from_parts(
            self.svd,
            self.colour.to_wire(),
            MxrSignalType::bpp_index_for_depth(self.depth).unwrap_or(0),
        )
    }
}

impl fmt::Display for V2ipOutputMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "svd {}, colour {}, {}bpp, {}Hz",
            self.svd,
            self.colour.to_wire(),
            self.depth,
            self.refresh
        )
    }
}

impl V2ipScalingSettings {
    /// The mode this sink is configured to scale to, `None` when it has none.
    ///
    /// The two are distinct on the wire: a sink with no mode configured leaves
    /// [`SCALING_FLAG_MODE_VALID`] clear, and never sets it over a zero mode.
    ///
    /// Trust it only where the sender reports
    /// [`crate::DeviceInfo::config_initialised`]. Firmware without that builds
    /// this block over uninitialised stack, where the valid bit itself is
    /// noise.
    pub const fn configured_mode(&self) -> Option<(MxrSignalType, u16)> {
        if self.flags & SCALING_FLAG_MODE_VALID == 0 {
            return None;
        }
        Some((self.mode, self.refresh))
    }

    /// Whether the output scales automatically, `None` when the sender did not
    /// say.
    pub const fn auto_scaling(&self) -> Option<bool> {
        if self.flags & SCALING_FLAG_OPTIONS_VALID == 0 {
            return None;
        }
        Some(self.flags & SCALING_FLAG_AUTO_SCALING != 0)
    }

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

/// The sink-side route a V2IP device is subscribed to, as the mesh believes it.
///
/// A route request addressed to the device sets this the moment it is seen,
/// which is what every device on the mesh does with one. So a request the
/// device refused, or that reached it while it was offline, reads back here as
/// though it had taken effect. Only the device's own configuration report
/// confirms a route, and it sends that on its own schedule rather than in reply.
///
/// **Addresses that read as unset mean "no route, or the sink could not work
/// one out" - never "definitely not subscribed".** This block is the one part
/// of a device configuration with no validity marker of its own, so a sender
/// with nothing to say sends zeros and every receiver stores them. A sender
/// leaves it empty when its own stream configuration does not resolve, and that
/// covers more than having no route: a selected source whose record has not
/// arrived yet, which is the state after a restart at either end, missing audio
/// bay configuration, or any of the three streams failing its validity check.
/// The audio format has a second gate of its own, so it can be absent while the
/// addresses are not.
///
/// This is worth expecting rather than guarding against. Any scaling change
/// makes the device rebuild and rebroadcast this block, and a write aimed at a
/// remote bay sends it zeroed however it was requested - so the empty reading
/// arrives most often during exactly the no-signal troubleshooting that
/// prompted the change. A device's periodic report puts a real route back
/// within a minute of it having one.
///
/// An empty reading is applied rather than ignored on purpose. A sink that has
/// genuinely dropped its route sends the same zeros, and so does every report
/// after it, so refusing them would cache a route that nothing later could ever
/// clear.
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

/// Why a decoder reports the state it does.
///
/// The primary cause only. Several causes can be true at once, and which of
/// them lands here is a fixed priority order in the firmware that the numbering
/// does not express: these values are identities, not ranks, and comparing or
/// ordering them says nothing. Ask [`V2ipDecoderReport::has_cause`] whether a
/// particular cause applies - a test against this field answers "is this the
/// one that won" instead, which is a different question.
///
/// Firmware adds causes, so the wire value is carried as it arrived: folding an
/// unrecognised one onto a named cause would report a fault this library
/// invented. Appending one cannot reorder the existing priorities.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct V2ipDecoderReason(u8);

impl V2ipDecoderReason {
    /// Decoding normally.
    pub const OK: Self = Self(0);
    /// No packets are arriving.
    pub const NO_PACKETS: Self = Self(1);
    /// Packets are arriving, degraded.
    pub const PACKETS_DEGRADED: Self = Self(2);
    /// No format could be recovered from the codestream.
    pub const NO_FORMAT: Self = Self(3);
    /// The recovered format is not the one the sink is configured for.
    pub const FORMAT_MISMATCH: Self = Self(4);
    /// The configured output format was refused.
    pub const FORMAT_REJECTED: Self = Self(5);
    /// The converter watchdog is holding the stream back.
    pub const DECODER_BLOCKED: Self = Self(6);
    /// A source switch is in progress: a step in an operation someone asked
    /// for, rather than a fault.
    pub const SWITCH_PENDING: Self = Self(7);
    /// PTP is unlocked. That costs audio alone; the picture is unaffected.
    pub const PTP_UNLOCKED: Self = Self(8);
    /// The pipeline is rebuilding after the HDMI transmitter stayed unlocked.
    ///
    /// The picture is down, and has been for five seconds before this can
    /// appear: the sender debounces the unlocked reading for that long, so
    /// this never reports a transient. Unlike [`Self::SWITCH_PENDING`] nobody
    /// asked for it.
    ///
    /// The debounce restarts each time it elapses, so this holding across
    /// reports is a restart loop rather than one event, and that is what to
    /// escalate on.
    ///
    /// It sits near the bottom of the priority order, below every input-side
    /// cause, so a rebuilding pipeline names one of those in
    /// [`V2ipDecoderReport::reason`] and carries this in
    /// [`V2ipDecoderReport::flags`] alone - always, rather than briefly.
    ///
    /// It is evaluated only while no format change is in progress. Across a
    /// switch it holds its previous value and clears on the first reading
    /// after the change settles, which [`V2ipDecoderReport::updates`] cannot
    /// distinguish: a value carried forward is still a stored reading.
    pub const TX_BRIDGE_UNLOCKED: Self = Self(9);
    /// The sink is configured but switched off, so no stream is expected.
    ///
    /// This outranks every other cause: whenever it applies it is what
    /// [`V2ipDecoderReport::reason`] carries.
    ///
    /// **The causes beneath it stay set in [`V2ipDecoderReport::flags`].** A
    /// sink switched off while it was running keeps the bits the decoder
    /// genuinely observed on the way down - no packets, no format - so a
    /// classifier that tests a fault mask over the whole word calls a
    /// deliberately disabled sink broken. Ask for this cause first and stop
    /// there; the bits below it describe what was seen, not a fault to report.
    ///
    /// This says nothing about geometry, in either direction. The decoder
    /// reports what it currently detects whatever the cause, so a switched-off
    /// sink still detecting a codestream carries a real geometry, and a zero
    /// one means the decoder has nothing rather than that the sink is off.
    ///
    /// Older senders never report this and give [`Self::NO_PACKETS`] for a
    /// disabled sink instead, indistinguishable from one whose source has
    /// died. So an absent [`Self::IDLE`] is not evidence a sink is enabled,
    /// and **nothing in this block answers enablement**: it carries no such
    /// field, and the answer comes from `V2IP_DEVICE_CFG` or the device's HTTP
    /// status.
    pub const IDLE: Self = Self(10);

    /// Wraps a raw wire value, including one this library has no name for.
    pub const fn from_wire(value: u8) -> Self {
        Self(value)
    }

    /// Returns the raw wire value.
    pub const fn to_wire(self) -> u8 {
        self.0
    }
}

impl fmt::Display for V2ipDecoderReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Self::OK => f.write_str("ok"),
            Self::NO_PACKETS => f.write_str("no packets"),
            Self::PACKETS_DEGRADED => f.write_str("packets degraded"),
            Self::NO_FORMAT => f.write_str("no format recovered"),
            Self::FORMAT_MISMATCH => f.write_str("format mismatch"),
            Self::FORMAT_REJECTED => f.write_str("format rejected"),
            Self::DECODER_BLOCKED => f.write_str("decoder blocked"),
            Self::SWITCH_PENDING => f.write_str("switch pending"),
            Self::PTP_UNLOCKED => f.write_str("PTP unlocked"),
            Self::TX_BRIDGE_UNLOCKED => f.write_str("TX bridge unlocked"),
            Self::IDLE => f.write_str("idle"),
            Self(v) => write!(f, "reason {v}"),
        }
    }
}

/// The colour space a decoder recovered from a codestream.
///
/// Zero is RGB and is also what a decoder with nothing to decode reports, so no
/// value here means "no signal" - [`V2ipDecoderReport::has_geometry`] is what
/// answers that.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct V2ipDecoderFormat(u16);

impl V2ipDecoderFormat {
    /// RGB.
    pub const RGB: Self = Self(0);
    /// YCbCr 4:4:4.
    pub const YCBCR_444: Self = Self(1);
    /// YCbCr 4:2:2.
    pub const YCBCR_422: Self = Self(2);
    /// YCbCr 4:2:0.
    pub const YCBCR_420: Self = Self(3);
    /// The decoder cannot name the format.
    ///
    /// 255, which is a value of its own rather than the 0xF a signal report
    /// uses for an unknown colour space. Mapping one onto the other yields a
    /// colour space the decoder never reported.
    pub const UNNAMED: Self = Self(255);

    /// Wraps a raw wire value, including one this library has no name for.
    pub const fn from_wire(value: u16) -> Self {
        Self(value)
    }

    /// Returns the raw wire value.
    pub const fn to_wire(self) -> u16 {
        self.0
    }
}

impl fmt::Display for V2ipDecoderFormat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Self::RGB => f.write_str("RGB"),
            Self::YCBCR_444 => f.write_str("YCbCr 4:4:4"),
            Self::YCBCR_422 => f.write_str("YCbCr 4:2:2"),
            Self::YCBCR_420 => f.write_str("YCbCr 4:2:0"),
            Self::UNNAMED => f.write_str("unnamed"),
            Self(v) => write!(f, "format {v}"),
        }
    }
}

/// What a sink's decoder recovered from the codestream it is being given.
///
/// This is what the decoder understood, read ahead of the scaler: the geometry
/// is unrounded and is not what the display is being sent. It separates "the
/// decoder understood the codestream" from "a picture came out the other end".
///
/// Colour depth is absent on purpose and will stay absent. The video processor
/// answers that one from a driver constant rather than from the codestream, so
/// there is no reading to carry; assert depth at the encoder's input bay
/// instead.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct V2ipDecoderReport {
    /// The primary cause of the state the decoder is in.
    pub reason: V2ipDecoderReason,
    /// The converter watchdog is holding the stream back.
    pub blocking: bool,
    /// The recovered picture width, and 0 when none was recovered.
    pub width: u16,
    /// The recovered picture height, and 0 when none was recovered.
    pub height: u16,
    /// The recovered colour space.
    pub format: V2ipDecoderFormat,
    /// How many readings the sink has stored. Monotonic, wrapping at 65535
    /// after some 36 hours, and never reset.
    ///
    /// A sink reads its video processor every two seconds and reports every
    /// second, so roughly every other report repeats a reading already seen:
    /// a frame arriving says nothing about how fresh the values in it are.
    /// This counter moves only when a reading is stored, so a processor that
    /// stopped answering leaves it still rather than implying a refresh.
    ///
    /// After pointing a sink at something else, wait for this to advance by
    /// two before trusting the geometry. It ticks when a reply lands rather
    /// than when a query is sent, so the first tick can carry an answer the
    /// processor read fractionally before the switch; the second cannot,
    /// because at most one query is outstanding at a time.
    pub updates: u16,
    /// Every cause that applies, as bit N for reason N. See
    /// [`Self::has_cause`].
    ///
    /// This is what to classify on, once [`V2ipDecoderReason::IDLE`] has been
    /// ruled out: that cause outranks the whole word and leaves the bits below
    /// it set, so a fault mask over `flags` reports a switched-off sink as
    /// broken. [`Self::reason`] carries whichever cause won a fixed priority
    /// contest, so a cause that is true can be absent from it while present
    /// here. Bit 0 is cleared by the sender, so an empty word means nothing
    /// beyond the primary cause applies.
    ///
    /// [`V2ipDecoderReason::NO_FORMAT`] and
    /// [`V2ipDecoderReason::FORMAT_MISMATCH`] are the two arms of one decision
    /// and never appear together.
    pub flags: u32,
    /// How many times the converter watchdog has triggered.
    pub blocked_count: u32,
}

impl V2ipDecoderReport {
    /// Reports whether the decoder recovered a geometry.
    ///
    /// This is what says whether the decoder is being given a codestream it
    /// understands. [`Self::format`] cannot: it reads
    /// [`V2ipDecoderFormat::RGB`] when nothing is arriving, which is
    /// indistinguishable from a real RGB reading.
    ///
    /// It answers that and nothing else. The reading is taken before any cause
    /// is decided, so it does not say whether the sink is switched on: a sink
    /// that is off can still detect a codestream, and one that is on can
    /// detect nothing.
    pub const fn has_geometry(&self) -> bool {
        self.width != 0 && self.height != 0
    }

    /// Reports whether `reason` is among the causes that apply.
    ///
    /// [`Self::reason`] carries the primary cause and `flags` carries all of
    /// them at once. Bit 0 is unused, so [`V2ipDecoderReason::OK`] is never
    /// among them and an empty word means nothing beyond the primary cause
    /// applies.
    pub const fn has_cause(&self, reason: V2ipDecoderReason) -> bool {
        let bit = reason.to_wire();
        bit > 0 && bit < u32::BITS as u8 && self.flags & (1 << bit) != 0
    }
}

/// What a statistics report says about the sink's decoder.
///
/// The three states are distinct answers and only [`Self::Answered`] carries a
/// reading. `valid` follows the sink being configured rather than the sink
/// being enabled, so a sink that is switched off still reports: as
/// [`V2ipDecoderReason::IDLE`], or from an older sender as
/// [`V2ipDecoderReason::NO_PACKETS`], which is the same reading a sink whose
/// source has died produces.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum V2ipDecoderDetail {
    /// The report carried no decoder block: the sender's firmware predates it.
    #[default]
    Absent,
    /// The block is there and the decoder has never answered. Every field it
    /// would carry is meaningless, so none is offered.
    NeverAnswered,
    /// A reading.
    Answered(V2ipDecoderReport),
}

impl V2ipDecoderDetail {
    /// The reading, for a caller that treats both of the other states as
    /// "nothing to show".
    pub const fn reading(self) -> Option<V2ipDecoderReport> {
        match self {
            Self::Answered(report) => Some(report),
            _ => None,
        }
    }
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
    /// What the sink's decoder recovered from the codestream it is decoding.
    pub decoder: V2ipDecoderDetail,
}
