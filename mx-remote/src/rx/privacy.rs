// Author: Lars Op den Kamp (lars@opdenkamp-it.nl)
// Copyright (c) 2026 Op den Kamp IT Solutions

//! How a V2IP source unit's streams are encrypted.

use std::net::Ipv4Addr;

use crate::event::Event;
use crate::state::State;
use crate::types::{V2ipPrivacy, V2ipPrivacyStream};
use crate::wire::UID_LEN;

use super::handlers::{u16_at, u32_at};
use super::Rx;

/// Width of the header every frame opens with: a uid, the type, the count of
/// streams that follow and two reserved bytes.
const PRIVACY_HEADER_SIZE: usize = UID_LEN + 4;

/// The type of a frame a source unit reports its streams in.
const PRIVACY_REPORT: u8 = 0;

/// The streams a report carries: video, audio and ancillary data.
const PRIVACY_STREAMS: usize = 3;

/// Width of one stream: the group, the port, the version and a reserved byte,
/// then the parameters as the unit's video processor reports them.
const PRIVACY_STREAM_SIZE: usize = 48;

/// Keeps the parameters a source unit reports, for the unit that sent them.
///
/// The header's uid plays no part, as units file a report by its sender too.
/// A request is for a unit with an encoder to answer, which this client is
/// not.
pub(super) fn privacy(state: &mut State, rx: &Rx<'_>, ev: &mut Vec<Event>) {
    let p = rx.frame.payload();
    let len = PRIVACY_HEADER_SIZE + PRIVACY_STREAMS * PRIVACY_STREAM_SIZE;
    if p.len() < len
        || p[UID_LEN] != PRIVACY_REPORT
        || usize::from(p[UID_LEN + 1]) < PRIVACY_STREAMS
    {
        return;
    }
    let at = |i: usize| stream(&p[PRIVACY_HEADER_SIZE + i * PRIVACY_STREAM_SIZE..]);
    let privacy = V2ipPrivacy {
        video: at(0),
        audio: at(1),
        anc: at(2),
    };
    if let Some(device) = state.device_mut(rx.sender()) {
        device.set_v2ip_privacy(privacy, ev);
    }
}

/// One stream: the group at 0, the port at 4, the version at 6, then whether
/// it is encrypted at 8, the two header extension ids at 9 and 10, the IV at
/// 11, the key generator at 19, the key id at 35 and the key version at 43,
/// unaligned. `s` holds at least [`PRIVACY_STREAM_SIZE`] bytes.
fn stream(s: &[u8]) -> V2ipPrivacyStream {
    V2ipPrivacyStream {
        group: Ipv4Addr::new(s[0], s[1], s[2], s[3]),
        port: u16_at(s, 4),
        version: s[6],
        on: s[8] != 0,
        full_id: s[9],
        short_id: s[10],
        iv: octets(&s[11..]),
        key_generator: octets(&s[19..]),
        key_id: octets(&s[35..]),
        key_version: u32_at(s, 43),
    }
}

/// The first `N` bytes of `s`, which holds at least that many.
fn octets<const N: usize>(s: &[u8]) -> [u8; N] {
    let mut out = [0; N];
    out.copy_from_slice(&s[..N]);
    out
}
