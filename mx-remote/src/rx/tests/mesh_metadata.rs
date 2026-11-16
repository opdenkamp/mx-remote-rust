// Author: Lars Op den Kamp (lars@opdenkamp-it.nl)
// Copyright (c) 2026 Op den Kamp IT Solutions

//! `MESH_CONFIG` (0x57) kinds 6 to 8, port metadata: what a receiver takes,
//! from whom, and what it sends back.
//!
//! The frames are composed here from the layout the handlers decode, byte by
//! byte rather than with the builders, so they rule out a slip and a builder
//! and decoder agreeing on one; they are not evidence against a shared
//! misreading of the layout, which only a captured frame can be.

use std::time::{Duration, Instant};

use super::*;
use crate::types::{MeshPort, PortMetadata, PortMetadataField};
use crate::wire::{crc32, op};

const OWN: u8 = 0xFE;

/// A `MESH_CONFIG` header, its reserved bytes poisoned.
fn header(kind: u8, flags: u8, offset: u16, total: u16, crc: u32, subject: DeviceUid) -> Vec<u8> {
    let mut p = vec![1, kind, flags, 0xA5];
    p.extend_from_slice(&offset.to_le_bytes());
    p.extend_from_slice(&total.to_le_bytes());
    p.extend_from_slice(&crc.to_le_bytes());
    p.extend_from_slice(&[0x5A, 0xC3, 0x3C, 0xA5]);
    p.extend_from_slice(subject.as_bytes());
    p
}

fn metadata_frame(port: MeshPort, body: &[u8]) -> Vec<u8> {
    let crc = if body.is_empty() { 0 } else { crc32(body) };
    let flags = if port.input { 0x04 } else { 0 } | 0x01;
    let mut p = header(6, flags, 0, body.len() as u16, crc, port.unit);
    p.extend_from_slice(body);
    p
}

/// An index entry, its reserved bytes zero as a unit sends them.
fn entry(port: MeshPort, crc: u32) -> Vec<u8> {
    let mut e = port.unit.as_bytes().to_vec();
    e.extend_from_slice(&[u8::from(port.input), 0, 0, 0]);
    e.extend_from_slice(&crc.to_le_bytes());
    e
}

/// The frames of an index from `from`, `per` entries to a frame.
fn index_frames(
    from: DeviceUid,
    generation: u32,
    entries: &[(MeshPort, u32)],
    per: usize,
) -> Vec<Vec<u8>> {
    let all: Vec<u8> = entries.iter().flat_map(|(p, c)| entry(*p, *c)).collect();
    let crc = if all.is_empty() { 0 } else { crc32(&all) };
    let chunks: Vec<&[(MeshPort, u32)]> = if entries.is_empty() {
        vec![&[]]
    } else {
        entries.chunks(per).collect()
    };
    let mut first = 0u16;
    chunks
        .into_iter()
        .map(|chunk| {
            let mut p = header(7, 0, first, entries.len() as u16, crc, from);
            p.extend_from_slice(&generation.to_le_bytes());
            for (port, c) in chunk {
                p.extend(entry(*port, *c));
            }
            first += chunk.len() as u16;
            p
        })
        .collect()
}

const TV: &[u8] = b"device_type\0Sony PlayStation 4\0logo\0http://x/l.png\0";

fn tv_fields() -> Vec<PortMetadataField> {
    vec![
        PortMetadataField::new("device_type", "Sony PlayStation 4"),
        PortMetadataField::new("logo", "http://x/l.png"),
    ]
}

/// A client in the mesh of controller `uid_n(0xC1)`, which is registered.
fn member() -> (Harness, DeviceUid) {
    let mut h = Harness::new(0x21);
    h.hello(0x33, "Unit", "P9SN00000021", DeviceFeature::V2IP_SINK);
    let controller = h.controller();
    h.state.mesh_controller = controller;
    (h, controller)
}

/// A client controlling its own mesh, with member `uid_n(0x21)`.
fn controller() -> (Harness, DeviceUid) {
    let mut h = Harness::new(0x21);
    h.hello(0x33, "Unit", "P9SN00000021", DeviceFeature::V2IP_SINK);
    h.state.mesh_controller = uid_n(OWN);
    let member = h.sender;
    h.state
        .device_mut(member)
        .unwrap()
        .set_mesh_master(uid_n(OWN), false, &mut Vec::new());
    (h, member)
}

fn outbox(h: &mut Harness) -> Vec<(Option<DeviceUid>, Vec<u8>)> {
    std::mem::take(&mut h.state.metadata.outbox)
}

fn changes(h: &Harness) -> Vec<PortMetadata> {
    h.events
        .iter()
        .filter_map(|e| match e {
            Event::PortMetadataChanged { metadata, .. } => Some(metadata.clone()),
            _ => None,
        })
        .collect()
}

