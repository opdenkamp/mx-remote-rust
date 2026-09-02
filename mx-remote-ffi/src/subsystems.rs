// Author: Lars Op den Kamp (lars@opdenkamp-it.nl)
// Copyright (c) 2026 Op den Kamp IT Solutions

//! What a device reports about one of its subsystems.
//!
//! These are the values that do not fit in a device or bay snapshot: streams,
//! statistics, the audio tree, the network ports. Each has an event that says
//! it moved and a call here that says what it is now, which is why the events
//! carry only an identifier - what they would carry instead is this, and a
//! copy taken at event time could only be staler than a read.
//!
//! A call returns `MXR_ERR_NOT_REPORTED` when the device exists but has not
//! sent that subsystem, which is a different answer from a device that has
//! never been heard from at all.

use std::ffi::c_char;
use std::net::Ipv4Addr;

use mx_remote::{
    AmpDolbySettings, AudioEndpoint, DeviceV2ipDetails, DeviceV2ipSink, FirmwareVersion,
    MultiviewerStatus, NetworkPortStatus, RcSettings, StreamKind, TopologyEntry, UtpCableStatus,
    V2ipDeviceStats, V2ipRxStats, V2ipStreamSource, V2ipStreamSources, V2ipTilingConfig,
    V2ipTxStats, VctStatus, MULTIVIEWER_INPUTS,
};

use crate::abi::{fail, guard, mxr_result_t, mxr_uid_t, put_str};
use crate::control::mxr_audio_format_t;
use crate::info::{copy_into, not_heard_from, null_out, MXR_NAME_LEN, MXR_VERSION_LEN};
use crate::remote::{mxr_remote_t, with, MXR_IP_STRING_LEN};

/// How many inputs a multiviewer has.
///
/// Written as a literal because the generated header needs one, and checked
/// against the core crate's value below so the two cannot drift apart.
pub const MXR_MULTIVIEWER_INPUTS: usize = 4;

const _: () = assert!(MXR_MULTIVIEWER_INPUTS == MULTIVIEWER_INPUTS);

/// How many pairs a UTP cable diagnostic covers.
pub const MXR_UTP_PAIRS: usize = 4;

/// Which of a V2IP device's streams an address describes.
#[repr(i32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum mxr_stream_kind_t {
    /// The video stream.
    MXR_STREAM_VIDEO = 0,
    /// The audio stream.
    MXR_STREAM_AUDIO = 1,
    /// The ancillary-data stream.
    MXR_STREAM_ANC = 2,
    /// The audio-return stream.
    MXR_STREAM_ARC = 3,
}

impl From<StreamKind> for mxr_stream_kind_t {
    fn from(kind: StreamKind) -> Self {
        match kind {
            StreamKind::Video => Self::MXR_STREAM_VIDEO,
            StreamKind::Audio => Self::MXR_STREAM_AUDIO,
            StreamKind::Anc => Self::MXR_STREAM_ANC,
            StreamKind::Arc => Self::MXR_STREAM_ARC,
        }
    }
}

/// One multicast stream address.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct mxr_stream_source_t {
    /// Which stream this address is for.
    pub kind: mxr_stream_kind_t,
    /// The multicast group, as a dotted quad.
    pub ip: [c_char; MXR_IP_STRING_LEN],
    /// The destination UDP port.
    pub port: u16,
    /// Whether this carries a usable address: a multicast group and a non-zero
    /// port, both. A slot a device has not filled in is not an error, so this
    /// is what separates an address from an empty slot.
    pub valid: bool,
}

impl From<V2ipStreamSource> for mxr_stream_source_t {
    fn from(s: V2ipStreamSource) -> Self {
        let mut out = Self {
            kind: s.kind.into(),
            ip: [0; MXR_IP_STRING_LEN],
            port: s.port,
            valid: s.is_valid(),
        };
        put_str(&mut out.ip, &s.ip.to_string());
        out
    }
}

/// The streams one V2IP source advertises.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct mxr_stream_sources_t {
    /// The originating device, zero when it is not known.
    pub uid: mxr_uid_t,
    /// The video stream.
    pub video: mxr_stream_source_t,
    /// The audio stream.
    pub audio: mxr_stream_source_t,
    /// The ancillary-data stream.
    pub anc: mxr_stream_source_t,
    /// Whether an audio-return stream is advertised.
    pub has_arc: bool,
    /// The audio-return stream, meaningful only when `has_arc` is set.
    pub arc: mxr_stream_source_t,
}

impl From<V2ipStreamSources> for mxr_stream_sources_t {
    fn from(s: V2ipStreamSources) -> Self {
        Self {
            uid: s.uid.into(),
            video: s.video.into(),
            audio: s.audio.into(),
            anc: s.anc.into(),
            has_arc: s.arc.is_some(),
            arc: s.arc.unwrap_or_default().into(),
        }
    }
}

