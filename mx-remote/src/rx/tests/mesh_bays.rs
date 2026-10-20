// Author: Lars Op den Kamp (lars@opdenkamp-it.nl)
// Copyright (c) 2026 Op den Kamp IT Solutions

//! Mesh bays: the OneIP bays a unit lists for other units' inputs and
//! outputs, built from its bay mappings and described by the bays they stand
//! for.

use crate::event::Event;
use crate::wire::{op, BayFeatures, BayStatus, BayUid, DeviceFeature, DeviceUid};
use crate::BayInfo;

use crate::testing::{bay_config_rec, poisoned, uid_n};

use super::Harness;

/// A receiver with only its own output, on port 127, whose bay configuration
/// also carries records for two mesh inputs on ports 1 and 2 and a mesh
/// output on 128.
fn receiver(n: u8) -> Harness {
    let mut h = Harness::new(n);
    h.hello(0x2F, "ONEIP", "RX0001", DeviceFeature::V2IP_SINK);
    let mut cfg = bay_config_rec(
        128,
        1,
        1,
        "Output 2",
        "Stale",
        BayStatus::NONE,
        BayFeatures::V2IP_SINK_REMOTE,
    );
    for port in [1, 2] {
        cfg.extend(bay_config_rec(
            port,
            0,
            port,
            &format!("Input {}", port + 1),
            "Stale",
            BayStatus::SIGNAL_DETECTED,
            BayFeatures::V2IP_SOURCE_REMOTE,
        ));
    }
    cfg.extend(bay_config_rec(
        127,
        1,
        0,
        "Output 1",
        "Lounge",
        BayStatus::NONE,
        BayFeatures::HDMI_OUT | BayFeatures::V2IP_SINK_LOCAL,
    ));
    h.feed(op::SYS_BAY_CONFIG, &cfg);
    h
}

/// Registers `owner` as a transmitter whose own input carries `name`.
fn transmitter(h: &mut Harness, owner: DeviceUid, name: &str, status: BayStatus) {
    h.feed_as(
        owner,
        op::SYS_HELLO,
        &crate::testing::hello_payload(
            0x2F,
            "ONEIP",
            "TX0001",
            "4.8.0",
            DeviceFeature::V2IP_SOURCE,
        ),
    );
    h.feed_as(
        owner,
        op::SYS_BAY_CONFIG,
        &bay_config_rec(
            0,
            0,
            0,
            "Input 1",
            name,
            status,
            BayFeatures::HDMI_IN | BayFeatures::V2IP_SOURCE_LOCAL,
        ),
    );
}

/// A 0x44 page as a unit sends it: the count above the direction bit, the
/// first port, four bytes of padding, then one uid per port.
fn page(is_input: bool, first_port: u16, uids: &[DeviceUid]) -> Vec<u8> {
    let count = u16::try_from(uids.len()).expect("a page holds fewer than 2^15 bays");
    let mut p = poisoned(8);
    p[0..2].copy_from_slice(&((count << 1) | u16::from(is_input)).to_le_bytes());
    p[2..4].copy_from_slice(&first_port.to_le_bytes());
    for uid in uids {
        p.extend_from_slice(uid.as_bytes());
    }
    p
}

fn info(h: &Harness, port: u16) -> BayInfo {
    BayInfo::of(&h.state, h.bay(port))
}

/// The records a unit sends for mesh bays are never taken; the mappings
/// create those bays, numbered by port, each standing for the unit it names.
#[test]
fn mesh_bays_come_from_the_mappings() {
    let mut h = receiver(1);
    assert!(
        h.device().bay(1).is_none(),
        "a mesh input's record was taken"
    );
    assert!(
        h.device().bay(128).is_none(),
        "a mesh output's record was taken"
    );
    assert!(
        h.device().bay(127).is_some(),
        "the unit's own bay was dropped"
    );

    let (a, b) = (uid_n(2), uid_n(3));
    h.feed(op::V2IP_BAY_MAPPINGS, &page(true, 1, &[a, b]));
    for (port, owner) in [(1, a), (2, b)] {
        let bay = h.bay(port);
        assert_eq!(bay.v2ip_uid, owner);
        assert!(bay.is_input() && !bay.is_local());
        assert_eq!(bay.bay_num(), u8::try_from(port).unwrap_or(0));
        assert_eq!(bay.port_name, format!("Input {}", port + 1));
        let uid = BayUid::new(h.sender, port);
        assert!(h.saw(|e| *e == Event::BayRegistered { bay: uid }));
    }

    // An output page names ports from 127 on.
    let c = uid_n(4);
    let sender = h.sender;
    h.feed(op::V2IP_BAY_MAPPINGS, &page(false, 127, &[sender, c]));
    let out = h.bay(128);
    assert!(out.is_output() && !out.is_local());
    assert_eq!((out.bay_num(), out.v2ip_uid), (1, c));
    assert_eq!(out.port_name, "Output 2");
}