#[test]
fn metadata_from_the_controller_is_taken_in_order() {
    let (mut h, controller) = member();
    let port = MeshPort::input(uid_n(0x33));
    h.feed_as(controller, op::MESH_CONFIG, &metadata_frame(port, TV));
    assert_eq!(h.state.metadata.get(port), Some(tv_fields()));
    assert_eq!(
        h.state.metadata.get(MeshPort::output(uid_n(0x33))),
        None,
        "the INPUT flag was not read"
    );
    assert_eq!(
        changes(&h),
        [PortMetadata {
            port,
            fields: tv_fields()
        }]
    );
    // The same again changes nothing.
    h.feed_as(controller, op::MESH_CONFIG, &metadata_frame(port, TV));
    assert_eq!(changes(&h).len(), 1);
    // An empty one removes it.
    h.feed_as(controller, op::MESH_CONFIG, &metadata_frame(port, b""));
    assert_eq!(h.state.metadata.get(port), None);
    assert_eq!(changes(&h)[1].fields, []);
}

#[test]
fn metadata_from_any_other_sender_is_dropped() {
    let (mut h, _) = member();
    let port = MeshPort::output(uid_n(0x33));
    h.feed(op::MESH_CONFIG, &metadata_frame(port, TV));
    assert_eq!(h.state.metadata.get(port), None);
    // Nor does a client in no mesh take it unasked.
    h.state.mesh_controller = DeviceUid::ZERO;
    h.feed_as(uid_n(0xC1), op::MESH_CONFIG, &metadata_frame(port, TV));
    assert_eq!(h.state.metadata.get(port), None);
}

#[test]
fn metadata_a_unit_refuses_is_dropped() {
    let (mut h, controller) = member();
    let port = MeshPort::input(uid_n(0x33));
    let mut bad_len = metadata_frame(port, TV);
    bad_len[6] += 1;
    let mut bad_crc = metadata_frame(port, TV);
    bad_crc[8] ^= 1;
    let unterminated = metadata_frame(port, b"device_type\0PS4");
    let bad_key = metadata_frame(port, b"device type\0PS4\0");
    let quote = metadata_frame(port, b"k\0a\"b\0");
    let empty_value = metadata_frame(port, b"k\0\0");
    let control = metadata_frame(port, b"k\0a\x01b\0");
    let long_key = metadata_frame(port, &[&[b'k'; 32][..], b"\0v\0"].concat());
    for frame in [
        bad_len,
        bad_crc,
        unterminated,
        bad_key,
        quote,
        empty_value,
        control,
        long_key,
    ] {
        h.feed_as(controller, op::MESH_CONFIG, &frame);
    }
    assert_eq!(h.state.metadata.get(port), None);
    // 31 characters, a value of 255 bytes, and a key twice are taken.
    let edge = [
        &[b'k'; 31][..],
        b"\0",
        &[b'v'; 255][..],
        b"\0k\0one\0k\0two\0",
    ]
    .concat();
    h.feed_as(controller, op::MESH_CONFIG, &metadata_frame(port, &edge));
    assert_eq!(h.state.metadata.get(port).map(|f| f.len()), Some(3));
}

#[test]
fn a_request_is_answered_with_what_is_held() {
    let (mut h, controller) = member();
    let port = MeshPort::input(uid_n(0x33));
    h.feed_as(controller, op::MESH_CONFIG, &metadata_frame(port, TV));
    outbox(&mut h);
    h.feed(op::MESH_CONFIG, &header(8, 0x04, 0, 0, 0, port.unit));
    h.feed(op::MESH_CONFIG, &header(8, 0x00, 0, 0, 0, port.unit));
    let sent = outbox(&mut h);
    assert_eq!(sent.len(), 2);
    let (to, held) = &sent[0];
    assert_eq!(*to, Some(h.sender), "answered to someone else");
    assert_eq!(&held[..3], &[1, 6, 0x04]);
    assert_eq!(&held[4..6], &[0, 0], "the offset");
    assert_eq!(&held[6..8], &(TV.len() as u16).to_le_bytes());
    assert_eq!(&held[8..12], &crc32(TV).to_le_bytes());
    assert_eq!(&held[16..32], port.unit.as_bytes());
    assert_eq!(&held[32..], TV);
    let (_, none) = &sent[1];
    assert_eq!(none.len(), 32, "the output holds nothing");
    assert_eq!(&none[..3], &[1, 6, 0]);
    assert_eq!(&none[6..12], &[0; 6], "no length or crc");
}