/// A V2IP device's own encoder configuration.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct mxr_v2ip_details_t {
    /// The video stream this device sources.
    pub video: mxr_stream_source_t,
    /// The audio stream this device sources.
    pub audio: mxr_stream_source_t,
    /// The ancillary-data stream this device sources.
    pub anc: mxr_stream_source_t,
    /// The audio-return stream this device sources.
    pub arc: mxr_stream_source_t,
    /// Encoder rate in units of 10Mb/s, or -1 when no rate has been reported.
    pub tx_rate: i16,
    /// DSCP marking for the video stream, or -1 when unmarked.
    pub dscp_video: i16,
    /// DSCP marking for the audio stream, or -1 when unmarked.
    pub dscp_audio: i16,
    /// DSCP marking for the ancillary-data stream, or -1 when unmarked.
    pub dscp_anc: i16,
    /// The signal type the output scales to.
    pub scaling_mode: u16,
    /// Refresh rate in Hz.
    pub scaling_refresh: u16,
    /// `MXR_SCALING_FLAG_*` bits. Bits outside those are undefined and are not
    /// reliably zero: firmware predating the fix builds this frame over an
    /// uninitialised stack local.
    pub scaling_flags: u8,
}

/// Set when the frame carries a scaling mode and refresh rate.
pub const MXR_SCALING_FLAG_MODE_VALID: u8 = 1 << 0;
/// Set when the frame carries the scaling options.
pub const MXR_SCALING_FLAG_OPTIONS_VALID: u8 = 1 << 1;
/// Set when the output scales automatically.
pub const MXR_SCALING_FLAG_AUTO_SCALING: u8 = 1 << 7;

/// The streams a V2IP sink is subscribed to.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct mxr_v2ip_sink_t {
    /// The streams the sink subscribes to.
    pub addresses: mxr_stream_sources_t,
    /// Whether the sender reported a resolved audio format.
    pub has_audio_format: bool,
    /// The audio format, meaningful only when `has_audio_format` is set.
    pub audio_format: mxr_audio_format_t,
}

/// Transmitter stream statistics.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct mxr_v2ip_tx_stats_t {
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

impl From<V2ipTxStats> for mxr_v2ip_tx_stats_t {
    fn from(s: V2ipTxStats) -> Self {
        Self {
            video: s.video,
            audio: s.audio,
            anc: s.anc,
            stream_down: s.stream_down,
            overflow: s.overflow,
        }
    }
}

/// Receiver stream statistics.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct mxr_v2ip_rx_stats_t {
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
    /// The decoder's health state: 0 unknown, 1 healthy, 2 bad, 3 starting.
    ///
    /// Only healthy and bad are verdicts. Reading failure as "not healthy"
    /// counts a decoder that is merely coming up as one that failed, which is
    /// what every sink reports for a moment after a route change.
    pub decoder_state: u8,
}

impl From<V2ipRxStats> for mxr_v2ip_rx_stats_t {
    fn from(s: V2ipRxStats) -> Self {
        Self {
            video_total: s.video_total,
            video_dropped: s.video_dropped,
            video_seq_errors: s.video_seq_errors,
            wdt_timeout: s.wdt_timeout,
            audio_total: s.audio_total,
            audio_dropped: s.audio_dropped,
            audio_seq_errors: s.audio_seq_errors,
            anc_total: s.anc_total,
            anc_dropped: s.anc_dropped,
            anc_seq_errors: s.anc_seq_errors,
            decoder_state: s.decoder_state.to_wire(),
        }
    }
}

/// A device's V2IP statistics, cumulative and over the last minute.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct mxr_v2ip_stats_t {
    /// Transmit totals since boot.
    pub tx: mxr_v2ip_tx_stats_t,
    /// Transmit counts over the last minute.
    pub tx_per_minute: mxr_v2ip_tx_stats_t,
    /// Receive totals since boot.
    pub rx: mxr_v2ip_rx_stats_t,
    /// Receive counts over the last minute.
    pub rx_per_minute: mxr_v2ip_rx_stats_t,
}

/// The window a sink is currently told to show.
///
/// This is the pollable view of a sink's window, not the persisted video wall
/// setting: on a sink running the wall module a write here is transient,
/// because that module pushes its own target window back within about a
/// second.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct mxr_tiling_config_t {
    /// The sink this window belongs to.
    pub target: mxr_uid_t,
    /// Window origin, horizontal.
    pub pos_x: u16,
    /// Window origin, vertical.
    pub pos_y: u16,
    /// Window width.
    pub width: u16,
    /// Window height.
    pub height: u16,
}