/// The entry for the unit's own bay maps it to the unit and creates nothing,
/// and a page ends past the last bay of its direction.
#[test]
fn a_mapping_page_skips_the_unit_and_ends_at_the_last_bay() {
    let mut h = receiver(5);
    let sender = h.sender;
    h.feed(op::V2IP_BAY_MAPPINGS, &page(false, 127, &[sender]));
    let own = h.bay(127);
    assert!(own.is_local(), "the unit's own bay became a mesh bay");
    assert_eq!(own.v2ip_uid, sender);

    let (a, b) = (uid_n(6), uid_n(7));
    h.feed(op::V2IP_BAY_MAPPINGS, &page(false, 253, &[a, b]));
    assert_eq!(h.bay(253).v2ip_uid, a);
    assert!(h.device().bay(254).is_none(), "a port past the last bay");

    // A unit whose mappings arrive ahead of its bay configuration.
    let mut early = Harness::new(21);
    early.hello(0x2F, "ONEIP", "TX0002", DeviceFeature::V2IP_SOURCE);
    let own = early.sender;
    early.feed(op::V2IP_BAY_MAPPINGS, &page(true, 126, &[own, a]));
    assert!(
        early.device().bay(126).is_none(),
        "the unit stood for itself"
    );
    assert!(
        early.device().bay(127).is_none(),
        "an input ran into the outputs"
    );
}

/// Released firmware numbers the first 16 bays of each direction ahead of the
/// rest, and its pages run on by bay number across that split.
#[test]
fn a_released_unit_numbers_its_ports_split() {
    let mut h = Harness::new(24);
    h.hello(0x27, "ONEIP", "RX0003", DeviceFeature::V2IP_SINK);
    let inputs: Vec<DeviceUid> = (0..17).map(|i| uid_n(100 + i)).collect();
    h.feed(op::V2IP_BAY_MAPPINGS, &page(true, 1, &inputs));
    for (port, number) in [(1, 1), (15, 15), (32, 16), (33, 17)] {
        let bay = h.bay(port);
        assert!(bay.is_input(), "port {port}");
        assert_eq!(bay.bay_num(), number, "port {port}");
    }
    assert!(h.device().bay(16).is_none(), "an input on an output's port");

    let outputs = [h.sender, uid_n(130), uid_n(131)];
    h.feed(op::V2IP_BAY_MAPPINGS, &page(false, 16, &outputs));
    for (port, number) in [(17, 1), (18, 2)] {
        let bay = h.bay(port);
        assert!(bay.is_output(), "port {port}");
        assert_eq!(bay.bay_num(), number, "port {port}");
    }
    let last: Vec<DeviceUid> = (0..2).map(|i| uid_n(140 + i)).collect();
    h.feed(op::V2IP_BAY_MAPPINGS, &page(false, 143, &last));
    assert_eq!(h.bay(143).bay_num(), 16);
    assert!(h.bay(144).is_output());
    h.feed(op::V2IP_BAY_MAPPINGS, &page(true, 142, &last));
    assert_eq!(h.bay(142).bay_num(), 126);
    assert!(h.device().bay(143).is_some_and(|b| b.is_output()));

    h.feed(op::V2IP_BAY_MAPPINGS, &page(false, 31, &[uid_n(132)]));
    assert_eq!(h.bay(31).bay_num(), 15);

    // From protocol 0x2B a unit numbers its ports fixed.
    let mut fixed = Harness::new(29);
    fixed.hello(0x2B, "ONEIP", "RX0005", DeviceFeature::V2IP_SINK);
    fixed.feed(op::V2IP_BAY_MAPPINGS, &page(true, 16, &[uid_n(30)]));
    assert!(fixed.bay(16).is_input());
}