#[test]
fn a_member_takes_its_controllers_index_and_asks_for_what_differs() {
    let (mut h, controller) = member();
    let same = MeshPort::input(uid_n(0x31));
    let changed = MeshPort::output(uid_n(0x32));
    let gone = MeshPort::input(uid_n(0x33));
    for port in [same, changed, gone] {
        h.feed_as(controller, op::MESH_CONFIG, &metadata_frame(port, TV));
    }
    outbox(&mut h);
    h.events.clear();
    let mut entries = vec![(same, crc32(TV)), (changed, 0x1234_5678)];
    // Enough more to need a second frame of 48.
    for n in 0x80..0xBCu8 {
        entries.push((MeshPort::input(uid_n(n)), 0x0101_0101 * u32::from(n)));
    }
    let frames = index_frames(controller, 0x0102_0304, &entries, 48);
    assert_eq!(frames.len(), 2);
    h.feed_as(controller, op::MESH_CONFIG, &frames[0]);
    assert_eq!(
        h.state.metadata.get(gone),
        Some(tv_fields()),
        "dropped before the index was whole"
    );
    h.feed_as(controller, op::MESH_CONFIG, &frames[1]);
    let asked: Vec<_> = outbox(&mut h);
    assert_eq!(
        asked.len(),
        61,
        "asked for other than the ports that differ"
    );
    assert!(asked.iter().all(|(to, _)| *to == Some(controller)));
    let (_, first) = &asked[0];
    assert_eq!(first.len(), 32);
    assert_eq!(&first[..3], &[1, 8, 0], "an output's request");
    assert_eq!(&first[16..32], changed.unit.as_bytes());
    assert_eq!(&asked[1].1[..3], &[1, 8, 0x04], "an input's request");
    assert_eq!(h.state.metadata.get(gone), None, "an unnamed port kept");
    assert_eq!(h.state.metadata.get(same), Some(tv_fields()));
    assert_eq!(h.state.metadata.generation, 0x0102_0304);
    assert!(h.events.iter().any(|e| matches!(
        e,
        Event::PortMetadataGenerationChanged {
            generation: 0x0102_0304,
            ..
        }
    )));
}

#[test]
fn an_answer_is_taken_from_the_index_sender_for_30_s() {
    let (mut h, controller) = controller();
    let member = h.sender;
    let port = MeshPort::input(uid_n(0x40));
    let start = Instant::now();
    let frames = index_frames(member, 1, &[(port, crc32(TV))], 48);
    h.feed_full(member, op::MESH_CONFIG, PROTOCOL_VERSION, &frames[0], start);
    assert_eq!(outbox(&mut h).len(), 1, "the port not asked for");
    let answer = metadata_frame(port, TV);
    let late = start + Duration::from_secs(30);
    h.feed_full(member, op::MESH_CONFIG, PROTOCOL_VERSION, &answer, late);
    assert_eq!(h.state.metadata.get(port), None, "taken after the window");
    let within = start + Duration::from_millis(29_900);
    h.feed_full(member, op::MESH_CONFIG, PROTOCOL_VERSION, &answer, within);
    assert_eq!(h.state.metadata.get(port), Some(tv_fields()));
    let _ = controller;
}

#[test]
fn a_member_answers_an_older_index_with_its_own_once_a_minute() {
    let (mut h, controller) = member();
    let port = MeshPort::output(uid_n(0x33));
    h.feed_as(controller, op::MESH_CONFIG, &metadata_frame(port, TV));
    h.state.metadata.generation = 9;
    outbox(&mut h);
    let start = Instant::now();
    let older = index_frames(controller, 8, &[], 48);
    h.feed_full(
        controller,
        op::MESH_CONFIG,
        PROTOCOL_VERSION,
        &older[0],
        start,
    );
    assert_eq!(h.state.metadata.get(port), Some(tv_fields()), "taken");
    let sent = outbox(&mut h);
    assert_eq!(sent.len(), 1);
    let (to, own) = &sent[0];
    assert_eq!(*to, Some(controller));
    assert_eq!(&own[..3], &[1, 7, 0]);
    assert_eq!(&own[4..6], &[0, 0], "the first entry");
    assert_eq!(&own[6..8], &[1, 0], "the entry count");
    let e = entry(port, crc32(TV));
    assert_eq!(&own[8..12], &crc32(&e).to_le_bytes());
    assert_eq!(&own[12..16], &[0; 4], "the reserved bytes");
    assert_eq!(&own[16..32], uid_n(OWN).as_bytes(), "the sender");
    assert_eq!(&own[32..36], &9u32.to_le_bytes());
    assert_eq!(&own[36..], &e[..]);
    let again = start + Duration::from_secs(59);
    h.feed_full(
        controller,
        op::MESH_CONFIG,
        PROTOCOL_VERSION,
        &older[0],
        again,
    );
    assert!(outbox(&mut h).is_empty(), "offered twice within 60 s");
    let later = start + Duration::from_secs(60);
    h.feed_full(
        controller,
        op::MESH_CONFIG,
        PROTOCOL_VERSION,
        &older[0],
        later,
    );
    assert_eq!(outbox(&mut h).len(), 1);
    // The same generation is taken.
    let same = index_frames(controller, 9, &[], 48);
    h.feed_as(controller, op::MESH_CONFIG, &same[0]);
    assert_eq!(h.state.metadata.get(port), None);
}

