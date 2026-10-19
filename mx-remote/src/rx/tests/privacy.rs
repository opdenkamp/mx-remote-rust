// Author: Lars Op den Kamp (lars@opdenkamp-it.nl)
// Copyright (c) 2026 Op den Kamp IT Solutions

//! How a source unit's streams are encrypted (0x54).

use std::net::Ipv4Addr;

use crate::event::Event;
use crate::testing::uid_n;
use crate::types::V2ipPrivacyStream;
use crate::wire::{op, DeviceFeature, DeviceUid};

use super::Harness;

/// A registry whose single peer is a source unit stamping 0x54 as it does.
fn source(n: u8) -> Harness {
    let mut h = Harness::new(n);
    h.hello(0x30, "ONEIP", "PV0001", DeviceFeature::V2IP_SOURCE);
    h
}

/// A report naming `uid` in its header, with the streams `report_stream`
/// builds for 1, 2 and 3.
fn report(uid: DeviceUid) -> Vec<u8> {
    let mut p = uid.as_bytes().to_vec();
    p.extend_from_slice(&[0, 3, 0xA5, 0x5A]);
    for n in 1..=3 {
        p.extend_from_slice(&report_stream(n));
    }
    p
}

/// Stream `n`, every field distinct from the others and from stream to
/// stream: a port and a key version too wide for a narrower read, and the
/// reserved bytes beside the version and after the key version set.
///
/// Stream 2 is in the clear, beside a set reserved byte.
fn report_stream(n: u8) -> Vec<u8> {
    let mut s = vec![239, 0, 0, n];
    s.extend_from_slice(&(5000 + u16::from(n) * 0x101).to_le_bytes());
    s.extend_from_slice(&[0x10 + n, 0xEE]);
    s.extend_from_slice(&[u8::from(n != 2), 0x20 + n, 0x30 + n]);
    s.extend((0..8).map(|i| 0x40 + n * 16 + i));
    s.extend((0..16).map(|i| 0x80 + n * 16 + i));
    s.extend((0..8).map(|i| 0xC0 + n * 8 + i));
    s.extend_from_slice(&(0x0102_0304 * u32::from(n)).to_le_bytes());
    s.push(0xEE);
    assert_eq!(s.len(), 48);
    s
}

/// What `report_stream(n)` carries.
fn expected(n: u8) -> V2ipPrivacyStream {
    let mut iv = [0; 8];
    let mut key_generator = [0; 16];
    let mut key_id = [0; 8];
    (0..8).for_each(|i| iv[usize::from(i)] = 0x40 + n * 16 + i);
    (0..16).for_each(|i| key_generator[usize::from(i)] = 0x80 + n * 16 + i);
    (0..8).for_each(|i| key_id[usize::from(i)] = 0xC0 + n * 8 + i);
    V2ipPrivacyStream {
        group: Ipv4Addr::new(239, 0, 0, n),
        port: 5000 + u16::from(n) * 0x101,
        version: 0x10 + n,
        on: n != 2,
        full_id: 0x20 + n,
        short_id: 0x30 + n,
        iv,
        key_generator,
        key_id,
        key_version: 0x0102_0304 * u32::from(n),
    }
}

/// A report is read field by field, video, audio and ancillary data in that
/// order, and filed under its sender whatever uid its header names.
#[test]
fn a_privacy_report_is_read_field_by_field() {
    let mut h = source(140);
    h.feed_proto(op::V2IP_PRIVACY, 0x30, &report(uid_n(141)));

    let p = h.device().v2ip_privacy.expect("no report was kept");
    assert_eq!(p.video, expected(1));
    assert_eq!(p.audio, expected(2));
    assert_eq!(p.anc, expected(3));
    assert!(h.saw(|e| matches!(
        e,
        Event::V2ipPrivacyChanged { device, .. } if *device == h.sender
    )));

    h.events.clear();
    h.feed_proto(op::V2IP_PRIVACY, 0x30, &report(uid_n(141)));
    assert!(h.events.is_empty(), "an unchanged report was announced");
}

/// Only a whole report of three streams is kept.
#[test]
fn only_a_whole_privacy_report_is_kept() {
    let mut h = source(142);
    let whole = report(h.sender);

    let mut request = whole.clone();
    request[16] = 1;
    let mut two = whole.clone();
    two[17] = 2;
    for (what, frame) in [
        ("a request", &request[..]),
        ("a count of two", &two[..]),
        ("a frame a byte short", &whole[..whole.len() - 1]),
    ] {
        h.feed_proto(op::V2IP_PRIVACY, 0x30, frame);
        assert_eq!(h.device().v2ip_privacy, None, "{what} was kept");
    }

    let mut longer = whole;
    longer[17] = 4;
    longer.extend_from_slice(&report_stream(4));
    h.feed_proto(op::V2IP_PRIVACY, 0x30, &longer);
    let p = h
        .device()
        .v2ip_privacy
        .expect("a longer report was dropped");
    assert_eq!(p.anc, expected(3));
}
