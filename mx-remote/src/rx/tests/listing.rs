// Author: Lars Op den Kamp (lars@opdenkamp-it.nl)
// Copyright (c) 2026 Op den Kamp IT Solutions

//! What a mesh controller lists about each device it has heard: identity as
//! announced, registration order, and whether a device's bays are in use.

use std::net::Ipv4Addr;
use std::time::{Duration, Instant};

use crate::event::Event;
use crate::rx::process_frame_on;
use crate::testing::{bay_config_rec, datagram, hello_payload, poisoned, uid_n};
use crate::types::MacAddress;
use crate::wire::{op, BayFeatures, BayStatus, DeviceFeature, Path};
use crate::{BayInfo, DeviceInfo};

use super::Harness;

/// A hello whose fields sit over poison: the name fills its sixteen bytes
/// with no terminator, and the version stops at a NUL with poison behind it.
///
/// Composed alongside the reading it checks, from the hello layout: it rules
/// out a slip, not a shared misreading of that layout.
fn raw_hello(name: &[u8], version: &[u8]) -> Vec<u8> {
    let mut p = poisoned(54);
    p[0..2].copy_from_slice(&0x2Fu16.to_le_bytes());
    p[2..2 + name.len()].copy_from_slice(name);
    if name.len() < 16 {
        p[2 + name.len()] = 0;
    }
    p[18..26].copy_from_slice(b"RAW0001\0");
    p[34..34 + version.len()].copy_from_slice(version);
    if version.len() < 16 {
        p[34 + version.len()] = 0;
    }
    p[50..54].copy_from_slice(&DeviceFeature::MESH.bits().to_le_bytes());
    p
}

/// The name and version are reported as the bytes announced: a byte outside
/// ASCII is kept, a name filling its field is kept whole without running into
/// the serial behind it, and an empty or blank name is not substituted.
#[test]
fn the_name_and_version_are_reported_as_announced() {
    let mut h = Harness::new(70);
    let name = b"Caf\xE9 lounge 1234";
    assert_eq!(name.len(), 16);
    h.feed(op::SYS_HELLO, &raw_hello(name, b"4.8\xB1-7"));
    let info = DeviceInfo::of(h.device(), Instant::now(), false);
    assert_eq!(info.raw_name, name.to_vec());
    assert_eq!(info.raw_version, b"4.8\xB1-7".to_vec());

    // Both bytes outside ASCII read as the same replacement character, so
    // only the raw name tells this hello apart from the last one.
    h.events.clear();
    h.feed(
        op::SYS_HELLO,
        &raw_hello(b"Caf\xE8 lounge 1234", b"4.8\xB1-7"),
    );
    assert!(
        h.saw(|e| matches!(e, Event::DeviceConfigChanged { .. })),
        "a renamed device was not reported changed"
    );
    h.events.clear();
    h.feed(
        op::SYS_HELLO,
        &raw_hello(b"Caf\xE8 lounge 1234", b"4.8\xB2-7"),
    );
    assert!(
        h.saw(|e| matches!(e, Event::DeviceConfigChanged { .. })),
        "a new version was not reported changed"
    );

    h.feed(op::SYS_HELLO, &raw_hello(b"", b"5.0"));
    let info = DeviceInfo::of(h.device(), Instant::now(), false);
    assert_eq!(info.name, "Unknown");
    assert!(info.raw_name.is_empty(), "an empty name was substituted");
    assert_eq!(info.raw_version, b"5.0".to_vec());

    h.feed(op::SYS_HELLO, &raw_hello(b"   ", b"5.0"));
    let info = DeviceInfo::of(h.device(), Instant::now(), false);
    assert_eq!(
        info.raw_name,
        b"   ".to_vec(),
        "a blank name was substituted"
    );
}

/// Whether a device's bays are online, as a unit judges it.
fn bays_online(h: &Harness) -> bool {
    DeviceInfo::of(h.device(), Instant::now(), false).bays_online
}

