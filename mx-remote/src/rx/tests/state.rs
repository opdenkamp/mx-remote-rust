// Author: Lars Op den Kamp (lars@opdenkamp-it.nl)
// Copyright (c) 2026 Op den Kamp IT Solutions

//! Discovery and the state a device builds up as its configuration arrives.

use std::time::{Duration, Instant};

use crate::event::Event;
use crate::wire::{op, BayFeatures, BayStatus, BayUid, DeviceFeature, V2IP_PORT_VIDEO};

use crate::testing::{bay_config_rec, link_rec, poisoned, stream_rec, uid_n};

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

    // A V2IP device is not fully configured until its link configuration has
    // arrived.
    assert!(!h.complete());
    h.feed(op::SYS_LINKS, &link_rec(0, "AMP00001", "Zone 1", 0));
    assert!(h.complete());
    assert!(h.saw(|e| matches!(e, Event::DeviceConfigComplete { .. })));
}

/// A unit that announces a reboot is described again only once it has sent
/// its bays again.
#[test]
fn a_reboot_waits_for_the_bays_again() {
    let mut h = Harness::new(4);
    let plain = DeviceFeature::from_bits(0);
    h.hello(0x2F, "UNIT", "RB0001", plain);
    let bays = bay_config_rec(
        0,
        0,
        0,
        "Input 1",
        "",
        BayStatus::NONE,
        BayFeatures::HDMI_IN,
    );
    h.feed(op::SYS_BAY_CONFIG, &bays);
    assert!(h.complete());

    h.hello(0x2F, "UNIT", "RB0001", DeviceFeature::STATUS_REBOOT);
    h.hello(0x2F, "UNIT", "RB0001", plain);
    assert!(
        !h.complete(),
        "described by the bays it sent before rebooting"
    );
    h.feed(op::SYS_BAY_CONFIG, &bays);
    assert!(h.complete());

    // So is one that went offline.
    let later = Instant::now() + Duration::from_secs(120);
    let sender = h.sender;
    let device = h.state.device_mut(sender).expect("registered");
    device.check_online(later, &mut Vec::new());
    assert!(!device.online);
    h.hello(0x2F, "UNIT", "RB0001", plain);
    assert!(
        !h.complete(),
        "described by the bays it sent before going offline"
    );
    h.feed(op::SYS_BAY_CONFIG, &bays);
    assert!(h.complete());
}

/// A link moved to another bay of the same unit is a change.
#[test]
fn a_link_moved_to_another_bay_of_the_same_unit_is_reported() {
    let mut h = Harness::new(3);
    h.hello(0x2F, "FF88", "HD0004", DeviceFeature::VIDEO_ROUTING);
    h.feed(
        op::SYS_BAY_CONFIG,
        &bay_config_rec(
            0,
            1,
            0,
            "Output 1",
            "TV",
            BayStatus::NONE,
            BayFeatures::HDMI_OUT,
        ),
    );
    h.feed(op::SYS_LINKS, &link_rec(0, "AMP00001", "Zone 1", 0));
    h.events.clear();
    h.feed(op::SYS_LINKS, &link_rec(0, "AMP00001", "Zone 2", 0));
    assert!(h.saw(|e| matches!(e, Event::BayUnlinked { .. })));
    assert!(h.saw(|e| matches!(e, Event::BayLinked { .. })));

    h.events.clear();
    h.feed(op::SYS_LINKS, &link_rec(0, "AMP00001", "Zone 2", 0));
    assert!(h.events.is_empty(), "a repeat is not a change");
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

    // All ones mirrors nothing, as all zero does.
    mp[16..32].fill(0xFF);
    h.feed(op::BAY_MIRROR_STATUS, &mp[..31]);
    assert!(h.bay(0).mirror.is_mirroring(), "a short report was taken");
    h.feed(op::BAY_MIRROR_STATUS, &mp);
    assert!(!h.bay(0).mirror.is_mirroring(), "all ones read as a device");

    // Management sets another unit's mirror; another peer does not.
    let controller = uid_n(31);
    h.feed_as(
        controller,
        op::SYS_HELLO,
        &crate::testing::hello_payload(0x2F, "Ctrl", "CTRL0009", "4.8.0", DeviceFeature::MANAGER),
    );
    mp[16..32].copy_from_slice(master.as_bytes());
    h.feed_as(controller, op::BAY_MIRROR_STATUS, &mp);
    assert!(
        h.bay(0).mirror.is_mirroring(),
        "management's mirror was lost"
    );

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
        &hello_payload(0x28, "Ctrl", "SG0002", "4.8.0", DeviceFeature::MANAGER),
    );
    h.feed_as(stranger, op::V2IP_MANUAL_SRC_SWITCH, &switch);
    assert!(
        h.device().v2ip_sink.is_some(),
        "the hello exemption did not register the sender"
    );
}

