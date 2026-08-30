// Author: Lars Op den Kamp (lars@opdenkamp-it.nl)
// Copyright (c) 2026 Op den Kamp IT Solutions

//! Fixtures shared by the test modules across the crate.
//!
//! A fixture assembles what a device sends, byte by byte, rather than calling
//! this library's own builders: a decoder and a builder that agree with each
//! other while both disagreeing with the wire would look correct to any test
//! built the other way.

use std::net::Ipv4Addr;

use crate::wire::{
    BayFeatures, BayStatus, DeviceFeature, DeviceUid, Opcode, BAY_CONFIG_SIZE, V2IP_PORT_ANC,
    V2IP_PORT_AUDIO, V2IP_PORT_VIDEO,
};

/// Returns `n` bytes pre-filled with a non-zero, position-varying pattern, for
/// callers that then write the real fields over it.
///
/// A zero-filled fixture cannot catch a field read at the right offset but the
/// wrong width: the padding beside it is zero, so a widened read returns the
/// same value. Poisoning the payload first makes any read that strays past a
/// field's real width produce a wrong answer instead of the right one. That is
/// the class a sweep over offsets structurally misses, because the offset is
/// correct.
pub(crate) fn poisoned(n: usize) -> Vec<u8> {
    (0..n).map(|i| 0xA5 ^ (i as u8)).collect()
}

/// A device uid distinct per `n`, with a fixed tail so a truncated read is
/// visible.
pub(crate) fn uid_n(n: u8) -> DeviceUid {
    let mut bytes = [0u8; 16];
    bytes[0] = n;
    bytes[15] = 0xAA;
    DeviceUid::from_array(bytes)
}

/// Writes `value` into the `len`-wide field at `at`, NUL-padded the way a
/// device sends one.
///
/// The padding matters because these fixtures are poisoned: a field left
/// half-written would read on into the poison, which is the point - a device
/// terminates its strings, so a fixture that does not is not a fixture of
/// anything a device sends.
pub(crate) fn field(dst: &mut [u8], at: usize, len: usize, value: &str) {
    let bytes = value.as_bytes();
    let taken = bytes.get(..len).unwrap_or(bytes);
    dst[at..at + taken.len()].copy_from_slice(taken);
    dst[at + taken.len()..at + len].fill(0);
}

/// Writes `value` as ASCII, truncated to `size` and NUL-padded to `size`.
pub(crate) fn fixed_str(dst: &mut Vec<u8>, value: &str, size: usize) {
    let bytes = value.as_bytes();
    let taken = bytes.get(..size).unwrap_or(bytes);
    dst.extend_from_slice(taken);
    dst.resize(dst.len() + (size - taken.len()), 0);
}

/// Builds a `SYS_HELLO` payload.
pub(crate) fn hello_payload(
    protocol: u16,
    name: &str,
    serial: &str,
    version: &str,
    features: DeviceFeature,
) -> Vec<u8> {
    let mut p = Vec::with_capacity(54);
    p.extend_from_slice(&protocol.to_le_bytes());
    fixed_str(&mut p, name, 16);
    fixed_str(&mut p, serial, 16);
    fixed_str(&mut p, version, 16);
    p.extend_from_slice(&features.bits().to_le_bytes());
    p
}

/// Builds one bay descriptor.
pub(crate) fn bay_config_rec(
    port: u8,
    mode: u8,
    bay: u8,
    name: &str,
    user: &str,
    status: BayStatus,
    features: BayFeatures,
) -> Vec<u8> {
    let mut p = poisoned(BAY_CONFIG_SIZE);
    p[0] = port;
    p[1] = mode;
    p[2] = bay;
    field(&mut p, 5, 16, name);
    field(&mut p, 21, 16, user);
    p[53..57].copy_from_slice(&status.bits().to_le_bytes());
    p[57..61].copy_from_slice(&features.bits().to_le_bytes());
    p
}

