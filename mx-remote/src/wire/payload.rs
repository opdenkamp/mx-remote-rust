// Author: Lars Op den Kamp (lars@opdenkamp-it.nl)
// Copyright (c) 2026 Op den Kamp IT Solutions

//! Payload builders.
//!
//! Each function lays out one command payload from its firmware declaration.
//! They produce bytes only - a payload reaches the wire through the single
//! transmit path, which is what applies the per-opcode protocol floor.

use std::net::Ipv4Addr;

use crate::types::{AmpZoneSettings, V2ipAudioFormat, VolumeMuteStatus};

use super::enums::{EdidProfile, RcAction};
use super::opcode::audio_sub;
use super::uid::DeviceUid;

/// Appends `value` as ASCII, truncated to `size` and NUL-padded to `size`.
pub(crate) fn append_fixed_str(dst: &mut Vec<u8>, value: &str, size: usize) {
    let taken = value.as_bytes().get(..size).unwrap_or(value.as_bytes());
    dst.extend_from_slice(taken);
    dst.resize(dst.len() + (size - taken.len()), 0);
}

/// One multicast stream destination.
///
/// 0.0.0.0 is how a caller says "no stream here", and firmware leaves that
/// stream where it is rather than tearing it down.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct StreamAddr {
    /// The group address.
    pub(crate) ip: Ipv4Addr,
    /// Destination UDP port.
    pub(crate) port: u16,
}

impl Default for StreamAddr {
    fn default() -> Self {
        Self {
            ip: Ipv4Addr::UNSPECIFIED,
            port: 0,
        }
    }
}

/// The video, audio and ancillary streams a V2IP sink is told to subscribe to.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct V2ipStreams {
    /// Video stream destination.
    pub(crate) video: StreamAddr,
    /// Audio stream destination.
    pub(crate) audio: StreamAddr,
    /// Ancillary-data stream destination.
    pub(crate) anc: StreamAddr,
}

/// Appends a `v2ip_stream_addr`: an IPv4 address, a `u16` port and two pad bytes.
pub(crate) fn append_stream_addr(dst: &mut Vec<u8>, addr: StreamAddr) {
    dst.extend_from_slice(&addr.ip.octets());
    dst.extend_from_slice(&addr.port.to_le_bytes());
    dst.extend_from_slice(&[0, 0]);
}

/// Builds the `V2IP_MANUAL_SRC_SWITCH` (0x24) payload.
///
/// The three stream addresses are named rather than passed as six positional
/// values, so a transposed pair is a compile error rather than a frame that
/// sends audio to the video group.
pub(crate) fn build_v2ip_manual_source_switch(
    target: DeviceUid,
    streams: V2ipStreams,
    format: Option<V2ipAudioFormat>,
) -> Vec<u8> {
    let mut p = Vec::with_capacity(48);
    p.extend_from_slice(target.as_bytes());
    append_stream_addr(&mut p, streams.video);
    append_stream_addr(&mut p, streams.audio);
    append_stream_addr(&mut p, streams.anc);
    if let Some(f) = format {
        p.extend_from_slice(&f.wire());
    }
    p
}

/// Builds the `AUDIO_SET_VOLUME` (0x14) payload for one bay.
pub(crate) fn build_set_volume(target: DeviceUid, port: u16, volume: VolumeMuteStatus) -> Vec<u8> {
    let mut p = Vec::with_capacity(24);
    p.extend_from_slice(target.as_bytes());
    p.extend_from_slice(&port.to_le_bytes());
    p.extend_from_slice(&volume.wire());
    // mxr_set_volume_request is ALIGN(8): three bytes of trailing padding.
    p.extend_from_slice(&[0, 0, 0]);
    p
}

/// Builds the `V2IP_AUDIO` (0x43) command header: a `u16` sub-opcode, two pad
/// bytes and the target uid.
pub(crate) fn audio_cmd_header(sub_opcode: u16, target: DeviceUid) -> Vec<u8> {
    let mut h = Vec::with_capacity(20);
    h.extend_from_slice(&sub_opcode.to_le_bytes());
    h.extend_from_slice(&[0, 0]);
    h.extend_from_slice(target.as_bytes());
    h
}

/// Builds the endpoint/value pair a `V2IP_AUDIO` command carries after its header.
pub(crate) fn audio_param(endpoint_id: u16, param: u32) -> [u8; 8] {
    let e = endpoint_id.to_le_bytes();
    let v = param.to_le_bytes();
    [e[0], e[1], 0, 0, v[0], v[1], v[2], v[3]]
}

/// Builds the `V2IP_MULTIVIEWER` (0x42) payload: the target uid, the sub-opcode
/// at 16, seven pad bytes, and the command's own parameters from 24.
pub(crate) fn mv_cmd_payload(target: DeviceUid, sub_opcode: u8, args: &[u8]) -> Vec<u8> {
    let mut p = Vec::with_capacity(24 + args.len());
    p.extend_from_slice(target.as_bytes());
    p.push(sub_opcode);
    p.extend_from_slice(&[0; 7]);
    p.extend_from_slice(args);
    p
}