/// What a multiviewer reports about itself.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct mxr_multiviewer_status_t {
    /// The multiviewer.
    pub uid: mxr_uid_t,
    /// The source device mapped to each input.
    pub mappings: [mxr_uid_t; MXR_MULTIVIEWER_INPUTS],
    /// The MCU firmware version.
    pub mcu_version: [c_char; MXR_NAME_LEN],
    /// The scaler firmware version.
    pub scaler_version: [c_char; MXR_NAME_LEN],
    /// The view mode the hardware reports, which is its own numbering rather
    /// than `view_mode`'s.
    pub hw_view_mode: u8,
    /// The window layout.
    pub view_mode: u8,
    /// Which corner the picture-in-picture window sits in.
    pub pip_position: u8,
    /// The size of the picture-in-picture window.
    pub pip_size: u8,
    /// The output resolution.
    pub output_mode: u8,
    /// The HDCP mode.
    pub hdcp_mode: u8,
    /// The IT content flag.
    pub output_itc: u8,
    /// The EDID presented to sources.
    pub edid_template: u8,
    /// How a source is fitted into its window.
    pub aspect_ratio: u8,
    /// Whether automatic source switching is on.
    pub auto_switch: u8,
    /// Which window the audio is taken from.
    pub audio_source: u8,
    /// Whether a volume has been reported.
    pub has_audio_volume: bool,
    /// The output volume.
    pub audio_volume: u8,
    /// Whether the output is muted.
    pub audio_muted: u8,
    /// The source shown in each window.
    pub video_sources: [u8; MXR_MULTIVIEWER_INPUTS],
    /// Which window remote control is forwarded to.
    pub remote_control: u8,
}

/// One node of a device's audio tree.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct mxr_audio_endpoint_t {
    /// The endpoint's identifier on its device.
    pub id: u8,
    /// What the endpoint can do, as `MXR_AUDIO_*` bits.
    pub features: u32,
    /// Whether the endpoint carries a stream address.
    pub has_address: bool,
    /// The stream address, meaningful only when `has_address` is set.
    pub address: mxr_stream_source_t,
    /// The endpoint this one hangs off, or -1 at a root.
    pub parent: i16,
    /// How many children this endpoint has; read them with
    /// `mxr_audio_endpoint_children()`.
    pub child_count: usize,
    /// Whether the device reported which inputs are selectable.
    pub has_inputs_available: bool,
    /// Bitmask of the endpoints this one may be switched to.
    pub inputs_available: u32,
    /// Whether the device reported which input is selected.
    pub has_inputs_routed: bool,
    /// Bitmask of the endpoint this one is listening to.
    pub inputs_routed: u32,
    /// The device at the other end of the link, zero when unlinked.
    pub linked_device: mxr_uid_t,
    /// The endpoint at the other end of the link, or -1 when unlinked.
    pub linked_endpoint: i16,
}

impl From<&AudioEndpoint> for mxr_audio_endpoint_t {
    fn from(e: &AudioEndpoint) -> Self {
        Self {
            id: e.id,
            features: e.features.bits(),
            has_address: e.address.is_some(),
            address: e.address.unwrap_or_default().into(),
            // An endpoint id is a byte on the wire, so -1 cannot collide.
            parent: e.parent.map_or(-1, i16::from),
            child_count: e.children.len(),
            has_inputs_available: e.inputs_available.is_some(),
            inputs_available: e.inputs_available.unwrap_or(0),
            has_inputs_routed: e.inputs_routed.is_some(),
            inputs_routed: e.inputs_routed.unwrap_or(0),
            linked_device: e.linked_device.into(),
            linked_endpoint: e.linked_endpoint.map_or(-1, i16::from),
        }
    }
}

/// The diagnostic result for one UTP cable pair.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct mxr_cable_status_t {
    /// Whether the pair is wired with normal polarity.
    pub polarity: bool,
    /// Which pair this describes.
    pub pair: u8,
    /// Measured skew.
    pub skew: u32,
    /// Measured length.
    pub length: u32,
}

impl From<UtpCableStatus> for mxr_cable_status_t {
    fn from(c: UtpCableStatus) -> Self {
        Self {
            polarity: c.polarity,
            pair: c.pair,
            skew: c.skew,
            length: c.length,
        }
    }
}