/// A released unit's output names its source in the same split numbering,
/// before the mapping that creates that source's bay.
#[test]
fn a_released_unit_routes_in_its_own_numbering() {
    let mut h = Harness::new(31);
    h.hello(0x27, "ONEIP", "RX0006", DeviceFeature::V2IP_SINK);
    let mut out = bay_config_rec(
        16,
        1,
        0,
        "Output 1",
        "Lounge",
        BayStatus::NONE,
        BayFeatures::HDMI_OUT | BayFeatures::V2IP_SINK_LOCAL,
    );
    let cases = [
        (15, true),
        (17, false),
        (31, false),
        (32, true),
        (142, true),
        (143, false),
    ];
    for (port, input) in cases {
        h.feed(op::MX_ROUTE, &route(16, 0xFFFE, 0xFFFE));
        out[3] = port;
        out[4] = port;
        h.feed(op::SYS_BAY_CONFIG, &out);
        let routed = input.then(|| BayUid::new(h.sender, u16::from(port)));
        assert_eq!(h.bay(16).video_source, routed, "port {port}");
    }
}

/// A page shorter than its count is not taken, and released firmware counts
/// more bays than it has, sending a zero uid for each: those map nothing, and
/// the entries after one keep their bays.
#[test]
fn a_mapping_page_is_taken_whole_and_skips_a_zero_uid() {
    let mut h = receiver(8);
    let (a, b) = (uid_n(9), uid_n(10));
    let mut short = page(true, 1, &[a, b]);
    short.truncate(8 + 16 + 15);
    h.feed(op::V2IP_BAY_MAPPINGS, &short);
    assert!(h.device().bay(1).is_none(), "a short page was taken");

    h.feed(
        op::V2IP_BAY_MAPPINGS,
        &page(true, 1, &[a, DeviceUid::ZERO, b]),
    );
    assert_eq!(h.bay(1).v2ip_uid, a);
    assert!(h.device().bay(2).is_none(), "a zero uid was mapped");
    assert_eq!(h.bay(3).v2ip_uid, b, "the entry after a zero uid");

    // A zero uid takes nothing away either.
    h.feed(op::V2IP_BAY_MAPPINGS, &page(true, 1, &[DeviceUid::ZERO]));
    assert_eq!(h.bay(1).v2ip_uid, a);
}

/// A mesh bay describes the bay it stands for, and keeps only its identity.
#[test]
fn a_mesh_bay_reads_the_bay_it_stands_for() {
    let mut h = receiver(11);
    let owner = uid_n(12);
    h.feed(op::V2IP_BAY_MAPPINGS, &page(true, 1, &[owner]));
    let unknown = info(&h, 1);
    assert_eq!(
        unknown.user_name, "Input 2",
        "named before its unit is known"
    );
    assert_eq!(unknown.signal_detected, None);

    transmitter(&mut h, owner, "Apple TV", BayStatus::SIGNAL_DETECTED);
    let i = info(&h, 1);
    assert_eq!(i.uid, BayUid::new(h.sender, 1));
    assert_eq!((i.port_name.as_str(), i.bay_num), ("Input 2", 1));
    assert!(!i.is_local);
    assert_eq!(i.features, BayFeatures::V2IP_SOURCE_REMOTE);
    assert_eq!(i.v2ip_uid, owner);
    assert_eq!(i.user_name, "Apple TV");
    assert_eq!(i.signal_detected, Some(true));
}

