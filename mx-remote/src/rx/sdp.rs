// Author: Lars Op den Kamp (lars@opdenkamp-it.nl)
// Copyright (c) 2026 Op den Kamp IT Solutions

//! What a OneIP output plays from a session description.

use std::net::Ipv4Addr;
use std::time::Duration;

use crate::event::Event;
use crate::state::{SdpPages, State};
use crate::types::{V2ipSinkSdp, V2IP_SDP_MAX};
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

/// The type of a frame an output reports the description it plays in, in
/// pages.
const SDP_DESCRIPTION: u8 = 5;

/// Width of the page header behind the frame header: the description's
/// length, the page's offset into it and the page's length, all u16, and two
/// reserved bytes.
const SDP_PAGE_HEADER_SIZE: usize = 8;

/// How long an output may take between two pages of one description before
/// the pages it sent are dropped.
const SDP_PAGE_TIMEOUT: Duration = Duration::from_secs(5);

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
    if p.len() < SDP_HEADER_SIZE {
        return;
    }
    if p[UID_LEN] == SDP_DESCRIPTION {
        description_page(state, rx, ev);
        return;
    }
    if p[UID_LEN] != SDP_STATUS {
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
        // A description is reported when a stream starts, so one that has
        // stopped leaves none behind it.
        if !status.is_active() {
            device.v2ip_sink_description_pages = None;
            device.set_v2ip_sink_description(None, ev);
        }
        device.set_v2ip_sink_sdp(status, ev);
    }
}

/// Puts together the description an output reports playing, one page at a
/// time, as a unit puts together the pages it is sent.
///
/// A page at offset 0 starts a description of 1 to [`V2IP_SDP_MAX`] bytes.
/// Every later page has to continue it: at the offset received so far, for
/// the same length, within [`SDP_PAGE_TIMEOUT`] of the page before. Anything
/// else drops the pages received, and the description is reported once its
/// last byte is in. Only the output itself reports one, so a frame whose
/// header names another device is dropped.
fn description_page(state: &mut State, rx: &Rx<'_>, ev: &mut Vec<Event>) {
    let p = rx.frame.payload();
    if rx.frame.uid(0) != Some(rx.sender()) {
        return;
    }
    let Some(page) = p.get(SDP_HEADER_SIZE..SDP_HEADER_SIZE + SDP_PAGE_HEADER_SIZE) else {
        return;
    };
    let (total, offset, len) = (u16_at(page, 0), u16_at(page, 2), u16_at(page, 4));
    let start = SDP_HEADER_SIZE + SDP_PAGE_HEADER_SIZE;
    let Some(text) = p.get(start..start + usize::from(len)) else {
        return;
    };
    let Some(device) = state.device_mut(rx.sender()) else {
        return;
    };
    let mut pages = match device.v2ip_sink_description_pages.take() {
        _ if offset == 0 => {
            if total == 0 || usize::from(total) > V2IP_SDP_MAX {
                return;
            }
            SdpPages {
                total,
                data: Vec::with_capacity(usize::from(total)),
                at: rx.timestamp,
            }
        }
        Some(pages)
            if pages.total == total
                && pages.data.len() == usize::from(offset)
                && rx.timestamp.saturating_duration_since(pages.at) <= SDP_PAGE_TIMEOUT =>
        {
            pages
        }
        _ => return,
    };
    if pages.data.len() + text.len() > usize::from(total) {
        return;
    }
    pages.data.extend_from_slice(text);
    pages.at = rx.timestamp;
    if pages.data.len() < usize::from(total) {
        device.v2ip_sink_description_pages = Some(pages);
        return;
    }
    let description = String::from_utf8_lossy(&pages.data).into_owned();
    device.set_v2ip_sink_description(Some(description), ev);
}