/// The link state and diagnostics of one network port.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct mxr_network_port_t {
    /// Port number.
    pub port: u16,
    /// Port name.
    pub name: [c_char; MXR_NAME_LEN],
    /// Negotiated link speed.
    pub link_speed: u8,
    /// Whether the link negotiated full duplex.
    pub link_full_duplex: bool,
    /// The port's own address, empty when it has not reported one.
    pub ip: [c_char; MXR_IP_STRING_LEN],
    /// The IGMP querier the port sees, empty when it sees none.
    pub querier: [c_char; MXR_IP_STRING_LEN],
    /// Whether the port reported a hardware address.
    pub has_mac_address: bool,
    /// The hardware address, meaningful only when `has_mac_address` is set.
    pub mac_address: [u8; 6],
    /// Whether the port reported link errors.
    pub has_errors: bool,
    /// Input errors.
    pub in_error: bool,
    /// Input frame check errors.
    pub in_fcs_error: bool,
    /// Input collisions.
    pub in_collision: bool,
    /// Deferred transmissions.
    pub out_deferred: bool,
    /// Excessive transmissions.
    pub out_excessive: bool,
    /// Polarity errors.
    pub polarity_error: bool,
    /// Skew warning.
    pub skew_warning: bool,
    /// Length warning.
    pub length_warning: bool,
    /// Whether the port reported a virtual cable test.
    pub has_vct_status: bool,
    /// Whether each pair raised a warning, meaningful only when
    /// `has_vct_status` is set.
    pub vct_warning: [bool; MXR_UTP_PAIRS],
    /// How many entries of `cable_status` the port filled in.
    pub cable_status_count: usize,
    /// Cable diagnostics per pair.
    pub cable_status: [mxr_cable_status_t; MXR_UTP_PAIRS],
}

/// One device in a topology report.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct mxr_topology_entry_t {
    /// The device this entry describes.
    pub uid: mxr_uid_t,
    /// Bitmask of the devices it is connected to.
    pub mask: u32,
}

impl From<TopologyEntry> for mxr_topology_entry_t {
    fn from(e: TopologyEntry) -> Self {
        Self {
            uid: e.uid.into(),
            mask: e.mask,
        }
    }
}

/// One firmware component a device reports.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct mxr_firmware_version_t {
    /// Which component this describes.
    pub firmware_type: u8,
    /// Build timestamp, in seconds since the Unix epoch.
    pub timestamp: u32,
    /// Source revision hash.
    pub hash: u32,
    /// Human-readable version string.
    pub version: [c_char; MXR_VERSION_LEN],
}

/// A ProAmp8's Dolby settings.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct mxr_dolby_settings_t {
    /// 0 = standard, 1 = 3-zone Dolby, 2 = 4-zone Dolby.
    pub mode: u8,
    /// Whether PCM is up-mixed to 5.1 rather than passed through.
    pub pcm_upmix: bool,
    /// Whether a Dolby stream was detected.
    pub dolby_detected: bool,
    /// Whether up-mixing is currently running.
    pub pcm_upmix_active: bool,
}

impl From<AmpDolbySettings> for mxr_dolby_settings_t {
    fn from(s: AmpDolbySettings) -> Self {
        Self {
            mode: s.mode,
            pcm_upmix: s.pcm_upmix,
            dolby_detected: s.dolby_detected,
            pcm_upmix_active: s.pcm_upmix_active,
        }
    }
}

/// The remote-control configuration of a source bay.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct mxr_rc_settings_t {
    /// The device this configuration belongs to.
    pub target: mxr_uid_t,
    /// The control method, as the wire value.
    ///
    /// Zero is infrared, a method a bay really uses, so it is not a stand-in
    /// for "not reported". Check that `mxr_rc_settings()` returned `MXR_OK`
    /// before reading this: a device that has not sent its settings yet
    /// leaves the struct as the caller allocated it, and a zeroed one then
    /// reads as a bay set to infrared. `mxr_bay_info_t` answers the same
    /// question with a `has_rc_type` flag beside its `rc_type`.
    pub rc_target: u8,
    /// The control target's address, empty when unset.
    pub ip: [c_char; MXR_IP_STRING_LEN],
    /// Whether CEC is enabled.
    pub cec_enabled: bool,
    /// Whether CEC powers the sink on automatically.
    pub cec_auto_on: bool,
    /// Whether remote-control commands are forwarded.
    pub forward_rc: bool,
    /// Whether infrared is forwarded.
    pub forward_ir: bool,
    /// The driver state on the source, as the wire value. One above the last
    /// this library knows is passed through as it arrived.
    pub rc_status: u8,
    /// The driver-reported status string, empty when unknown.
    pub status_name: [c_char; MXR_NAME_LEN],
}

/// Writes an address into a fixed-width field, leaving it empty when there is
/// none.
fn put_ip(dst: &mut [c_char], ip: Option<Ipv4Addr>) {
    put_str(dst, &ip.map(|ip| ip.to_string()).unwrap_or_default());
}

/// Declares a getter for one subsystem of a device.
///
/// Each has the same three answers - no such device, the device has not sent
/// this, here it is - and writing them out once keeps a getter that answers
/// differently visible as one.
/// Writes a subsystem reading through `out`, or reports why there is none.
///
/// # Safety
///
/// `out` is null or points at a writable `T`.
unsafe fn fill<T>(
    r: &mxr_remote_t,
    uid: mxr_uid_t,
    out: *mut T,
    what: &str,
    value: Option<T>,
) -> mxr_result_t {
    if out.is_null() {
        return null_out(what);
    }
    match value {
        Some(value) => {
            // SAFETY: the caller guarantees a writable T, and it is not null.
            unsafe { *out = value };
            mxr_result_t::MXR_OK
        }
        None => not_reported(r, uid, what),
    }
}