/// A primary bay list of one input.
fn one_bay() -> Vec<u8> {
    bay_config_rec(
        0,
        0,
        0,
        "Input 1",
        "",
        BayStatus::NONE,
        BayFeatures::HDMI_IN,
    )
}

/// A OneIP device's bays come online with its source list, of any length a
/// unit takes, and not with its bay lists.
#[test]
fn a_oneip_device_comes_online_with_its_source_list() {
    let mut h = Harness::new(71);
    h.hello(0x2F, "ONEIP", "ON0001", DeviceFeature::V2IP_SINK);
    assert!(!bays_online(&h), "online from its hello alone");
    h.feed(op::SYS_BAY_CONFIG, &one_bay());
    h.feed(op::SYS_BAY_CONFIG_SECONDARY, &one_bay());
    assert!(!bays_online(&h), "a OneIP device online by its bay list");
    h.feed(op::SYS_BAY_V2IP_SOURCES, &poisoned(39));
    assert!(!bays_online(&h), "online by a list a unit refuses");
    h.feed(op::SYS_BAY_V2IP_SOURCES, &[]);
    assert!(bays_online(&h));
}

/// Any other device's bays come online with its primary bay list, and not
/// with an empty one, its secondary one or a source list.
#[test]
fn another_device_comes_online_with_its_primary_bay_list() {
    let mut h = Harness::new(72);
    h.hello(0x2F, "FF88", "MX0001", DeviceFeature::VIDEO_ROUTING);
    h.feed(op::SYS_BAY_V2IP_SOURCES, &[]);
    h.feed(op::SYS_BAY_CONFIG_SECONDARY, &one_bay());
    h.feed(op::SYS_BAY_CONFIG, &[]);
    assert!(!bays_online(&h));
    h.feed(op::SYS_BAY_CONFIG, &one_bay());
    assert!(bays_online(&h));
}

/// A device announcing a reboot, or going offline, is offline until its bays
/// arrive again, whatever it announces meanwhile.
#[test]
fn a_reboot_or_silence_takes_the_bays_offline() {
    let mut h = Harness::new(73);
    let plain = DeviceFeature::from_bits(0);
    h.hello(0x2F, "FF88", "MX0002", plain);
    h.feed(op::SYS_BAY_CONFIG, &one_bay());
    assert!(bays_online(&h));

    h.hello(0x2F, "FF88", "MX0002", DeviceFeature::STATUS_REBOOT);
    assert!(!bays_online(&h), "still online while rebooting");
    assert!(
        DeviceInfo::of(h.device(), Instant::now(), false).online,
        "a rebooting device is no longer connected"
    );
    h.hello(0x2F, "FF88", "MX0002", plain);
    assert!(!bays_online(&h), "online again by its hello");
    h.feed(op::SYS_BAY_CONFIG, &one_bay());
    assert!(bays_online(&h));

    let later = Instant::now() + Duration::from_secs(120);
    let sender = h.sender;
    let device = h.state.device_mut(sender).expect("registered");
    assert!(
        !DeviceInfo::of(device, later, false).bays_online,
        "online while silent"
    );
    device.check_online(later, &mut Vec::new());
    h.hello(0x2F, "FF88", "MX0002", plain);
    assert!(!bays_online(&h), "back online by its hello");
    h.feed(op::SYS_BAY_CONFIG, &one_bay());
    assert!(bays_online(&h));
}