/// Builds one record of a V2IP sources frame.
pub(crate) fn stream_rec(
    uid: DeviceUid,
    video: &str,
    audio: &str,
    anc: &str,
    port: u16,
) -> Vec<u8> {
    let mut rec = poisoned(40);
    rec[0..16].copy_from_slice(uid.as_bytes());
    for (off, ip) in [(16, video), (24, audio), (32, anc)] {
        let addr: Ipv4Addr = ip.parse().unwrap_or(Ipv4Addr::UNSPECIFIED);
        rec[off..off + 4].copy_from_slice(&addr.octets());
        rec[off + 4..off + 6].copy_from_slice(&port.to_le_bytes());
    }
    rec
}

/// A `v2ip_device_config_update` payload builder.
///
/// Every field goes in verbatim, so a caller can send the zeroed blocks and
/// out-of-range rate that a controller writing one field does.
#[derive(Default)]
pub(crate) struct Cfg {
    pub(crate) uid: DeviceUid,
    pub(crate) video_ip: &'static str,
    pub(crate) audio_ip: &'static str,
    pub(crate) anc_ip: &'static str,
    pub(crate) arc_ip: &'static str,
    pub(crate) video_port: u16,
    pub(crate) audio_port: u16,
    pub(crate) anc_port: u16,
    pub(crate) arc_port: u16,
    pub(crate) rate: u8,
    pub(crate) dscp_video: u8,
    pub(crate) dscp_audio: u8,
    pub(crate) dscp_anc: u8,
    pub(crate) mode: u16,
    pub(crate) refresh: u16,
    pub(crate) flags: u8,
}

impl Cfg {
    /// A source block that passes the firmware's own validity check.
    pub(crate) fn addresses(uid: DeviceUid, base: &'static str) -> Self {
        Self {
            uid,
            video_ip: base,
            video_port: V2IP_PORT_VIDEO,
            audio_ip: base,
            audio_port: V2IP_PORT_AUDIO,
            anc_ip: base,
            anc_port: V2IP_PORT_ANC,
            ..Self::default()
        }
    }

    pub(crate) fn bytes(&self) -> Vec<u8> {
        let mut p = poisoned(88);
        p[0..16].copy_from_slice(self.uid.as_bytes());
        let mut put = |off: usize, ip: &str, port: u16| {
            if ip.is_empty() {
                return;
            }
            let addr: Ipv4Addr = ip.parse().unwrap_or(Ipv4Addr::UNSPECIFIED);
            p[off..off + 4].copy_from_slice(&addr.octets());
            p[off + 4..off + 6].copy_from_slice(&port.to_le_bytes());
        };
        put(16, self.video_ip, self.video_port);
        put(24, self.audio_ip, self.audio_port);
        put(32, self.anc_ip, self.anc_port);
        put(48, self.arc_ip, self.arc_port);
        p[40] = self.rate;
        p[41] = self.dscp_video;
        p[42] = self.dscp_audio;
        p[43] = self.dscp_anc;
        p[56..58].copy_from_slice(&self.mode.to_le_bytes());
        p[58..60].copy_from_slice(&self.refresh.to_le_bytes());
        p[60] = self.flags;
        p
    }
}

/// Assembles a datagram the way a device does.
///
/// The header is written out here rather than taken from this library's own
/// frame builder, so that a decoder and a builder cannot agree with each other
/// while both disagreeing with the wire.
pub(crate) fn datagram(sender: DeviceUid, op: Opcode, protocol: u16, payload: &[u8]) -> Vec<u8> {
    let mut data = Vec::with_capacity(24 + payload.len());
    data.extend_from_slice(b"P8");
    data.extend_from_slice(&protocol.to_le_bytes());
    data.extend_from_slice(sender.as_bytes());
    data.extend_from_slice(&op.0.to_le_bytes());
    let declared = u16::try_from(payload.len()).unwrap_or(u16::MAX);
    data.extend_from_slice(&declared.to_le_bytes());
    data.extend_from_slice(payload);
    data
}