/// Reports why a subsystem read found nothing: no such device, or a device
/// that has not sent this.
fn not_reported(r: &mxr_remote_t, uid: mxr_uid_t, what: &str) -> mxr_result_t {
    if r.remote.device(uid.into()).is_none() {
        return not_heard_from(uid);
    }
    fail(
        mxr_result_t::MXR_ERR_NOT_REPORTED,
        &format!("the device has reported no {what}"),
    )
}

/// Fills `out` with a device's V2IP statistics.
///
/// A device sends these only while subscribed; see
/// `mxr_subscribe_v2ip_stats()`.
///
/// # Safety
///
/// `remote` is null or a live handle, and `out` points at a writable
/// [`mxr_v2ip_stats_t`].
#[no_mangle]
pub unsafe extern "C" fn mxr_v2ip_stats(
    remote: *const mxr_remote_t,
    uid: mxr_uid_t,
    out: *mut mxr_v2ip_stats_t,
) -> mxr_result_t {
    // SAFETY: the caller guarantees a live handle or null.
    let handle = unsafe { remote.as_ref() };
    with(handle, |r| {
        let value = r
            .remote
            .v2ip_stats(uid.into())
            .map(|s: V2ipDeviceStats| mxr_v2ip_stats_t {
                tx: s.tx.into(),
                tx_per_minute: s.tx_per_minute.into(),
                rx: s.rx.into(),
                rx_per_minute: s.rx_per_minute.into(),
            });
        // SAFETY: the caller guarantees a writable mxr_v2ip_stats_t or null.
        unsafe { fill(r, uid, out, "V2IP statistics", value) }
    })
}

/// Fills `out` with a V2IP device's own encoder configuration.
///
/// # Safety
///
/// `remote` is null or a live handle, and `out` points at a writable
/// [`mxr_v2ip_details_t`].
#[no_mangle]
pub unsafe extern "C" fn mxr_v2ip_details(
    remote: *const mxr_remote_t,
    uid: mxr_uid_t,
    out: *mut mxr_v2ip_details_t,
) -> mxr_result_t {
    // SAFETY: the caller guarantees a live handle or null.
    let handle = unsafe { remote.as_ref() };
    with(handle, |r| {
        let value = r
            .remote
            .v2ip_details(uid.into())
            .map(|d: DeviceV2ipDetails| mxr_v2ip_details_t {
                video: d.video.into(),
                audio: d.audio.into(),
                anc: d.anc.into(),
                arc: d.arc.into(),
                // A rate and a marking are both bytes on the wire, so -1 cannot
                // collide with a value a device could report.
                tx_rate: d.tx_rate.map_or(-1, i16::from),
                dscp_video: d.dscp.video.map_or(-1, i16::from),
                dscp_audio: d.dscp.audio.map_or(-1, i16::from),
                dscp_anc: d.dscp.anc.map_or(-1, i16::from),
                scaling_mode: d.scaling.mode.to_wire(),
                scaling_refresh: d.scaling.refresh,
                scaling_flags: d.scaling.flags,
            });
        // SAFETY: the caller guarantees a writable mxr_v2ip_details_t or null.
        unsafe { fill(r, uid, out, "V2IP encoder configuration", value) }
    })
}

/// Fills `out` with the streams a V2IP sink is subscribed to.
///
/// # Safety
///
/// `remote` is null or a live handle, and `out` points at a writable
/// [`mxr_v2ip_sink_t`].
#[no_mangle]
pub unsafe extern "C" fn mxr_v2ip_sink(
    remote: *const mxr_remote_t,
    uid: mxr_uid_t,
    out: *mut mxr_v2ip_sink_t,
) -> mxr_result_t {
    // SAFETY: the caller guarantees a live handle or null.
    let handle = unsafe { remote.as_ref() };
    with(handle, |r| {
        let value = r
            .remote
            .v2ip_sink(uid.into())
            .map(|s: DeviceV2ipSink| mxr_v2ip_sink_t {
                addresses: s.addresses.into(),
                has_audio_format: s.audio_fmt.is_some(),
                audio_format: {
                    let f = s.audio_fmt.unwrap_or_default();
                    mxr_audio_format_t {
                        sample_rate: f.sample_rate,
                        channels: f.channels,
                    }
                },
            });
        // SAFETY: the caller guarantees a writable mxr_v2ip_sink_t or null.
        unsafe { fill(r, uid, out, "V2IP sink route", value) }
    })
}

