// Author: Lars Op den Kamp (lars@opdenkamp-it.nl)
// Copyright (c) 2026 Op den Kamp IT Solutions

//! Payload builders.
//!
//! Each function lays out one command payload from its firmware declaration.
//! They produce bytes only - a payload reaches the wire through the single
//! transmit path, which is what applies the per-opcode protocol floor.

use std::net::Ipv4Addr;

use crate::types::{
    AmpZoneSettings, V2ipAudioFormat, VideoWallOp, VideoWallWindow, VolumeMuteStatus,
    VIDEO_WALL_CLEARED,
};

use super::enums::{EdidProfile, MxrSignalType, RcAction, RcKey};
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
///
/// The two carry opposite byte orders. An address is held in network order, so
/// its octets go out as they are written down, while the port beside it is
/// little-endian like every other scalar here. Writing the address as a `u32`
/// reverses it, and stays invisible for as long as the addresses under test
/// read the same backwards.
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
///
/// **Never append a field here.** Receivers check this payload's length
/// exactly, alone among the opcodes, so a longer hello is discarded rather
/// than read up to the part the receiver knows. What that costs is not a
/// feature: a hello is how this client stops being unknown, so a device that
/// drops it never registers the sender and ignores everything sent afterwards
/// as coming from a stranger. Announce anything new on an opcode that tolerates
/// growth.
///
/// This holds however many receivers are later fixed to accept a longer hello:
/// the ones already deployed are the ones that decide, and a client that cannot
/// be seen by them is worth less than any field it might have announced.
///
/// The asymmetry is deliberate and only on this side: [`super::Frame`] reads a
/// received hello field by field with no length gate at all, so a peer that
/// grows its own hello stays readable here.
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
    let mut p = Vec::with_capacity(40);
    p.extend_from_slice(target.as_bytes());
    p.extend_from_slice(&port.to_le_bytes());
    append_fixed_str(&mut p, name, 16);
    // mxr_bay_name_data is ALIGN(8), rounding 34 bytes up to 40.
    p.extend_from_slice(&[0; 6]);
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

/// Builds the `RC_TX_KEY` (0x0C) payload for one bay.
///
/// The device routes the key onward over CEC, infrared or IP, whichever that
/// bay is configured for.
pub(crate) fn build_rc_key(target: DeviceUid, port: u16, key: RcKey) -> Vec<u8> {
    let mut p = Vec::with_capacity(20);
    p.extend_from_slice(target.as_bytes());
    p.extend_from_slice(&port.to_le_bytes());
    p.extend_from_slice(&key.to_wire().to_le_bytes());
    p
}

/// Builds a payload naming a target device and one flag byte.
fn build_target_and_flag(target: DeviceUid, flag: bool) -> Vec<u8> {
    let mut p = Vec::with_capacity(17);
    p.extend_from_slice(target.as_bytes());
    p.push(u8::from(flag));
    p
}

/// Builds the `V2IP_VIDEO_WALL` (0x49) payload.
///
/// 32 bytes, not the 29 its fields add up to. The struct is 4-aligned, so the
/// op byte at 28 is followed by three bytes of padding, and the receiver's
/// length check is against the whole struct: a payload built by summing field
/// widths is three bytes short and dropped without a word.
///
/// A revert carries no window, so the geometry is zeroed rather than filled
/// in: the receiver ignores it, and sending a window there would leave a
/// reader of the frame unable to tell a revert from a placement.
pub(crate) fn build_video_wall(
    target: DeviceUid,
    window: VideoWallWindow,
    op: VideoWallOp,
) -> Vec<u8> {
    let mut p = Vec::with_capacity(32);
    p.extend_from_slice(target.as_bytes());
    let geometry = if op == VideoWallOp::REVERT {
        VIDEO_WALL_CLEARED
    } else {
        window
    };
    for value in [
        geometry.pos_x,
        geometry.pos_y,
        geometry.width,
        geometry.height,
        geometry.raster_w,
        geometry.raster_h,
    ] {
        p.extend_from_slice(&value.to_le_bytes());
    }
    p.push(op.to_wire());
    p.extend_from_slice(&[0, 0, 0]);
    p
}

