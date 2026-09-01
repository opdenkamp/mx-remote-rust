// Author: Lars Op den Kamp (lars@opdenkamp-it.nl)
// Copyright (c) 2026 Op den Kamp IT Solutions

//! Discovery and the state a device builds up as its configuration arrives.

use crate::event::Event;
use crate::wire::{op, BayFeatures, BayStatus, BayUid, DeviceFeature, V2IP_PORT_VIDEO};

use crate::testing::{bay_config_rec, poisoned, stream_rec, uid_n};

use super::Harness;

#[test]
fn discovery_builds_a_device_from_its_configuration() {
    let mut h = Harness::new(1);
    let sender = h.sender;
    let source = uid_n(9);

    h.hello(
        0x27,
        "FF88",
        "AB1234",
        DeviceFeature::V2IP_SOURCE | DeviceFeature::V2IP_SINK,
    );
    assert_eq!(h.device().serial(), "AB1234");
    assert!(h.device().is_v2ip());
    assert_eq!(
        h.device().hello.address,
        Some(std::net::Ipv4Addr::new(10, 8, 8, 9))
    );

    let mut cfg = bay_config_rec(
        0,
        0,
        0,
        "Input 1",
        "Apple TV",
        BayStatus::NONE,
        BayFeatures::V2IP_SOURCE_LOCAL,
    );
    cfg.extend(bay_config_rec(
        1,
        1,
        0,
        "Output 1",
        "Living Room",
        BayStatus::NONE,
        BayFeatures::V2IP_SINK_LOCAL,
    ));
    h.feed(op::SYS_BAY_CONFIG, &cfg);

    assert_eq!(h.device().bays.len(), 2);
    let input = h.bay(0);
    assert!(input.is_input() && input.is_v2ip_source());
    assert_eq!(input.user_name(), "Apple TV");

    // Two records, so a stride read at anything but the record width shifts
    // the second one's fields.
    let mut sources = stream_rec(
        source,
        "239.1.1.1",
        "239.1.1.2",
        "239.1.1.3",
        V2IP_PORT_VIDEO,
    );
    sources.extend(stream_rec(
        uid_n(10),
        "239.2.2.1",
        "239.2.2.2",
        "239.2.2.3",
        V2IP_PORT_VIDEO,
    ));
    h.feed(op::SYS_BAY_V2IP_SOURCES, &sources);

    let advertised = h
        .device()
        .v2ip_sources
        .clone()
        .expect("no stream addresses");
    assert_eq!(advertised.len(), 2);
    assert_eq!(advertised[1].uid, uid_n(10));
    assert_eq!(
        advertised[1].video.ip,
        std::net::Ipv4Addr::new(239, 2, 2, 1)
    );
    assert_eq!(advertised[1].audio.port, V2IP_PORT_VIDEO);

    let device = h.device();
    let streams = device
        .v2ip_source_for(device.bay(0).expect("input bay"))
        .expect("no stream addresses");
    assert_eq!(streams.video.ip, std::net::Ipv4Addr::new(239, 1, 1, 1));
    assert_eq!(streams.audio.ip, std::net::Ipv4Addr::new(239, 1, 1, 2));

    // A V2IP source is keyed by the device producing the stream, not by the
    // port it happens to be mapped to.
    assert_eq!(
        h.state.link_key(BayUid::new(sender, 0)),
        Some(BayUid::new(source, 0))
    );

    // A V2IP device is not fully configured until its links have arrived.
    assert!(!h.device().configuration_complete());
    h.feed(op::SYS_LINKS, &[]);
    assert!(h.device().configuration_complete());
    assert!(h.saw(|e| matches!(e, Event::DeviceConfigComplete { .. })));
}