/// Fills `out` with the window a sink is told to show.
///
/// # Safety
///
/// `remote` is null or a live handle, and `out` points at a writable
/// [`mxr_tiling_config_t`].
#[no_mangle]
pub unsafe extern "C" fn mxr_v2ip_tiling(
    remote: *const mxr_remote_t,
    uid: mxr_uid_t,
    out: *mut mxr_tiling_config_t,
) -> mxr_result_t {
    // SAFETY: the caller guarantees a live handle or null.
    let handle = unsafe { remote.as_ref() };
    with(handle, |r| {
        let value =
            r.remote
                .v2ip_tiling(uid.into())
                .map(|t: V2ipTilingConfig| mxr_tiling_config_t {
                    target: t.target.into(),
                    pos_x: t.pos_x,
                    pos_y: t.pos_y,
                    width: t.width,
                    height: t.height,
                });
        // SAFETY: the caller guarantees a writable mxr_tiling_config_t or null.
        unsafe { fill(r, uid, out, "window", value) }
    })
}

/// Fills `out` with what a multiviewer reports about itself.
///
/// # Safety
///
/// `remote` is null or a live handle, and `out` points at a writable
/// [`mxr_multiviewer_status_t`].
#[no_mangle]
pub unsafe extern "C" fn mxr_multiviewer_status(
    remote: *const mxr_remote_t,
    uid: mxr_uid_t,
    out: *mut mxr_multiviewer_status_t,
) -> mxr_result_t {
    // SAFETY: the caller guarantees a live handle or null.
    let handle = unsafe { remote.as_ref() };
    with(handle, |r| {
        let value = r.remote.multiviewer_status(uid.into()).map(multiviewer_of);
        // SAFETY: the caller guarantees a writable mxr_multiviewer_status_t or null.
        unsafe { fill(r, uid, out, "multiviewer status", value) }
    })
}

/// Copies a multiviewer's report into the C shape.
fn multiviewer_of(s: MultiviewerStatus) -> mxr_multiviewer_status_t {
    let mut out = mxr_multiviewer_status_t {
        uid: s.uid.into(),
        mappings: [mxr_uid_t::default(); MXR_MULTIVIEWER_INPUTS],
        mcu_version: [0; MXR_NAME_LEN],
        scaler_version: [0; MXR_NAME_LEN],
        hw_view_mode: s.hw_view_mode,
        view_mode: s.view_mode.to_wire(),
        pip_position: s.pip_position.to_wire(),
        pip_size: s.pip_size.to_wire(),
        output_mode: s.output_mode.to_wire(),
        hdcp_mode: s.hdcp_mode.to_wire(),
        output_itc: s.output_itc.to_wire(),
        edid_template: s.edid_template.to_wire(),
        aspect_ratio: s.aspect_ratio.to_wire(),
        auto_switch: s.auto_switch.to_wire(),
        audio_source: s.audio_source.to_wire(),
        has_audio_volume: s.audio_volume.is_some(),
        audio_volume: s.audio_volume.unwrap_or(0),
        audio_muted: s.audio_muted.to_wire(),
        video_sources: [0; MXR_MULTIVIEWER_INPUTS],
        remote_control: s.remote_control.to_wire(),
    };
    for (slot, uid) in out.mappings.iter_mut().zip(s.mappings) {
        *slot = uid.into();
    }
    for (slot, source) in out.video_sources.iter_mut().zip(s.video_sources) {
        *slot = source.to_wire();
    }
    put_str(&mut out.mcu_version, &s.mcu_version);
    put_str(&mut out.scaler_version, &s.scaler_version);
    out
}

/// Fills `out` with a ProAmp8's Dolby settings.
///
/// # Safety
///
/// `remote` is null or a live handle, and `out` points at a writable
/// [`mxr_dolby_settings_t`].
#[no_mangle]
pub unsafe extern "C" fn mxr_dolby_settings(
    remote: *const mxr_remote_t,
    uid: mxr_uid_t,
    out: *mut mxr_dolby_settings_t,
) -> mxr_result_t {
    // SAFETY: the caller guarantees a live handle or null.
    let handle = unsafe { remote.as_ref() };
    with(handle, |r| {
        let value = r
            .remote
            .dolby_settings(uid.into())
            .map(mxr_dolby_settings_t::from);
        // SAFETY: the caller guarantees a writable mxr_dolby_settings_t or null.
        unsafe { fill(r, uid, out, "Dolby settings", value) }
    })
}