/// Builds the `V2IP_STATS` (0x3F) request payload.
pub(crate) fn build_stats_request(target: DeviceUid, subscribe: bool) -> Vec<u8> {
    build_target_and_flag(target, subscribe)
}

/// Builds the `DEV_EDID` (0x07) request payload.
///
/// `output` asks for the EDID of the display on the device's output rather
/// than the one the device presents on its input.
pub(crate) fn build_edid_request(target: DeviceUid, output: bool) -> Vec<u8> {
    build_target_and_flag(target, output)
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
/// The body names the route end to end - its first uid is the sink and its
/// second is the source - while the header names the device being addressed for
/// this hop. A controller sending a single-hop command puts the sink in both,
/// as this does, but they are separate fields and a receiver resolves the body
/// on its own: do not read one from the other.
///
/// The receiving struct calls its first field `source` and its second `target`,
/// which is the reverse of what they carry. Nothing on the wire distinguishes a
/// client that swapped them from one that did not, and the naming invites the
/// swap, so the orientation here rests on how the module reads the fields
/// rather than on what they are called.
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

/// The `tx_rate` a frame that is not setting a rate carries.
///
/// The field's valid range ends well below this, and a receiver drops an
/// out-of-range rate and keeps the one it had. Sending a plain zero would ask
/// for a rate of zero.
const V2IP_RATE_UNSET: u8 = 0xFF;

/// Builds the `V2IP_DEVICE_CFG` (0x3C) payload that writes one sink's scaling
/// block, leaving every other field of the configuration alone.
///
/// 88 bytes, which is both the receiver's minimum and the whole of
/// `v2ip_device_config_update`: uid 0..16, source 16..40, the options word at
/// 40, audio return 48..56, scaling 56..64, tiling 64..88.
///
/// **88 rather than the 120-byte form.** The longer form appends a sink block,
/// and a receiver copies that block into its record for the target with no
/// validity test of its own - unlike the source, rate, marking, scaling and
/// tiling fields, which each sit behind one. This frame is a broadcast, so
/// every device on the network runs that copy, not just the addressee: sending
/// the long form with the block zeroed would replace the whole network's idea
/// of where the target's sink is subscribed, as a side effect of setting one
/// scaling flag. At 88 the block is absent rather than zeroed and nothing
/// reads it.
///
/// **The source block at 16..40 must stay zeroed**, and does. A receiver hands
/// this frame's addresses to its encoder unconditionally, on every frame it
/// applies rather than only on the ones that carry addresses; what stops a
/// scaling write from repointing the encoder is that the call refuses a video
/// address which is not multicast. Zero is not multicast. Anything that is,
/// written here, would move a transceiver's stream.
///
/// Which halves of the block a receiver reads is chosen by the validity bits in
/// `flags`, not by this layout: the mode and refresh are read behind one bit
/// and the options behind another, so a write that carries neither bit lands as
/// a no-op rather than as a request to zero the settings.
pub(crate) fn build_v2ip_scaling(
    target: DeviceUid,
    mode: MxrSignalType,
    refresh: u16,
    flags: u8,
) -> Vec<u8> {
    let mut p = Vec::with_capacity(88);
    p.extend_from_slice(target.as_bytes());
    // source: three stream slots, left zeroed so the encoder keeps its own.
    p.resize(40, 0);
    p.push(V2IP_RATE_UNSET);
    // Three dscp bytes with no MXR_V2IP_DSCP_SET bit, so no marking is applied,
    // then the padding that aligns the audio-return slot.
    p.resize(48, 0);
    // audio return: zeroed, which reads as carrying no address.
    p.resize(56, 0);
    p.extend_from_slice(&mode.to_wire().to_le_bytes());
    p.extend_from_slice(&refresh.to_le_bytes());
    p.push(flags);
    // The scaling struct is 8-aligned, so its five bytes of fields are followed
    // by three of padding; then the tiling window, whose zero uid is what says
    // no window is carried.
    p.resize(88, 0);
    p
}