/// Builds a device that advertises V2IP sources.
fn source_device(n: u8) -> Harness {
    let mut h = Harness::new(n);
    h.hello(
        0x27,
        "FF88",
        "PG0004",
        DeviceFeature::V2IP_SOURCE | DeviceFeature::V2IP_SINK,
    );
    h
}

/// One page of a source list: where its records start, how long the whole list
/// was when the page was built, then the records.
fn source_page(first: u16, total: u16, records: &[Vec<u8>]) -> Vec<u8> {
    let mut p = Vec::new();
    p.extend_from_slice(&first.to_le_bytes());
    p.extend_from_slice(&total.to_le_bytes());
    p.extend_from_slice(&[0; 4]);
    for r in records {
        p.extend_from_slice(r);
    }
    p
}

fn source_rec(n: u8) -> Vec<u8> {
    stream_rec(
        uid_n(n),
        &format!("239.9.{n}.1"),
        &format!("239.9.{n}.2"),
        &format!("239.9.{n}.3"),
        V2IP_PORT_VIDEO,
    )
}

/// A payload that is neither bare records nor a page is refused, not read from
/// the first byte.
///
/// Both known forms start their records at a fixed offset, so anything else is
/// a layout this client cannot place. Reading it from zero would not fail - it
/// would offset every record and report a full set of plausible addresses
/// belonging to no bay.
#[test]
fn a_source_list_of_an_unplaceable_length_is_refused() {
    let mut h = source_device(31);
    let mut odd = source_rec(41);
    odd.extend_from_slice(&[0; 4]);
    h.feed(op::SYS_BAY_V2IP_SOURCES, &odd);

    assert!(
        h.device().v2ip_sources.is_none(),
        "a record read at the wrong offset was reported as a source"
    );
}

/// Pages are placed by the position they state, not by the order they arrive.
///
/// Each page is its own datagram, so reordering and loss are both ordinary. A
/// record's position is what maps it to a bay, which is why it has to come from
/// the page header rather than from what has been seen so far.
#[test]
fn source_pages_are_placed_by_their_stated_position() {
    let mut h = source_device(32);

    // The second page first, which on its own describes nothing that can be
    // placed from the start of the list.
    h.feed(
        op::SYS_BAY_V2IP_SOURCES,
        &source_page(2, 4, &[source_rec(43), source_rec(44)]),
    );
    assert!(
        h.device().v2ip_sources.is_none(),
        "a list was reported with its first records still missing"
    );

    // The first page completes the run from the start.
    h.feed(
        op::SYS_BAY_V2IP_SOURCES,
        &source_page(0, 4, &[source_rec(41), source_rec(42)]),
    );
    let sources = h.device().v2ip_sources.clone().expect("no sources");
    assert_eq!(
        sources.iter().map(|s| s.uid).collect::<Vec<_>>(),
        vec![uid_n(41), uid_n(42), uid_n(43), uid_n(44)],
        "pages were ordered by arrival rather than by position"
    );
}

/// A page revises the records it carries and leaves the rest alone.
///
/// A page is a window on the sender's list, so treating one as the list would
/// leave a reader holding only that window - every bay outside it losing the
/// source it had.
#[test]
fn a_page_does_not_replace_the_list_it_is_part_of() {
    let mut h = source_device(33);
    h.feed(
        op::SYS_BAY_V2IP_SOURCES,
        &[source_rec(41), source_rec(42), source_rec(43)].concat(),
    );
    assert_eq!(h.device().v2ip_sources.as_ref().map(Vec::len), Some(3));

    // One page revising the middle record only.
    h.feed(
        op::SYS_BAY_V2IP_SOURCES,
        &source_page(1, 3, &[source_rec(52)]),
    );
    let sources = h.device().v2ip_sources.clone().expect("the list was lost");
    assert_eq!(
        sources.iter().map(|s| s.uid).collect::<Vec<_>>(),
        vec![uid_n(41), uid_n(52), uid_n(43)],
        "a page was read as the whole list"
    );
}