/// Fills `out` with a source bay's remote-control configuration.
///
/// # Safety
///
/// `remote` is null or a live handle, and `out` points at a writable
/// [`mxr_rc_settings_t`].
#[no_mangle]
pub unsafe extern "C" fn mxr_rc_settings(
    remote: *const mxr_remote_t,
    uid: mxr_uid_t,
    out: *mut mxr_rc_settings_t,
) -> mxr_result_t {
    // SAFETY: the caller guarantees a live handle or null.
    let handle = unsafe { remote.as_ref() };
    with(handle, |r| {
        let value = r.remote.rc_settings(uid.into()).map(rc_settings_of);
        // SAFETY: the caller guarantees a writable mxr_rc_settings_t or null.
        unsafe { fill(r, uid, out, "remote-control configuration", value) }
    })
}

/// Copies a remote-control configuration into the C shape.
fn rc_settings_of(s: RcSettings) -> mxr_rc_settings_t {
    let mut out = mxr_rc_settings_t {
        target: s.target.into(),
        rc_target: s.rc_target,
        ip: [0; MXR_IP_STRING_LEN],
        cec_enabled: s.cec_enabled,
        cec_auto_on: s.cec_auto_on,
        forward_rc: s.forward_rc,
        forward_ir: s.forward_ir,
        rc_status: s.rc_status,
        status_name: [0; MXR_NAME_LEN],
    };
    put_ip(&mut out.ip, s.ip);
    put_str(&mut out.status_name, &s.status_name);
    out
}

/// Writes the streams a device's source bays advertise, and returns how many
/// there are.
///
/// Returns the full count even when it exceeds `cap`, so calling with `cap`
/// zero sizes the buffer.
///
/// # Safety
///
/// `remote` is null or a live handle, and `out` is null or points at `cap`
/// writable [`mxr_stream_sources_t`].
#[no_mangle]
pub unsafe extern "C" fn mxr_v2ip_sources(
    remote: *const mxr_remote_t,
    uid: mxr_uid_t,
    out: *mut mxr_stream_sources_t,
    cap: usize,
) -> usize {
    guard(0, || {
        // SAFETY: the caller guarantees a live handle or null.
        let Some(r) = (unsafe { remote.as_ref() }) else {
            return no_handle();
        };
        let Some(sources) = r.remote.v2ip_sources(uid.into()) else {
            not_reported(r, uid, "V2IP stream sources");
            return 0;
        };
        // SAFETY: the caller guarantees cap writable elements at out.
        unsafe { copy_into(&sources, out, cap) }
    })
}

/// Writes a device's network ports, and returns how many there are.
///
/// # Safety
///
/// `remote` is null or a live handle, and `out` is null or points at `cap`
/// writable [`mxr_network_port_t`].
#[no_mangle]
pub unsafe extern "C" fn mxr_network_status(
    remote: *const mxr_remote_t,
    uid: mxr_uid_t,
    out: *mut mxr_network_port_t,
    cap: usize,
) -> usize {
    guard(0, || {
        // SAFETY: the caller guarantees a live handle or null.
        let Some(r) = (unsafe { remote.as_ref() }) else {
            return no_handle();
        };
        let ports: Vec<mxr_network_port_t> = r
            .remote
            .network_status(uid.into())
            .iter()
            .map(port_of)
            .collect();
        // SAFETY: the caller guarantees cap writable elements at out.
        unsafe { copy_into(&ports, out, cap) }
    })
}

/// Copies one port report into the C shape.
fn port_of(p: &NetworkPortStatus) -> mxr_network_port_t {
    let errors = p.errors.unwrap_or_default();
    let mut out = mxr_network_port_t {
        port: p.port,
        name: [0; MXR_NAME_LEN],
        link_speed: p.link_speed.to_wire(),
        link_full_duplex: p.link_full_duplex,
        ip: [0; MXR_IP_STRING_LEN],
        querier: [0; MXR_IP_STRING_LEN],
        has_mac_address: p.mac_address.is_some(),
        mac_address: p.mac_address.unwrap_or_default().0,
        has_errors: p.errors.is_some(),
        in_error: errors.in_error,
        in_fcs_error: errors.in_fcs_error,
        in_collision: errors.in_collision,
        out_deferred: errors.out_deferred,
        out_excessive: errors.out_excessive,
        polarity_error: errors.polarity_error,
        skew_warning: errors.skew_warning,
        length_warning: errors.length_warning,
        has_vct_status: p.vct_status.is_some(),
        vct_warning: [false; MXR_UTP_PAIRS],
        cable_status_count: p.cable_status.len().min(MXR_UTP_PAIRS),
        cable_status: [mxr_cable_status_t {
            polarity: false,
            pair: 0,
            skew: 0,
            length: 0,
        }; MXR_UTP_PAIRS],
    };
    put_str(&mut out.name, &p.name);
    put_ip(&mut out.ip, p.ip);
    put_ip(&mut out.querier, p.querier);
    if let Some(vct) = p.vct_status {
        for (slot, status) in out.vct_warning.iter_mut().zip(vct) {
            *slot = status == VctStatus::Warning;
        }
    }
    for (slot, cable) in out.cable_status.iter_mut().zip(&p.cable_status) {
        *slot = (*cable).into();
    }
    out
}