/// A device is reached by multicast while a frame from it arrived by
/// multicast in the last two minutes, the frame registering it excepted.
/// Without the endpoints told apart, that is not known.
#[test]
fn a_device_is_reached_by_multicast_while_heard_by_it() {
    let mut h = Harness::new(74);
    let multicast = |h: &Harness, at: Instant| DeviceInfo::of(h.device(), at, true).multicast;
    let hello = datagram(
        h.sender,
        op::SYS_HELLO,
        0x01,
        &hello_payload(0x2F, "ONEIP", "MC0001", "4.8.0", DeviceFeature::MESH),
    );
    let t0 = Instant::now();
    process_frame_on(&mut h.state, &hello, None, Some(Path::Multicast), t0);
    assert_eq!(
        multicast(&h, t0),
        Some(false),
        "reached by the frame that registered it"
    );
    assert_eq!(DeviceInfo::of(h.device(), t0, false).multicast, None);

    let t1 = t0 + Duration::from_secs(1);
    process_frame_on(&mut h.state, &hello, None, Some(Path::Broadcast), t1);
    assert_eq!(multicast(&h, t1), Some(false), "reached by broadcast");

    let t2 = t1 + Duration::from_secs(1);
    process_frame_on(&mut h.state, &hello, None, Some(Path::Multicast), t2);
    assert_eq!(multicast(&h, t2), Some(true));
    let edge = t2 + Duration::from_secs(120);
    assert_eq!(
        multicast(&h, edge - Duration::from_millis(1)),
        Some(true),
        "forgotten inside two minutes"
    );
    assert_eq!(multicast(&h, edge), Some(false), "kept past two minutes");
}

/// A device is forgotten three offline windows after it was last heard, to
/// the moment: 60s windows at protocol 0x2A and later, 15s from 0x20, 180s
/// below.
#[test]
fn a_device_is_forgotten_three_offline_windows_after_its_last_frame() {
    for (n, protocol, forget) in [(75, 0x2A, 180), (76, 0x29, 45), (77, 0x1F, 540)] {
        let mut h = Harness::new(n);
        let t0 = Instant::now();
        let hello = hello_payload(protocol, "ONEIP", "FG0002", "4.8.0", DeviceFeature::MESH);
        h.feed_at(op::SYS_HELLO, &hello, t0);
        let edge = t0 + Duration::from_secs(forget);

        h.state
            .forget_silent(edge - Duration::from_nanos(1), &mut h.events);
        assert!(h.state.device(h.sender).is_some(), "forgotten early");
        h.state.forget_silent(edge, &mut h.events);
        assert!(h.state.device(h.sender).is_none(), "kept at {forget}s");
        assert!(h.saw(|e| *e == Event::DeviceUnregistered { device: h.sender }));
    }
}

/// A hello's captured source address waits ten seconds for the hello's UDP
/// copy to register its sender, and is dropped by the next pass after that.
#[test]
fn a_captured_source_address_waits_for_its_sender_to_register() {
    let mut h = Harness::new(78);
    let mac = MacAddress([0x02, 0x0A, 0x0B, 0x0C, 0x0D, 0x0E]);
    let t0 = Instant::now();
    let hello = hello_payload(0x2A, "ONEIP", "MA0002", "4.8.0", DeviceFeature::MESH);

    h.state.note_hello_source(h.sender, mac, t0);
    h.state
        .forget_silent(t0 + Duration::from_millis(9_999), &mut h.events);
    h.feed_at(op::SYS_HELLO, &hello, t0);
    assert_eq!(h.state.hello_source(h.sender), Some(mac));

    let late = uid_n(79);
    h.state.note_hello_source(late, mac, t0);
    h.state
        .forget_silent(t0 + Duration::from_secs(10), &mut h.events);
    h.feed_full(late, op::SYS_HELLO, 1, &hello, t0);
    assert_eq!(h.state.hello_source(late), None, "kept past ten seconds");
    assert_eq!(
        h.state.hello_source(h.sender),
        Some(mac),
        "a registered one"
    );
}

/// A hello shorter than 54 bytes is ignored whole: it registers nothing and
/// changes nothing about a known device. A longer one is read by its first
/// 54 bytes.
#[test]
fn a_hello_is_taken_only_at_its_full_length() {
    let mut h = Harness::new(80);
    let hello = |name: &str| hello_payload(0x2A, name, "SH0001", "4.8.0", DeviceFeature::MESH);
    h.feed(op::SYS_HELLO, &hello("Short")[..53]);
    assert!(
        h.state.device(h.sender).is_none(),
        "registered by a short hello"
    );

    let mut long = hello("Long");
    long.extend_from_slice(&poisoned(6));
    h.feed(op::SYS_HELLO, &long);
    assert_eq!(h.device().hello.name, "Long");
    assert_eq!(h.device().hello.features, DeviceFeature::MESH);

    h.feed(op::SYS_HELLO, &hello("Renamed")[..53]);
    assert_eq!(h.device().hello.name, "Long", "updated by a short hello");
}