#[test]
fn mirror_and_mesh_reports() {
    let mut h = Harness::new(2);
    let sender = h.sender;
    let master = uid_n(7);

    h.hello(
        0x27,
        "ONEIP",
        "RX0001",
        DeviceFeature::V2IP_SINK | DeviceFeature::MESH,
    );
    h.feed(
        op::SYS_BAY_CONFIG,
        &bay_config_rec(
            0,
            1,
            0,
            "Output 1",
            "TV",
            BayStatus::NONE,
            BayFeatures::V2IP_SINK_LOCAL,
        ),
    );

    // A mirror report names the reporting device at 0 and the device it
    // follows at 16.
    let mut mp = poisoned(32);
    mp[0..16].copy_from_slice(sender.as_bytes());
    mp[16..32].copy_from_slice(master.as_bytes());
    h.feed(op::BAY_MIRROR_STATUS, &mp);

    let mirror = h.bay(0).mirror;
    assert!(mirror.is_mirroring());
    assert_eq!(mirror.target.map(|b| b.device), Some(master));
    assert!(h.saw(|e| matches!(e, Event::MirrorStatusChanged { .. })));

    // Mesh membership: the sub-opcode at 0, the master uid at 4. The struct is
    // 8-aligned, so it is 40 bytes rather than the 36 its fields occupy.
    let mut mesh = poisoned(40);
    mesh[0] = 0xFF;
    mesh[4..20].copy_from_slice(master.as_bytes());
    h.feed(op::MESH_OPERATION, &mesh);
    assert_eq!(h.device().mesh_master, master);
    assert!(h.saw(|e| matches!(e, Event::MeshMasterChanged { .. })));
}

/// A mesh operation is read only where the mesh itself acts on one.
///
/// The opcode's receiver drops a frame short of the 8-aligned struct, and drops
/// one stamped below 0x1A whatever its length. Reading either anyway would take
/// a mesh master from a frame no device on the network acted on - and 0x1A is
/// not this opcode's table entry, which is 0x1D for a parameter its
/// report-controller operation grew later.
#[test]
fn a_mesh_report_below_its_own_gate_is_not_a_master() {
    let mut h = Harness::new(3);
    let master = uid_n(77);
    h.hello(
        0x28,
        "ONEIP",
        "MG0001",
        DeviceFeature::V2IP_SINK | DeviceFeature::MESH,
    );

    let mut mesh = poisoned(40);
    mesh[0] = 0xFF;
    mesh[4..20].copy_from_slice(master.as_bytes());

    // Short of the struct: the fields end at 36 and the struct does not.
    h.feed(op::MESH_OPERATION, &mesh[..36]);
    assert_eq!(
        h.device().mesh_master,
        crate::wire::DeviceUid::ZERO,
        "a frame short of the struct named a master"
    );

    // Long enough, but stamped a version below the receiver's accept gate.
    h.feed_proto(op::MESH_OPERATION, 0x19, &mesh);
    assert_eq!(
        h.device().mesh_master,
        crate::wire::DeviceUid::ZERO,
        "a frame stamped below the accept gate named a master"
    );

    // The gate's own version is enough; nothing here needs the table entry.
    h.feed_proto(op::MESH_OPERATION, 0x1A, &mesh);
    assert_eq!(h.device().mesh_master, master);
}

/// A frame from a device this client has never heard of is not acted on.
///
/// The addressee drops it too - a device processes nothing from a uid it has no
/// record of - so applying it would leave this client holding a route the
/// device it names never took. Hello and discover are exempt, because they are
/// what ends the condition: a hello is how a sender stops being unknown, and
/// without that exemption nothing could ever become known.
#[test]
fn a_frame_from_an_unknown_sender_is_not_acted_on() {
    use crate::testing::hello_payload;

    let mut h = Harness::new(12);
    let sink = h.sender;
    let stranger = uid_n(99);
    h.hello(
        0x28,
        "ONEIP",
        "SG0001",
        DeviceFeature::V2IP_SINK | DeviceFeature::MESH,
    );

    // 0x24 addressed at the known sink, sent by a device that has not
    // announced itself. Every device on the mesh applies an observed switch to
    // its record of the addressee - but only when it knows the sender.
    let mut switch = poisoned(40);
    switch[0..16].copy_from_slice(sink.as_bytes());
    switch[16..20].copy_from_slice(&[239, 1, 1, 1]);
    switch[24..28].copy_from_slice(&[239, 1, 1, 2]);
    h.feed_as(stranger, op::V2IP_MANUAL_SRC_SWITCH, &switch);
    assert!(
        h.device().v2ip_sink.is_none(),
        "a stranger's route request was applied to the sink"
    );

    // The same frame once its sender has introduced itself.
    h.feed_as(
        stranger,
        op::SYS_HELLO,
        &hello_payload(0x28, "ONEIP", "SG0002", "4.8.0", DeviceFeature::V2IP_SOURCE),
    );
    h.feed_as(stranger, op::V2IP_MANUAL_SRC_SWITCH, &switch);
    assert!(
        h.device().v2ip_sink.is_some(),
        "the hello exemption did not register the sender"
    );
}