/// A change to the bay a mesh bay stands for is reported for the mesh bay as
/// well; a request arriving on it is not.
#[test]
fn a_change_to_the_owner_is_reported_for_its_mesh_bays() {
    let mut h = receiver(13);
    let owner = uid_n(14);
    transmitter(&mut h, owner, "Apple TV", BayStatus::NONE);
    h.feed(op::V2IP_BAY_MAPPINGS, &page(true, 1, &[owner]));
    h.events.clear();

    transmitter(&mut h, owner, "Kodi", BayStatus::NONE);
    let mesh = BayUid::new(h.sender, 1);
    let named = |bay| Event::NameChanged {
        bay,
        name: "Kodi".into(),
    };
    for bay in [mesh, BayUid::new(owner, 0)] {
        let times = h.events.iter().filter(|e| **e == named(bay)).count();
        assert_eq!(times, 1, "{bay:?}");
    }

    // What the receiver says about the bay is its own copy of it.
    h.events.clear();
    h.feed(op::DEV_POWER_CHANGE, &[1, 1]);
    assert!(h.events.is_empty(), "{:?}", h.events);

    let bay = BayUid::new(owner, 0);
    let unrepeated = [
        Event::BayRegistered { bay },
        Event::KeyPressed {
            bay,
            key: crate::wire::RcKey::NUM1,
        },
        Event::ActionReceived {
            bay,
            action: crate::wire::RcAction::POWER_ON,
        },
        Event::VolumeStep { bay, up: true },
        Event::AudioClipped {
            bay,
            clip: crate::types::AudioClip::default(),
        },
        Event::IrCaptured {
            bay,
            capture: crate::types::IrCapture::default(),
        },
    ];
    for event in unrepeated {
        let mut ev = vec![event.clone()];
        h.state.mirror_mesh_events(&mut ev);
        assert_eq!(ev, vec![event], "repeated for the mesh bay");
    }
}

/// A page pointing a mesh bay at another unit reports it under that unit's
/// name.
#[test]
fn a_remapped_mesh_bay_is_renamed() {
    let mut h = receiver(15);
    let (a, b) = (uid_n(16), uid_n(17));
    transmitter(&mut h, b, "Kodi", BayStatus::NONE);
    h.feed(op::V2IP_BAY_MAPPINGS, &page(true, 1, &[a]));
    h.events.clear();
    h.feed(op::V2IP_BAY_MAPPINGS, &page(true, 1, &[b]));
    let mesh = BayUid::new(h.sender, 1);
    assert!(h.saw(|e| *e
        == Event::NameChanged {
            bay: mesh,
            name: "Kodi".into()
        }));
    assert!(!h.saw(|e| matches!(e, Event::BayRegistered { .. })));

    h.events.clear();
    h.feed(op::V2IP_BAY_MAPPINGS, &page(true, 1, &[b]));
    assert!(h.events.is_empty(), "an unchanged page: {:?}", h.events);
}

/// A unit with no input of its own describes none of the mesh bays standing
/// for its inputs, and what happens to its own mesh bays is not theirs.
#[test]
fn a_unit_without_an_input_describes_no_mesh_input() {
    let mut h = receiver(25);
    let other = uid_n(26);
    // A second receiver, whose input 1 the first lists under its own.
    h.feed_as(
        other,
        op::SYS_HELLO,
        &crate::testing::hello_payload(0x2F, "ONEIP", "RX0004", "4.8.0", DeviceFeature::V2IP_SINK),
    );
    h.feed_as(other, op::V2IP_BAY_MAPPINGS, &page(true, 1, &[uid_n(27)]));
    h.feed(op::V2IP_BAY_MAPPINGS, &page(true, 1, &[other]));
    h.events.clear();

    h.feed_as(other, op::V2IP_BAY_MAPPINGS, &page(true, 1, &[uid_n(28)]));
    let mesh = BayUid::new(h.sender, 1);
    assert!(
        !h.saw(|e| matches!(e, Event::NameChanged { bay, .. } if *bay == mesh)),
        "{:?}",
        h.events
    );
}