/// Every frame from a sender not registered is dropped, but a hello and a
/// discover.
#[test]
fn a_stranger_is_heard_only_by_its_hello_or_discover() {
    let mut h = Harness::new(81);
    let own = h.state.uid;
    h.feed(op::SYS_PING, own.as_bytes());
    assert!(!h.state.hello_requested, "a stranger's ping was answered");
    h.feed(op::SYS_DISCOVER, &[]);
    assert!(h.state.hello_requested, "a stranger's discover was not");
}

/// A short hello does not bring back a device gone silent; a whole one does.
#[test]
fn only_a_whole_hello_brings_a_silent_device_back() {
    let mut h = Harness::new(82);
    let hello = hello_payload(0x2A, "Unit", "SH0002", "4.8.0", DeviceFeature::MESH);
    let t0 = Instant::now();
    h.feed_at(op::SYS_HELLO, &hello, t0);
    let later = t0 + Duration::from_secs(61);
    h.feed_at(op::SYS_HELLO, &hello[..53], later);
    assert!(!h.device().is_online(later), "back by a short hello");
    h.feed_at(op::SYS_HELLO, &hello, later);
    assert!(h.device().is_online(later));
}

/// A frame that is a second copy, or carries an opcode no handler takes,
/// still keeps a live device alive, as every frame with a valid header does.
#[test]
fn every_frame_with_a_valid_header_keeps_a_device_alive() {
    let mut h = Harness::new(83);
    let hello = hello_payload(0x2A, "Unit", "RF0001", "4.8.0", DeviceFeature::MESH);
    let t0 = Instant::now();
    h.feed_at(op::SYS_HELLO, &hello, t0);
    let window = Duration::from_secs(60);

    let pulse = datagram(h.sender, op::SYS_MONITORING_PULSE, 1, &[]);
    let t1 = t0 + Duration::from_secs(30);
    process_frame_on(&mut h.state, &pulse, None, Some(Path::Multicast), t1);
    let t2 = t1 + Duration::from_millis(50);
    process_frame_on(&mut h.state, &pulse, None, Some(Path::Broadcast), t2);
    assert!(
        h.device().is_online(t2 + window - Duration::from_millis(1)),
        "a copy"
    );

    let unhandled = datagram(h.sender, crate::wire::Opcode(0x17), 1, &[]);
    let t3 = t2 + Duration::from_secs(30);
    process_frame_on(&mut h.state, &unhandled, None, None, t3);
    assert!(
        h.device().is_online(t3 + window - Duration::from_millis(1)),
        "an unhandled opcode"
    );
}

/// A bay list from a device gone silent is taken, and does not bring it
/// back: it stays not alive, the next pass takes its bays offline again,
/// and only a hello revives it, still without its bays.
#[test]
fn a_bay_list_does_not_bring_a_silent_device_back() {
    let mut h = Harness::new(84);
    let hello = hello_payload(
        0x2A,
        "FF88",
        "RF0002",
        "4.8.0",
        DeviceFeature::VIDEO_ROUTING,
    );
    let t0 = Instant::now();
    h.feed_at(op::SYS_HELLO, &hello, t0);
    let sender = h.sender;
    let t1 = t0 + Duration::from_secs(61);
    // A pass has already seen it go.
    h.state
        .device_mut(sender)
        .expect("registered")
        .check_online(t1, &mut Vec::new());
    h.feed_at(op::SYS_BAY_CONFIG, &one_bay(), t1);
    assert!(h.device().bay(0).is_some(), "the bay list was not taken");
    assert!(!h.device().is_online(t1), "brought back by its bay list");
    assert!(!DeviceInfo::of(h.device(), t1, false).bays_online);

    let t2 = t1 + Duration::from_secs(1);
    h.state
        .device_mut(sender)
        .expect("registered")
        .check_online(t2, &mut Vec::new());
    h.feed_at(op::SYS_HELLO, &hello, t2);
    assert!(h.device().is_online(t2));
    assert!(
        !DeviceInfo::of(h.device(), t2, false).bays_online,
        "online by the bay list it sent while silent"
    );
}