/// Builds the `AMP_ZONE_SETTINGS` (0x3D) payload for the given target bay.
pub(crate) fn build_amp_zone_settings(
    target: DeviceUid,
    zone: u16,
    s: &AmpZoneSettings,
) -> Vec<u8> {
    let mut p = Vec::with_capacity(56);
    p.extend_from_slice(target.as_bytes());
    p.extend_from_slice(&zone.to_le_bytes());
    p.extend_from_slice(&[s.gain_left, s.gain_right, s.volume_min, s.volume_max]);
    // The u32 delays align to 4, so the padding sits ahead of them.
    p.extend_from_slice(&[0, 0]);
    p.extend_from_slice(&s.delay_left.to_le_bytes());
    p.extend_from_slice(&s.delay_right.to_le_bytes());
    p.extend_from_slice(&[s.bass, s.treble, s.bridged, s.power_mode, s.power_level]);
    // power_auto_time aligns to 4 in the same way.
    p.extend_from_slice(&[0, 0, 0]);
    p.extend_from_slice(&s.power_timeout.to_le_bytes());
    p.extend_from_slice(&s.eq_left);
    p.extend_from_slice(&s.eq_right);
    // mxr_amp_zone_settings is ALIGN(8), rounding 54 bytes up to 56.
    p.extend_from_slice(&[0, 0]);
    p
}

/// Builds the `SYS_HELLO` (0x00) payload announcing this client.
pub(crate) fn build_hello(
    protocol: u16,
    app_name: &str,
    serial: &str,
    version: &str,
    features: u32,
) -> Vec<u8> {
    let mut p = Vec::with_capacity(54);
    p.extend_from_slice(&protocol.to_le_bytes());
    append_fixed_str(&mut p, app_name, 16);
    append_fixed_str(&mut p, serial, 16);
    append_fixed_str(&mut p, version, 16);
    p.extend_from_slice(&features.to_le_bytes());
    p
}

/// Builds the `CHANGE_BAY_NAME` (0x22) payload.
pub(crate) fn build_set_bay_name(target: DeviceUid, port: u16, name: &str) -> Vec<u8> {
    let mut p = Vec::with_capacity(34);
    p.extend_from_slice(target.as_bytes());
    p.extend_from_slice(&port.to_le_bytes());
    append_fixed_str(&mut p, name, 16);
    p
}

/// Builds a payload naming only a target device, used by the commands whose
/// whole request is "you".
pub(crate) fn build_target_only(target: DeviceUid) -> Vec<u8> {
    target.as_bytes().to_vec()
}

/// Builds the `BAY_EDID_PROFILE` (0x34) payload.
pub(crate) fn build_edid_profile(target: DeviceUid, profile: EdidProfile) -> Vec<u8> {
    let mut p = Vec::with_capacity(24);
    p.extend_from_slice(target.as_bytes());
    p.extend_from_slice(&profile.to_wire().to_le_bytes());
    // mxr_edid_profile_request is ALIGN(8), rounding 18 bytes up to 24.
    p.extend_from_slice(&[0; 6]);
    p
}

/// Builds the `RC_TX_ACTION` (0x0E) payload.
pub(crate) fn build_rc_action(target: DeviceUid, port: u16, action: RcAction) -> Vec<u8> {
    let mut p = Vec::with_capacity(20);
    p.extend_from_slice(target.as_bytes());
    p.extend_from_slice(&port.to_le_bytes());
    p.extend_from_slice(&action.to_wire().to_le_bytes());
    p
}

/// Builds the `V2IP_STATS` (0x3F) request payload.
pub(crate) fn build_stats_request(target: DeviceUid, subscribe: bool) -> Vec<u8> {
    let mut p = Vec::with_capacity(17);
    p.extend_from_slice(target.as_bytes());
    p.push(u8::from(subscribe));
    p
}

/// Builds the `V2IP_SOURCE_SWITCH` (0x1F) payload: the sink uid, then the
/// video and audio group addresses to subscribe to.
///
/// 0.0.0.0 leaves that stream where it is, so routing video and routing audio
/// are separate calls that each name only their own.
pub(crate) fn build_v2ip_source_switch(
    sink: DeviceUid,
    video_ip: Ipv4Addr,
    audio_ip: Ipv4Addr,
) -> Vec<u8> {
    let mut p = Vec::with_capacity(24);
    p.extend_from_slice(sink.as_bytes());
    p.extend_from_slice(&video_ip.octets());
    p.extend_from_slice(&audio_ip.octets());
    p
}

/// Builds the `BAY_HIDE` (0x27) payload.
pub(crate) fn build_bay_hide(target: DeviceUid, port: u16, hidden: bool) -> Vec<u8> {
    let mut p = Vec::with_capacity(24);
    p.extend_from_slice(target.as_bytes());
    p.extend_from_slice(&port.to_le_bytes());
    p.push(u8::from(hidden));
    // mxr_bay_hide_request is ALIGN(8), rounding 19 bytes up to 24.
    p.extend_from_slice(&[0; 5]);
    p
}

/// Builds the `V2IP_AUDIO` (0x43) `SELECT_INPUT` body.
///
/// The sink is named twice - once as the command header's target and again at
/// the head of the body - so a decoder reading the body's second uid as the
/// sink would make one device both source and target of a single frame.
pub(crate) fn build_audio_select_input(
    sink: DeviceUid,
    sink_endpoint: u16,
    source: DeviceUid,
    source_endpoint: u16,
) -> Vec<u8> {
    let mut p = audio_cmd_header(audio_sub::SELECT_INPUT, sink);
    p.extend_from_slice(sink.as_bytes());
    p.extend_from_slice(source.as_bytes());
    p.extend_from_slice(&sink_endpoint.to_le_bytes());
    p.extend_from_slice(&source_endpoint.to_le_bytes());
    p
}