#[test]
fn a_member_takes_no_index_but_its_controllers() {
    let (mut h, _) = member();
    let port = MeshPort::output(uid_n(0x33));
    let frames = index_frames(h.sender, 5, &[(port, 1)], 48);
    h.feed(op::MESH_CONFIG, &frames[0]);
    assert!(outbox(&mut h).is_empty());
    assert_eq!(h.state.metadata.generation, 0);
}

#[test]
fn a_controller_takes_only_a_newer_index_and_only_from_a_member() {
    let (mut h, member) = controller();
    h.state.metadata.generation = 4;
    let port = MeshPort::output(uid_n(0x33));
    let same = index_frames(member, 4, &[(port, 1)], 48);
    h.feed_as(member, op::MESH_CONFIG, &same[0]);
    assert!(outbox(&mut h).is_empty(), "took the same generation");
    let newer = index_frames(member, 5, &[(port, 1)], 48);
    h.state
        .device_mut(member)
        .unwrap()
        .set_mesh_master(uid_n(0x77), false, &mut Vec::new());
    h.feed_as(member, op::MESH_CONFIG, &newer[0]);
    assert!(outbox(&mut h).is_empty(), "took a non-member's");
    h.state
        .device_mut(member)
        .unwrap()
        .set_mesh_master(uid_n(OWN), false, &mut Vec::new());
    h.feed_as(member, op::MESH_CONFIG, &newer[0]);
    assert_eq!(outbox(&mut h).len(), 1);
    assert_eq!(h.state.metadata.generation, 5);
}

#[test]
fn a_later_frame_must_follow_on() {
    let (mut h, controller) = member();
    let entries: Vec<_> = (0..4u8)
        .map(|n| (MeshPort::input(uid_n(n)), 7 + u32::from(n)))
        .collect();
    let frames = index_frames(controller, 3, &entries, 2);
    let mut wrong_crc = frames[1].clone();
    wrong_crc[8] ^= 1;
    let mut wrong_offset = frames[1].clone();
    wrong_offset[4] = 1;
    let mut part_entry = frames[1].clone();
    part_entry.pop();
    h.feed_as(controller, op::MESH_CONFIG, &frames[0]);
    assert_eq!(outbox(&mut h).len(), 2);
    for frame in [&wrong_crc, &wrong_offset, &part_entry] {
        h.feed_as(controller, op::MESH_CONFIG, frame);
    }
    assert!(outbox(&mut h).is_empty(), "took a frame out of step");
    h.feed_as(uid_n(0x21), op::MESH_CONFIG, &frames[1]);
    assert!(outbox(&mut h).is_empty(), "took another sender's frame");
    assert_eq!(h.state.metadata.generation, 0);
    h.feed_as(controller, op::MESH_CONFIG, &frames[1]);
    assert_eq!(outbox(&mut h).len(), 2);
    assert_eq!(h.state.metadata.generation, 3);
}

#[test]
fn the_controllers_api_change_goes_out_with_its_index_a_second_later() {
    let (mut h, _) = controller();
    let port = MeshPort::input(uid_n(0x33));
    let now = Instant::now();
    let mut ev = Vec::new();
    h.state
        .set_port_metadata(port, tv_fields(), now, &mut ev)
        .expect("refused");
    assert_eq!(h.state.metadata.generation, 1);
    let sent = outbox(&mut h);
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].0, None, "not sent to every unit");
    assert_eq!(&sent[0].1[32..], TV);
    let period = Duration::from_secs(50);
    h.state
        .check_metadata_index(now + Duration::from_millis(900), period);
    assert!(outbox(&mut h).is_empty(), "the index went early");
    h.state
        .check_metadata_index(now + Duration::from_secs(1), period);
    let index = outbox(&mut h);
    assert_eq!(index.len(), 1);
    assert_eq!(&index[0].1[..2], &[1, 7]);
    assert_eq!(&index[0].1[32..36], &1u32.to_le_bytes());
    // Setting the same again changes nothing and raises nothing.
    h.state
        .set_port_metadata(port, tv_fields(), now, &mut ev)
        .expect("refused");
    assert_eq!(h.state.metadata.generation, 1);
    assert!(outbox(&mut h).is_empty());
    // A key twice is refused by the API.
    let twice = vec![
        PortMetadataField::new("k", "a"),
        PortMetadataField::new("k", "b"),
    ];
    assert!(h
        .state
        .set_port_metadata(port, twice, now, &mut ev)
        .is_err());
}