/// The serial is reported as the bytes announced, up to its first NUL.
#[test]
fn the_serial_is_reported_as_announced() {
    let mut h = Harness::new(85);
    let mut hello = raw_hello(b"Unit", b"4.8.0");
    hello[18..34].copy_from_slice(b"SN\xC900001\0\xA5\xA5\xA5\xA5\xA5\xA5\xA5");
    h.feed(op::SYS_HELLO, &hello);
    let info = DeviceInfo::of(h.device(), Instant::now(), false);
    assert_eq!(info.raw_serial, b"SN\xC900001".to_vec());

    // Only the raw serial tells this one apart from the last.
    h.events.clear();
    hello[20] = 0xCA;
    h.feed(op::SYS_HELLO, &hello);
    assert!(
        h.saw(|e| matches!(e, Event::DeviceConfigChanged { .. })),
        "a new serial was not reported changed"
    );

    // One filling its field is cut at the field, not at the version's end.
    hello[18..34].copy_from_slice(b"SN\xC90000123456789");
    h.feed(op::SYS_HELLO, &hello);
    let info = DeviceInfo::of(h.device(), Instant::now(), false);
    assert_eq!(info.raw_serial, b"SN\xC90000123456789".to_vec());
}

/// A ping is answered only at exactly 16 bytes, its target's uid, as a unit
/// answers one.
#[test]
fn a_ping_is_answered_only_at_its_exact_length() {
    let mut h = Harness::new(86);
    let hello = hello_payload(0x2A, "Unit", "PG0001", "4.8.0", DeviceFeature::MESH);
    h.feed(op::SYS_HELLO, &hello);
    let own = h.state.uid;
    let mut longer = own.as_bytes().to_vec();
    longer.push(0);
    h.feed(op::SYS_PING, &longer);
    assert!(!h.state.hello_requested, "a 17-byte ping was answered");
    h.feed(op::SYS_PING, &own.as_bytes()[..15]);
    assert!(!h.state.hello_requested, "a 15-byte ping was answered");
    h.feed(op::SYS_PING, own.as_bytes());
    assert!(h.state.hello_requested);
}

/// A bay reports its whole status word as last sent, by its bay list or its
/// status report: an encoder error and the source's HDCP mode included,
/// which no other field carries. Before either, it has reported none.
#[test]
fn a_bay_reports_its_whole_status_word() {
    let mut h = Harness::new(87);
    h.hello(0x2F, "FF88", "SW0001", DeviceFeature::VIDEO_ROUTING);
    let first = BayStatus::from_bits(0x0080_0008);
    h.feed(
        op::SYS_BAY_CONFIG,
        &bay_config_rec(0, 0, 0, "Input 1", "", first, BayFeatures::HDMI_IN),
    );
    let status = |h: &Harness| BayInfo::of(&h.state, h.bay(0)).status;
    assert_eq!(status(&h), Some(first));

    let mut record = poisoned(32);
    record[0..2].copy_from_slice(&0u16.to_le_bytes());
    let next = BayStatus::ENCODER_ERROR | BayStatus::from_bits(2 << 22);
    record[20..24].copy_from_slice(&next.bits().to_le_bytes());
    h.feed(op::BAY_STATUS, &record);
    let reported = status(&h).expect("a status word");
    assert_eq!(reported, next);
    assert!(reported.has(BayStatus::ENCODER_ERROR));
    assert_eq!(reported.hdcp(), 2);
}

