// Author: Lars Op den Kamp (lars@opdenkamp-it.nl)
// Copyright (c) 2026 Op den Kamp IT Solutions

//! What a V2IP output plays from a session description.

use std::net::Ipv4Addr;

use crate::event::Event;
use crate::state::State;
use crate::types::V2ipSinkSdp;
use crate::wire::{
    cstr, V2ipSdpClockMode, V2ipSdpClockState, V2ipSdpEncoding, V2ipSdpFlag, V2ipSdpMediaClock,
    V2ipSdpRefClock, UID_LEN,
};

use super::handlers::{u16_at, u32_at};
use super::Rx;

/// Width of the header every frame opens with: the output's uid, the type,
/// a value and two reserved bytes.
const SDP_HEADER_SIZE: usize = UID_LEN + 4;

/// The type of a frame an output reports its status in.
const SDP_STATUS: u8 = 0;

/// Width of the status behind the header: ten single bytes, the port, rate,
/// clock offset and group, a 48-byte name and an 8-byte layout.
const SDP_STATUS_SIZE: usize = 80;

/// Applies the status an output reports, to the output that sent it.
///
/// The other types are commands for an output, which a peer sends and only
/// that output acts on. The status grows at the back, so a frame is read as
/// far as it goes and the fields it does not reach read as zero.
pub(super) fn sink_sdp(state: &mut State, rx: &Rx<'_>, ev: &mut Vec<Event>) {
    let p = rx.frame.payload();
    if p.len() < SDP_HEADER_SIZE || p[UID_LEN] != SDP_STATUS {
        return;
    }
    let mut s = p[SDP_HEADER_SIZE..].to_vec();
    s.resize(s.len().max(SDP_STATUS_SIZE), 0);
    let status = V2ipSinkSdp {
        flags: V2ipSdpFlag::from_bits(s[0]),
        screen: s[1],
        encoding: V2ipSdpEncoding::from_wire(s[2]),
        channels: s[3],
        payload_type: s[4],
        media_clock: V2ipSdpMediaClock::from_wire(s[5]),
        ref_clock: V2ipSdpRefClock::from_wire(s[6]),
        clock_mode: V2ipSdpClockMode::from_wire(s[7]),
        clock_state: V2ipSdpClockState::from_wire(s[8]),
        port: u16_at(&s, 10),
        rate_hz: u32_at(&s, 12),
        media_clock_offset: u32_at(&s, 16),
        group: Ipv4Addr::new(s[20], s[21], s[22], s[23]),
        // The output terminates both strings in their last byte, so a name
        // that fills its field stops one byte short of it.
        name: cstr(&s[24..71]),
        layout: cstr(&s[72..79]),
    };
    if let Some(device) = state.device_mut(rx.sender()) {
        device.set_v2ip_sink_sdp(status, ev);
    }
}