/// A unit sends its bay configuration ahead of its mappings, so its output's
/// route names a mesh input before that input exists.
#[test]
fn a_route_names_a_mesh_input_before_its_mapping() {
    let mut h = Harness::new(18);
    h.hello(0x2F, "ONEIP", "RX0002", DeviceFeature::V2IP_SINK);
    let mut out = bay_config_rec(
        127,
        1,
        0,
        "Output 1",
        "Lounge",
        BayStatus::NONE,
        BayFeatures::HDMI_OUT | BayFeatures::V2IP_SINK_LOCAL,
    );
    out[3] = 2;
    out[4] = 2;
    h.feed(op::SYS_BAY_CONFIG, &out);
    assert_eq!(h.bay(127).video_source, Some(BayUid::new(h.sender, 2)));

    out[3] = 126;
    h.feed(op::SYS_BAY_CONFIG, &out);
    assert_eq!(h.bay(127).video_source, Some(BayUid::new(h.sender, 126)));

    // Only an input port is a source.
    out[3] = 128;
    h.feed(op::SYS_BAY_CONFIG, &out);
    assert_eq!(h.bay(127).video_source, Some(BayUid::new(h.sender, 126)));

    // So it is in the routing change report.
    h.feed(op::MX_ROUTE, &route(127, 3, 4));
    assert_eq!(h.bay(127).video_source, Some(BayUid::new(h.sender, 3)));
    let audio = h.bay(127).effective_audio_source();
    assert_eq!(audio, Some(BayUid::new(h.sender, 4)));
    h.feed(op::MX_ROUTE, &route(127, 128, 128));
    assert_eq!(h.bay(127).video_source, Some(BayUid::new(h.sender, 3)));
}

/// A 0x08 routing change as a unit sends it: u16 ports, the sink at 0, the
/// selected input at 2, video at 4, the scrambled flag at 6 and audio at 7.
fn route(sink: u16, video: u16, audio: u16) -> Vec<u8> {
    let mut p = poisoned(9);
    p[0..2].copy_from_slice(&sink.to_le_bytes());
    p[2..4].copy_from_slice(&video.to_le_bytes());
    p[4..6].copy_from_slice(&video.to_le_bytes());
    p[7..9].copy_from_slice(&audio.to_le_bytes());
    p
}

/// A unit reports 0xFF for both of an output's sources whenever either is
/// unrouted, so its bay configuration keeps the route; the routing change
/// report clears it.
#[test]
fn a_route_is_cleared_only_by_a_routing_change() {
    let mut h = receiver(32);
    let mut out = bay_config_rec(
        127,
        1,
        0,
        "Output 1",
        "Lounge",
        BayStatus::NONE,
        BayFeatures::HDMI_OUT | BayFeatures::V2IP_SINK_LOCAL,
    );
    out[3] = 2;
    out[4] = 3;
    h.feed(op::SYS_BAY_CONFIG, &out);
    out[3] = 0xFF;
    out[4] = 0xFF;
    h.feed(op::SYS_BAY_CONFIG, &out);
    let at = |port| Some(BayUid::new(h.sender, port));
    assert_eq!(h.bay(127).video_source, at(2));
    assert_eq!(h.bay(127).effective_audio_source(), at(3));

    h.feed(op::MX_ROUTE, &route(127, 0xFFFE, 0xFFFE));
    assert_eq!(h.bay(127).video_source, None);
}

/// A source is found by the name its unit gives it, and not while that unit
/// hides it.
#[test]
fn a_mesh_input_is_found_by_its_owner_name() {
    let mut h = receiver(19);
    let owner = uid_n(20);
    transmitter(&mut h, owner, "Apple TV", BayStatus::NONE);
    h.feed(op::V2IP_BAY_MAPPINGS, &page(true, 1, &[owner]));
    let device = h.device();
    let found = h.state.input_by_user_name(device, "Apple TV");
    assert_eq!(found.map(|b| b.port), Some(1));
    assert!(h.state.input_by_user_name(device, "Stale").is_none());

    transmitter(&mut h, owner, "Apple TV", BayStatus::HIDDEN);
    assert!(h.state.input_by_user_name(h.device(), "Apple TV").is_none());
}

/// A stream is traced to the unit producing it, not to a mesh bay another
/// unit lists it under.
#[test]
fn a_stream_is_traced_to_its_own_unit() {
    let mut h = receiver(22);
    let owner = uid_n(23);
    transmitter(&mut h, owner, "Apple TV", BayStatus::NONE);
    h.feed(op::V2IP_BAY_MAPPINGS, &page(true, 1, &[owner]));
    let rec = crate::testing::stream_rec(owner, "239.0.0.5", "239.0.1.5", "", 5004);
    h.feed(op::SYS_BAY_V2IP_SOURCES, &rec);
    let ip = "239.0.0.5".parse().expect("an address");
    assert_eq!(
        h.state.bay_by_stream_ip(ip, false),
        Some(BayUid::new(owner, 0))
    );
}