/// A uplink link report of the current layout, `features` at 2 and the
/// querier at 32, poisoned elsewhere.
fn uplink(features: u16, querier: [u8; 4], len: usize) -> Vec<u8> {
    let mut p = poisoned(len);
    p[0..2].copy_from_slice(&3u16.to_le_bytes());
    p[2..4].copy_from_slice(&features.to_le_bytes());
    p[4..21].fill(0);
    p[28..32].copy_from_slice(&[10, 8, 8, 9]);
    p[32..36].copy_from_slice(&querier);
    p
}

/// A unit's IGMP querier is the one its last uplink report names, as a unit
/// stores it: none while the report does not support IGMP or names 0.0.0.0.
/// Only a whole report of the current layout counts, and only the uplink.
#[test]
fn a_units_igmp_querier_is_its_uplinks() {
    const IGMP: u16 = 1 << 3;
    const UPLINK: u16 = 1 << 6;
    let mut h = Harness::new(88);
    h.hello(0x2F, "ONEIP", "IG0001", DeviceFeature::V2IP_SINK);
    let querier = |h: &Harness| DeviceInfo::of(h.device(), Instant::now(), false).igmp_querier;
    assert_eq!(querier(&h), None);

    h.feed_proto(
        op::NET_LINK_STATUS,
        0x22,
        &uplink(IGMP | UPLINK, [10, 1, 2, 3], 144),
    );
    assert_eq!(querier(&h), Some(Ipv4Addr::new(10, 1, 2, 3)));

    for (protocol, payload) in [
        (0x22, uplink(IGMP, [10, 9, 9, 9], 144)),
        (0x22, uplink(IGMP | UPLINK, [10, 9, 9, 9], 143)),
        (0x21, uplink(IGMP | UPLINK, [10, 9, 9, 9], 152)),
    ] {
        h.feed_proto(op::NET_LINK_STATUS, protocol, &payload);
        assert_eq!(
            querier(&h),
            Some(Ipv4Addr::new(10, 1, 2, 3)),
            "{protocol:#x}"
        );
    }

    h.feed_proto(
        op::NET_LINK_STATUS,
        0x22,
        &uplink(UPLINK, [10, 1, 2, 3], 144),
    );
    assert_eq!(querier(&h), None, "a report without IGMP support");
    h.feed_proto(
        op::NET_LINK_STATUS,
        0x22,
        &uplink(IGMP | UPLINK, [0; 4], 144),
    );
    assert_eq!(querier(&h), None, "0.0.0.0");
}

/// A frame whose opcode is 0x58 or above has no valid header to a unit: it
/// neither keeps its sender alive nor proves its path. Below that, a frame
/// does both whether or not a handler takes it, as an application's message
/// at 0x55 does.
#[test]
fn an_opcode_past_the_table_touches_no_timer() {
    let mut h = Harness::new(89);
    let hello = hello_payload(0x2A, "App", "OP0001", "4.8.0", DeviceFeature::MANAGER);
    let t0 = Instant::now();
    h.feed_at(op::SYS_HELLO, &hello, t0);
    let t1 = t0 + Duration::from_secs(50);
    let past = datagram(h.sender, crate::wire::Opcode(0x58), 1, &[]);
    process_frame_on(&mut h.state, &past, None, Some(Path::Multicast), t1);
    assert!(
        !h.device().is_online(t0 + Duration::from_secs(60)),
        "kept alive by 0x58"
    );
    assert_eq!(h.device().heard_multicast, None, "0x58 proved its path");

    let message = datagram(h.sender, op::THIRD_PARTY, 1, b"VNDRhello");
    h.events.extend(process_frame_on(
        &mut h.state,
        &message,
        None,
        Some(Path::Multicast),
        t1,
    ));
    assert!(
        h.saw(|e| matches!(e, Event::ThirdPartyMessage { .. })),
        "the application's message was dropped"
    );
    assert!(
        h.device().is_online(t0 + Duration::from_secs(60)),
        "0x55 did not keep its sender alive"
    );
    assert_eq!(
        h.device().heard_multicast,
        Some(t1),
        "0x55 did not prove its path"
    );
}