/// Writes a device's view of the mesh topology, and returns how many entries
/// there are.
///
/// # Safety
///
/// `remote` is null or a live handle, and `out` is null or points at `cap`
/// writable [`mxr_topology_entry_t`].
#[no_mangle]
pub unsafe extern "C" fn mxr_topology(
    remote: *const mxr_remote_t,
    uid: mxr_uid_t,
    out: *mut mxr_topology_entry_t,
    cap: usize,
) -> usize {
    guard(0, || {
        // SAFETY: the caller guarantees a live handle or null.
        let Some(r) = (unsafe { remote.as_ref() }) else {
            return no_handle();
        };
        let topology = r.remote.topology(uid.into());
        // SAFETY: the caller guarantees cap writable elements at out.
        unsafe { copy_into(&topology, out, cap) }
    })
}

/// Writes the firmware versions a device reports, and returns how many there
/// are.
///
/// # Safety
///
/// `remote` is null or a live handle, and `out` is null or points at `cap`
/// writable [`mxr_firmware_version_t`].
#[no_mangle]
pub unsafe extern "C" fn mxr_device_firmware(
    remote: *const mxr_remote_t,
    uid: mxr_uid_t,
    out: *mut mxr_firmware_version_t,
    cap: usize,
) -> usize {
    guard(0, || {
        // SAFETY: the caller guarantees a live handle or null.
        let Some(r) = (unsafe { remote.as_ref() }) else {
            return no_handle();
        };
        let versions: Vec<mxr_firmware_version_t> = r
            .remote
            .firmware(uid.into())
            .iter()
            .map(|(_, v)| firmware_of(v))
            .collect();
        // SAFETY: the caller guarantees cap writable elements at out.
        unsafe { copy_into(&versions, out, cap) }
    })
}

/// Copies one firmware report into the C shape.
fn firmware_of(v: &FirmwareVersion) -> mxr_firmware_version_t {
    let mut out = mxr_firmware_version_t {
        firmware_type: v.firmware_type.to_wire(),
        timestamp: v.timestamp,
        hash: v.hash,
        version: [0; MXR_VERSION_LEN],
    };
    put_str(&mut out.version, &v.version);
    out
}

/// Writes a device's audio endpoints, in the order it reported them, and
/// returns how many there are.
///
/// # Safety
///
/// `remote` is null or a live handle, and `out` is null or points at `cap`
/// writable [`mxr_audio_endpoint_t`].
#[no_mangle]
pub unsafe extern "C" fn mxr_audio_endpoints(
    remote: *const mxr_remote_t,
    uid: mxr_uid_t,
    out: *mut mxr_audio_endpoint_t,
    cap: usize,
) -> usize {
    guard(0, || {
        // SAFETY: the caller guarantees a live handle or null.
        let Some(r) = (unsafe { remote.as_ref() }) else {
            return no_handle();
        };
        let Some(endpoints) = r.remote.audio_endpoints(uid.into()) else {
            not_reported(r, uid, "audio endpoints");
            return 0;
        };
        let list: Vec<mxr_audio_endpoint_t> = endpoints.list().map(Into::into).collect();
        // SAFETY: the caller guarantees cap writable elements at out.
        unsafe { copy_into(&list, out, cap) }
    })
}

/// Writes the endpoints hanging off one audio endpoint, and returns how many
/// there are.
///
/// # Safety
///
/// `remote` is null or a live handle, and `out` is null or points at `cap`
/// writable bytes.
#[no_mangle]
pub unsafe extern "C" fn mxr_audio_endpoint_children(
    remote: *const mxr_remote_t,
    uid: mxr_uid_t,
    endpoint: u8,
    out: *mut u8,
    cap: usize,
) -> usize {
    guard(0, || {
        // SAFETY: the caller guarantees a live handle or null.
        let Some(r) = (unsafe { remote.as_ref() }) else {
            return no_handle();
        };
        let children = match r.remote.audio_endpoints(uid.into()) {
            Some(endpoints) => match endpoints.get(endpoint) {
                Some(e) => e.children.clone(),
                None => {
                    fail(
                        mxr_result_t::MXR_ERR_NOT_FOUND,
                        &format!("the device has no audio endpoint {endpoint}"),
                    );
                    return 0;
                }
            },
            None => {
                not_reported(r, uid, "audio endpoints");
                return 0;
            }
        };
        // SAFETY: the caller guarantees cap writable bytes at out.
        unsafe { copy_into(&children, out, cap) }
    })
}

/// Reports a null handle from a call whose answer is a count.
fn no_handle() -> usize {
    fail(
        mxr_result_t::MXR_ERR_INVALID_ARGUMENT,
        "the client handle is null",
    );
    0
}