/// A device is not fully described until it has sent its bay configuration.
///
/// Nothing on the wire says how many bays to expect - not the hello, not the
/// records, which carry neither an index nor a total - so what can be
/// established is that the configuration was sent, and nothing about it can be
/// concluded from a hello alone.
#[test]
fn a_device_that_has_not_sent_its_bays_is_not_fully_described() {
    let mut h = Harness::new(34);
    h.hello(0x28, "FF88", "PG0005", DeviceFeature::VIDEO_ROUTING);

    assert!(
        !h.complete(),
        "a device was fully described on its hello alone"
    );
    assert!(
        !h.saw(|e| matches!(e, Event::DeviceConfigComplete { .. })),
        "completion was announced before anything was known"
    );

    // The secondary list is not that configuration: a device sends the primary
    // one whatever else it sends, so having only this is not having described
    // itself.
    h.feed(
        op::SYS_BAY_CONFIG_SECONDARY,
        &bay_config_rec(
            2,
            1,
            0,
            "Output 2",
            "TV",
            BayStatus::NONE,
            BayFeatures::HDMI_OUT,
        ),
    );
    // With its links in too, the only thing still outstanding is the primary
    // list - which is what makes this assertion about that list and not about
    // something else still missing.
    h.feed(op::SYS_LINKS, &link_rec(2, "AMP00001", "Zone 2", 0));
    assert!(
        !h.complete(),
        "the secondary list was taken for the configuration"
    );

    // The primary list, which is what was missing.
    h.feed(
        op::SYS_BAY_CONFIG,
        &bay_config_rec(
            1,
            1,
            0,
            "Output 1",
            "TV",
            BayStatus::NONE,
            BayFeatures::HDMI_OUT,
        ),
    );
    assert!(h.complete());
    assert!(h.saw(|e| matches!(e, Event::DeviceConfigComplete { .. })));
}

/// A unit takes a write about itself from the controller it follows, not
/// from another unit claiming to control a mesh.
#[test]
fn management_is_the_controller_a_unit_follows() {
    use crate::testing::hello_payload;

    let mut h = Harness::new(5);
    h.hello(0x2F, "ONEIP", "MG0001", DeviceFeature::V2IP_SINK);
    h.feed(
        op::SYS_BAY_CONFIG,
        &bay_config_rec(
            0,
            1,
            0,
            "Output 1",
            "TV",
            BayStatus::NONE,
            BayFeatures::HDMI_OUT,
        ),
    );
    let (followed, other) = (uid_n(40), uid_n(41));
    for (uid, features) in [
        (followed, DeviceFeature::from_bits(0)),
        (other, DeviceFeature::MESH_MASTER),
    ] {
        h.feed_as(
            uid,
            op::SYS_HELLO,
            &hello_payload(0x2F, "ONEIP", "MG0002", "4.8.0", features),
        );
    }
    let unit = h.sender;
    let hide = |hidden: u8| {
        let mut p = unit.as_bytes().to_vec();
        p.extend_from_slice(&[0, 0, hidden, 0, 0, 0, 0, 0]);
        p
    };

    // Until the unit says whom it follows, a controller's own claim stands.
    h.feed_as(other, op::BAY_HIDE, &hide(1));
    assert_eq!(h.bay(0).hidden, Some(true));

    let mut mesh = poisoned(40);
    mesh[0] = 0xFF;
    mesh[4..20].copy_from_slice(followed.as_bytes());
    h.feed(op::MESH_OPERATION, &mesh);
    h.feed_as(other, op::BAY_HIDE, &hide(0));
    assert_eq!(h.bay(0).hidden, Some(true), "another mesh's controller");
    h.feed_as(followed, op::BAY_HIDE, &hide(0));
    assert_eq!(h.bay(0).hidden, Some(false), "the controller it follows");
}

