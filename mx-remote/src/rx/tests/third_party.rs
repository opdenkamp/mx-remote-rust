// Author: Lars Op den Kamp (lars@opdenkamp-it.nl)
// Copyright (c) 2026 Op den Kamp IT Solutions

//! `THIRD_PARTY` (0x55): what a receiver reports of an application's message.
//!
//! The fixtures are composed from the layout this library defines for the
//! opcode, so they rule out a slip rather than a shared misreading.

use std::net::Ipv4Addr;

use super::*;
use crate::types::{ThirdPartyMessage, ThirdPartyVendor};
use crate::wire::{op, ThirdPartyVendorFlag};

fn known(n: u8) -> Harness {
    let mut h = Harness::new(n);
    h.hello(0x30, "audio-cast", "P9SN00000000", DeviceFeature::MANAGER);
    h.events.clear();
    h
}

fn messages(h: &Harness) -> Vec<(DeviceUid, ThirdPartyMessage)> {
    h.events
        .iter()
        .filter_map(|e| match e {
            Event::ThirdPartyMessage { device, message } => Some((*device, message.clone())),
            _ => None,
        })
        .collect()
}

/// The flag byte comes first and the three identifying bytes follow, and the
/// payload after them arrives byte for byte.
#[test]
fn a_message_reports_its_vendor_payload_and_sender() {
    let mut h = known(0x31);
    let mut p = vec![0x81, b'O', b'D', b'K'];
    let body = poisoned(37);
    p.extend_from_slice(&body);
    h.feed(op::THIRD_PARTY, &p);

    let got = messages(&h);
    assert_eq!(got.len(), 1, "{:?}", h.events);
    let (device, message) = &got[0];
    assert_eq!(*device, uid_n(0x31));
    assert_eq!(message.vendor.flags, ThirdPartyVendorFlag::from_bits(0x81));
    assert_eq!(message.vendor.id, *b"ODK");
    assert_eq!(message.payload, body);
    assert_eq!(message.address, Some(Ipv4Addr::new(10, 8, 8, 9)));
    assert!(message
        .vendor
        .same_vendor(ThirdPartyVendor::private(*b"ODK")));
}

/// A vendor id alone is a message with nothing after it, and anything shorter
/// is not a message.
#[test]
fn the_vendor_id_is_the_whole_minimum() {
    let mut h = known(0x32);
    h.feed(op::THIRD_PARTY, &[0x00, 0x00, 0x1A, 0x2B]);
    let got = messages(&h);
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].1.vendor, ThirdPartyVendor::oui([0x00, 0x1A, 0x2B]));
    assert!(got[0].1.payload.is_empty());

    h.events.clear();
    for len in 0..4 {
        h.feed(op::THIRD_PARTY, &poisoned(len));
    }
    assert!(messages(&h).is_empty(), "{:?}", h.events);
}

/// Stamped at the lowest version, a message is still read.
#[test]
fn a_message_stamped_at_the_lowest_version_is_read() {
    let mut h = known(0x33);
    h.feed_proto(op::THIRD_PARTY, 0x01, &[1, b'O', b'D', b'K', 7]);
    assert_eq!(messages(&h).len(), 1);
}

/// A sender that has not announced itself is not heard.
#[test]
fn a_message_from_a_stranger_is_dropped() {
    let mut h = Harness::new(0x34);
    h.feed(op::THIRD_PARTY, &[1, b'O', b'D', b'K', 7]);
    assert!(messages(&h).is_empty(), "{:?}", h.events);
}

/// This client never hears itself. Its own uid is never a known device
/// either, so the stranger rule drops the frame too, and only the two
/// together being lost turns this red.
#[test]
fn a_message_from_this_client_is_dropped() {
    let mut h = known(0x35);
    let me = h.state.uid;
    h.feed_as(me, op::THIRD_PARTY, &[1, b'O', b'D', b'K', 7]);
    assert!(messages(&h).is_empty(), "{:?}", h.events);
}

/// Whether two ids name the same vendor rests on the not-an-OUI flag and the
/// three bytes, not on the other flag bits.
#[test]
fn same_vendor_ignores_the_unnamed_flag_bits() {
    let odk = ThirdPartyVendor::private(*b"ODK");
    let mut flagged = odk;
    flagged.flags = ThirdPartyVendorFlag::from_bits(0xF1);
    assert!(odk.same_vendor(flagged));
    assert!(!odk.same_vendor(ThirdPartyVendor::oui(*b"ODK")));
    assert!(!odk.same_vendor(ThirdPartyVendor::private(*b"ODL")));
    assert!(!odk.same_vendor(ThirdPartyVendor::private(*b"PDK")));
    assert!(!odk.same_vendor(ThirdPartyVendor::private(*b"OEK")));
}