/// A unit on released firmware, below protocol 0x2F, takes a command for
/// itself from any sender, so one from a peer that is not management is
/// recorded as taken.
#[test]
fn a_released_unit_takes_commands_from_anyone() {
    use crate::testing::hello_payload;

    let mut h = Harness::new(6);
    h.hello(0x27, "ONEIP", "RL0001", DeviceFeature::V2IP_SINK);
    h.feed(
        op::SYS_BAY_CONFIG,
        &bay_config_rec(
            0,
            1,
            0,
            "Output 1",
            "TV",
            BayStatus::NONE,
            BayFeatures::HDMI_OUT,
        ),
    );
    let peer = uid_n(42);
    h.feed_as(
        peer,
        op::SYS_HELLO,
        &hello_payload(
            0x27,
            "ONEIP",
            "RL0002",
            "4.4.117",
            DeviceFeature::V2IP_SOURCE,
        ),
    );
    let mut hide = h.sender.as_bytes().to_vec();
    hide.extend_from_slice(&[0, 0, 1, 0, 0, 0, 0, 0]);
    h.feed_as(peer, op::BAY_HIDE, &hide);
    assert_eq!(h.bay(0).hidden, Some(true));

    let unit = h.sender;
    let addressed = |len: usize| {
        let mut p = vec![0u8; len];
        p[0..16].copy_from_slice(unit.as_bytes());
        p
    };
    let mut tiling = addressed(24);
    tiling[20..22].copy_from_slice(&1920u16.to_le_bytes());
    h.feed_as(peer, op::V2IP_TILING, &tiling);
    assert!(h.device().tiling.is_some(), "a window");

    let mut switch = addressed(48);
    switch[16..20].copy_from_slice(&[239, 1, 1, 1]);
    h.feed_as(peer, op::V2IP_MANUAL_SRC_SWITCH, &switch);
    assert!(h.device().v2ip_sink.is_some(), "a manual switch");

    let mut mirror = addressed(32);
    mirror[16..32].copy_from_slice(uid_n(43).as_bytes());
    h.feed_as(peer, op::BAY_MIRROR_STATUS, &mirror);
    assert!(h.bay(0).mirror.is_mirroring(), "a mirror");

    let mut filter = addressed(32);
    filter[16..32].copy_from_slice(uid_n(44).as_bytes());
    h.feed_as(peer, op::BAY_FILTER_STATUS, &filter);
    assert_eq!(h.bay(0).filtered, [uid_n(44)], "a filter");

    // A source the switch can name by its stream.
    let source = uid_n(45);
    h.feed_as(
        source,
        op::SYS_HELLO,
        &hello_payload(
            0x27,
            "ONEIP",
            "RL0003",
            "4.4.117",
            DeviceFeature::V2IP_SOURCE,
        ),
    );
    h.feed_as(
        source,
        op::SYS_BAY_CONFIG,
        &bay_config_rec(
            0,
            0,
            0,
            "Input 1",
            "Sky",
            BayStatus::NONE,
            BayFeatures::HDMI_IN,
        ),
    );
    h.feed_as(
        source,
        op::SYS_BAY_V2IP_SOURCES,
        &stream_rec(
            source,
            "239.4.4.1",
            "239.4.4.2",
            "239.4.4.3",
            V2IP_PORT_VIDEO,
        ),
    );
    let mut route = addressed(24);
    route[16..20].copy_from_slice(&[239, 4, 4, 1]);
    h.feed_as(peer, op::V2IP_SOURCE_SWITCH, &route);
    assert_eq!(
        h.bay(0).video_source.map(|b| b.device),
        Some(source),
        "a source switch"
    );

    let mut view = addressed(25);
    view[16] = 1; // a view mode command
    h.feed_as(peer, op::V2IP_MULTIVIEWER, &view);
    assert!(
        h.saw(|e| matches!(e, Event::MultiviewerCommand { .. })),
        "a multiviewer command"
    );

    let mut mute = vec![1u8, 0, 0, 0];
    mute.extend_from_slice(unit.as_bytes());
    mute.extend_from_slice(&[2, 0, 0, 0, 1, 0, 0, 0]);
    h.feed_as(peer, op::V2IP_AUDIO, &mute);
    assert!(
        h.saw(|e| matches!(e, Event::AudioEndpointMute { .. })),
        "an audio command"
    );
}
