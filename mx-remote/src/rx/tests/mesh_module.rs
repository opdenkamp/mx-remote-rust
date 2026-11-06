// Author: Lars Op den Kamp (lars@opdenkamp-it.nl)
// Copyright (c) 2026 Op den Kamp IT Solutions

//! `MESH_REPLACE` (0x56) and `MESH_CONFIG` (0x57): what a receiver reads of
//! each kind, and the frames it drops.
//!
//! The frames are composed here from the layout the handlers decode, byte by
//! byte rather than with the builders, so they rule out a slip and a builder
//! and decoder agreeing on one; they are not evidence against a shared
//! misreading of the layout.

use std::time::{Duration, Instant};

use super::*;
use crate::types::{
    ControllerHello, ControllerPart, MeshBay, MeshBays, MeshConfig, MeshConfigDigest, MeshSetting,
};
use crate::wire::{op, DeviceFeature, MeshReplaceResult};

/// A `MESH_REPLACE` header, its reserved byte poisoned.
fn replace_header(kind: u8, first: u16, total: u16, flags: u8) -> Vec<u8> {
    let mut p = vec![1, kind];
    p.extend_from_slice(&first.to_le_bytes());
    p.extend_from_slice(&total.to_le_bytes());
    p.extend_from_slice(&[flags, 0xA5]);
    p
}

/// A list record whose every field is distinct from its neighbours, and wider
/// than a narrowed read could reproduce.
fn record(n: u8) -> Vec<u8> {
    let mut r = uid_n(n).as_bytes().to_vec();
    r.extend_from_slice(&(0x8040_0018u32 + u32::from(n)).to_le_bytes());
    r.extend_from_slice(&(0x0301u16 + u16::from(n)).to_le_bytes());
    r.extend_from_slice(&(0x0702u16 + u16::from(n)).to_le_bytes());
    r.extend_from_slice(b"Lounge TV input!");
    r.extend_from_slice(b"Bar\0\xA5\xA5\xA5\xA5\xA5\xA5\xA5\xA5\xA5\xA5\xA5\xA5");
    r
}

fn units_frame(
    first: u16,
    total: u16,
    flags: u8,
    records: impl IntoIterator<Item = u8>,
) -> Vec<u8> {
    let mut p = replace_header(1, first, total, flags);
    for n in records {
        p.extend(record(n));
    }
    p
}

fn kept(h: &Harness) -> Vec<DeviceUid> {
    h.device()
        .mesh_replace_units
        .as_ref()
        .map(|(list, _)| list.units.iter().map(|u| u.uid).collect())
        .unwrap_or_default()
}

fn mesh_unit(h: &mut Harness) {
    h.hello(0x33, "Unit", "P9SN12345678", DeviceFeature::V2IP_SINK);
}

#[test]
fn a_list_is_read_by_whole_records() {
    let mut h = Harness::new(0x31);
    mesh_unit(&mut h);
    let mut p = units_frame(0, 3, 1, [0x40, 0x41]);
    // Part of a third record, which no whole record covers.
    p.extend_from_slice(&record(0x42)[..30]);
    h.feed(op::MESH_REPLACE, &p);
    let (list, _) = h.device().mesh_replace_units.clone().expect("not kept");
    assert!(list.offline_controller);
    assert_eq!(list.total, 3);
    assert_eq!(list.units.len(), 2, "a part record was read");
    let u = &list.units[1];
    assert_eq!(u.uid, uid_n(0x41));
    assert_eq!(u.features.bits(), 0x8040_0059);
    assert_eq!(u.input, 0x0342);
    assert_eq!(u.output, 0x0743);
    assert_eq!(u.input_name, "Lounge TV input!", "an unterminated name");
    assert_eq!(u.output_name, "Bar");
    assert!(h.saw(|e| matches!(e, Event::MeshReplaceUnitsReceived { units, .. } if units == &list)));
}

#[test]
fn a_list_from_its_first_record_replaces_the_last() {
    let mut h = Harness::new(0x31);
    mesh_unit(&mut h);
    let t0 = Instant::now();
    h.feed_at(op::MESH_REPLACE, &units_frame(0, 3, 0, [1, 2]), t0);
    h.feed_at(op::MESH_REPLACE, &units_frame(2, 3, 0, [3]), t0);
    assert_eq!(
        kept(&h),
        [uid_n(1), uid_n(2), uid_n(3)],
        "a later frame added nothing"
    );
    h.feed_at(op::MESH_REPLACE, &units_frame(0, 1, 0, [4]), t0);
    assert_eq!(kept(&h), [uid_n(4)], "a new list kept the old one");
    h.feed_at(op::MESH_REPLACE, &units_frame(0, 0, 0, []), t0);
    assert_eq!(kept(&h), [], "an empty list kept the old one");

    // A later frame after the list has aged out starts a new one.
    h.feed_at(op::MESH_REPLACE, &units_frame(0, 2, 0, [5]), t0);
    let t1 = t0 + Duration::from_secs(31);
    h.feed_at(op::MESH_REPLACE, &units_frame(1, 2, 0, [6]), t1);
    assert_eq!(kept(&h), [uid_n(6)], "an aged list was added to");
}

#[test]
fn no_more_units_are_kept_than_a_controller_lists() {
    let mut h = Harness::new(0x31);
    mesh_unit(&mut h);
    let other = uid_n(0x32);
    h.feed_as(
        other,
        op::SYS_HELLO,
        &hello_payload(
            0x33,
            "Other",
            "P9SN87654321",
            "4.8.0",
            DeviceFeature::V2IP_SINK,
        ),
    );
    h.feed_as(other, op::MESH_REPLACE, &units_frame(0, 24, 0, 100..124));
    h.feed(op::MESH_REPLACE, &units_frame(0, 24, 0, 0..24));
    assert_eq!(kept(&h).len(), 8, "32 in all");
    // The sender's own earlier frame does not count against it again.
    h.feed(op::MESH_REPLACE, &units_frame(0, 5, 0, 0..5));
    assert_eq!(kept(&h).len(), 5);
}

#[test]
fn a_frame_of_another_version_or_too_short_is_dropped() {
    let mut h = Harness::new(0x31);
    mesh_unit(&mut h);
    let mut p = units_frame(0, 1, 0, [1]);
    p[0] = 2;
    h.feed(op::MESH_REPLACE, &p);
    assert!(
        h.device().mesh_replace_units.is_none(),
        "version 2 was read"
    );
    h.feed(op::MESH_REPLACE, &[1, 0, 0, 0, 0, 0, 0]);
    assert!(
        !h.saw(|e| matches!(e, Event::MeshReplaceQueried { .. })),
        "7 bytes"
    );
    h.feed(op::MESH_REPLACE, &replace_header(0, 0, 0, 0));
    assert!(h.saw(|e| matches!(e, Event::MeshReplaceQueried { device } if *device == h.sender)));

    let mut request = replace_header(2, 0, 0, 0);
    request.extend_from_slice(uid_n(9).as_bytes());
    request.extend_from_slice(&poisoned(7));
    h.feed(op::MESH_REPLACE, &request);
    assert!(
        !h.saw(|e| matches!(e, Event::MeshReplaceRequested { .. })),
        "31 bytes"
    );
    request.push(0);
    h.feed(op::MESH_REPLACE, &request);
    assert!(h.saw(|e| matches!(e, Event::MeshReplaceRequested { unit, .. } if *unit == uid_n(9))));
}

#[test]
fn an_answer_is_read_only_for_the_unit_asked_about() {
    let mut h = Harness::new(0x31);
    mesh_unit(&mut h);
    let result = |unit: DeviceUid, result: u8| {
        let mut p = replace_header(3, 0, 0, 0);
        p.extend_from_slice(unit.as_bytes());
        p.push(result);
        p.extend_from_slice(&poisoned(7));
        p
    };
    h.feed(op::MESH_REPLACE, &result(uid_n(9), 2));
    assert!(
        !h.saw(|e| matches!(e, Event::MeshReplaceAnswered { .. })),
        "unasked"
    );
    h.state.mesh_replace_pending = Some((h.sender, uid_n(9)));
    h.feed(op::MESH_REPLACE, &result(uid_n(8), 2));
    assert!(
        !h.saw(|e| matches!(e, Event::MeshReplaceAnswered { .. })),
        "another unit"
    );
    h.feed(op::MESH_REPLACE, &result(uid_n(9), 0x77));
    assert!(h.saw(|e| matches!(
        e,
        Event::MeshReplaceAnswered { unit, result, .. }
            if *unit == uid_n(9) && *result == MeshReplaceResult::from_wire(0x77)
    )));
    assert_eq!(h.state.mesh_replace_pending, None, "still waiting");
}

/// A `MESH_CONFIG` header, its reserved bytes poisoned.
fn config_header(
    kind: u8,
    flags: u8,
    offset: u16,
    total: u16,
    crc: u32,
    subject: DeviceUid,
) -> Vec<u8> {
    let mut p = vec![1, kind, flags, 0xA5];
    p.extend_from_slice(&offset.to_le_bytes());
    p.extend_from_slice(&total.to_le_bytes());
    p.extend_from_slice(&crc.to_le_bytes());
    p.extend_from_slice(&poisoned(4));
    p.extend_from_slice(subject.as_bytes());
    p
}

#[test]
fn a_hello_is_read_only_about_its_sender() {
    let mut h = Harness::new(0x31);
    mesh_unit(&mut h);
    h.feed(
        op::MESH_CONFIG,
        &config_header(0, 0, 0, 0x0602, 0x8899_AABB, uid_n(5)),
    );
    assert_eq!(h.device().mesh_config_digest, None, "another unit's");
    h.feed(
        op::MESH_CONFIG,
        &config_header(0, 0, 0, 0x0602, 0x8899_AABB, h.sender),
    );
    let digest = MeshConfigDigest {
        length: 0x0602,
        crc32: 0x8899_AABB,
        lost: false,
    };
    assert_eq!(h.device().mesh_config_digest, Some(digest));
    assert!(
        h.saw(|e| matches!(e, Event::MeshConfigDigestChanged { digest: d, .. } if *d == digest))
    );
    let mut short = config_header(0, 0, 0, 1, 1, h.sender);
    short.pop();
    h.feed(op::MESH_CONFIG, &short);
    assert_eq!(h.device().mesh_config_digest, Some(digest), "31 bytes");

    h.feed(op::MESH_CONFIG, &config_header(1, 0, 0, 0, 0, uid_n(5)));
    assert!(
        h.saw(|e| matches!(e, Event::MeshConfigRequested { subject, .. } if *subject == uid_n(5)))
    );
}

/// Three settings and their carried form, 293 bytes: one of a single
/// byte, one of 250 bytes and one empty.
fn settings() -> (Vec<MeshSetting>, Vec<u8>) {
    let long = "n".repeat(250);
    let list = vec![
        MeshSetting::new("led.status", "1"),
        MeshSetting::new("mbay.input1.name", long.clone()),
        MeshSetting::new("ir.profile", ""),
    ];
    let mut bytes = Vec::new();
    for s in &list {
        bytes.push(s.name.len() as u8);
        bytes.extend_from_slice(s.name.as_bytes());
        bytes.push(s.value.len() as u8);
        bytes.extend_from_slice(s.value.as_bytes());
    }
    (list, bytes)
}

/// Settings in frames of 100 bytes, three for [`settings`].
fn pages(kind: u8, flags: u8, subject: DeviceUid, bytes: &[u8], crc: u32) -> Vec<Vec<u8>> {
    bytes
        .chunks(100)
        .enumerate()
        .map(|(i, chunk)| {
            let mut p = config_header(
                kind,
                flags,
                (i * 100) as u16,
                bytes.len() as u16,
                crc,
                subject,
            );
            p.extend_from_slice(chunk);
            p
        })
        .collect()
}

#[test]
fn settings_are_taken_once_every_frame_has_arrived() {
    let mut h = Harness::new(0x31);
    mesh_unit(&mut h);
    let (list, bytes) = settings();
    let crc = crate::wire::crc32(&bytes);
    let frames = pages(2, 1, uid_n(7), &bytes, crc);
    assert!(frames.len() > 1);
    for (i, frame) in frames.iter().enumerate() {
        assert!(h.state.mesh_configs.is_empty(), "taken after {i} frames");
        h.feed(op::MESH_CONFIG, frame);
    }
    let expected = MeshConfig {
        subject: uid_n(7),
        holder: h.sender,
        copy: true,
        lost: false,
        settings: list,
    };
    assert_eq!(h.state.mesh_configs.get(&uid_n(7)), Some(&expected));
    assert!(h.saw(|e| matches!(e, Event::MeshConfigReceived { config, .. } if config == &expected)));
}

#[test]
fn settings_out_of_order_or_with_another_crc_are_dropped() {
    let (_, bytes) = settings();
    let crc = crate::wire::crc32(&bytes);
    let fed = |frames: Vec<Vec<u8>>| {
        let mut h = Harness::new(0x31);
        mesh_unit(&mut h);
        for frame in &frames {
            h.feed(op::MESH_CONFIG, frame);
        }
        h.state.mesh_configs.is_empty()
    };
    assert!(!fed(pages(2, 0, uid_n(7), &bytes, crc)), "in order");
    let mut swapped = pages(2, 0, uid_n(7), &bytes, crc);
    swapped.swap(1, 2);
    assert!(fed(swapped), "out of order");
    let mut repeated = pages(2, 0, uid_n(7), &bytes, crc);
    repeated.insert(2, repeated[1].clone());
    assert!(fed(repeated), "a frame twice");
    assert!(fed(pages(2, 0, uid_n(7), &bytes, crc ^ 1)), "another crc");
    let mut moved = pages(2, 0, uid_n(7), &bytes, crc);
    moved[1][16..32].copy_from_slice(uid_n(8).as_bytes());
    assert!(fed(moved), "another unit part way");
    let mut flagged = pages(2, 0, uid_n(7), &bytes, crc);
    flagged[1][2] = 1;
    assert!(fed(flagged), "other flags part way");
    let mut long = pages(2, 0, uid_n(7), &bytes, crc);
    long.last_mut().unwrap().push(0);
    assert!(fed(long), "past the length");

    // A frame naming the wrong offset, whose bytes would otherwise make
    // settings with the right crc.
    let mut skipped = pages(2, 0, uid_n(7), &bytes, crc);
    skipped[1][4..6].copy_from_slice(&150u16.to_le_bytes());
    assert!(fed(skipped), "the wrong offset");

    // Settings of 1985 bytes, which would decode and match their crc32, are
    // more than a unit takes.
    let pair = |n: usize| [vec![0, n as u8], vec![b'v'; n]].concat();
    let fits: Vec<u8> = pair(246).repeat(8);
    assert_eq!(fits.len(), 1984);
    assert!(
        !fed(pages(2, 0, uid_n(7), &fits, crate::wire::crc32(&fits))),
        "1984 bytes"
    );
    let over = [pair(246).repeat(7), pair(247)].concat();
    assert!(
        fed(pages(2, 0, uid_n(7), &over, crate::wire::crc32(&over))),
        "1985 bytes"
    );
}

#[test]
fn a_config_frame_of_another_version_or_too_short_is_dropped() {
    let mut h = Harness::new(0x31);
    mesh_unit(&mut h);
    let mut hello = config_header(0, 0, 0, 0x0602, 0x8899_AABB, h.sender);
    hello[0] = 2;
    h.feed(op::MESH_CONFIG, &hello);
    assert_eq!(h.device().mesh_config_digest, None, "version 2 was read");
    let mut request = config_header(1, 0, 0, 0, 0, uid_n(5));
    request.pop();
    h.feed(op::MESH_CONFIG, &request);
    assert!(
        !h.saw(|e| matches!(e, Event::MeshConfigRequested { .. })),
        "31 bytes"
    );
}

#[test]
fn a_set_is_reported_only_from_this_clients_controller() {
    let mut h = Harness::new(0x31);
    mesh_unit(&mut h);
    let own = h.state.uid;
    let (list, bytes) = settings();
    let crc = crate::wire::crc32(&bytes);
    let set = |h: &mut Harness, subject: DeviceUid| {
        for frame in pages(3, 0, subject, &bytes, crc) {
            h.feed(op::MESH_CONFIG, &frame);
        }
        h.saw(|e| matches!(e, Event::MeshConfigSetRequested { settings, .. } if settings == &list))
    };
    assert!(!set(&mut h, own), "from a unit this client does not follow");
    h.state.mesh_controller = h.sender;
    assert!(!set(&mut h, uid_n(7)), "for another unit");
    assert!(set(&mut h, own));
    assert!(h.state.mesh_configs.is_empty(), "a set kept as a dump");
}

/// One `mesh_bays_entry`, its reserved byte poisoned.
fn bays_entry(input: bool, bay: u16, features: u32, unit: DeviceUid) -> Vec<u8> {
    let mut e = vec![u8::from(input), 0xA5];
    e.extend_from_slice(&bay.to_le_bytes());
    e.extend_from_slice(&features.to_le_bytes());
    e.extend_from_slice(unit.as_bytes());
    e
}

/// A numbering of 50 bays: inputs 0 to 24 of units 0 to 24, then outputs
/// 1 to 25 of units 100 to 124, each with features of its own.
fn numbering() -> (Vec<MeshBay>, Vec<u8>) {
    let mut bays = Vec::new();
    let mut bytes = Vec::new();
    for n in 0u8..50 {
        let input = n < 25;
        let bay = MeshBay {
            input,
            bay: if input {
                u16::from(n)
            } else {
                u16::from(n) - 24
            },
            features: DeviceFeature::from_bits(0x0040_0018 ^ u32::from(n)),
            unit: uid_n(if input { n } else { n + 75 }),
        };
        bytes.extend(bays_entry(
            bay.input,
            bay.bay,
            bay.features.bits(),
            bay.unit,
        ));
        bays.push(bay);
    }
    (bays, bytes)
}

#[test]
fn a_numbering_is_taken_entry_by_entry_once_every_frame_has_arrived() {
    let mut h = Harness::new(0x33);
    mesh_unit(&mut h);
    let (bays, bytes) = numbering();
    assert_eq!(bytes.len(), 1200);
    let crc = crate::wire::crc32(&bytes);
    let frames = pages(5, 1, uid_n(7), &bytes, crc);
    for (i, frame) in frames.iter().enumerate() {
        assert!(h.state.mesh_bays.is_empty(), "taken after {i} frames");
        h.feed(op::MESH_CONFIG, frame);
    }
    let expected = MeshBays {
        subject: uid_n(7),
        holder: h.sender,
        copy: true,
        bays,
    };
    assert_eq!(h.state.mesh_bays.get(&uid_n(7)), Some(&expected));
    assert!(h.saw(|e| matches!(e, Event::MeshBaysReceived { bays, .. } if bays == &expected)));
    assert!(
        h.state.mesh_configs.is_empty(),
        "a numbering kept as settings"
    );
}

#[test]
fn a_numbering_of_ragged_entries_or_another_crc_is_dropped() {
    let (_, bytes) = numbering();
    let fed = |bytes: &[u8], crc: u32| {
        let mut h = Harness::new(0x33);
        mesh_unit(&mut h);
        for frame in pages(5, 0, uid_n(7), bytes, crc) {
            h.feed(op::MESH_CONFIG, &frame);
        }
        h.state.mesh_bays.is_empty()
    };
    assert!(!fed(&bytes, crate::wire::crc32(&bytes)));
    let ragged = &bytes[..bytes.len() - 1];
    assert!(fed(ragged, crate::wire::crc32(ragged)), "a part entry");
    assert!(fed(&bytes, crate::wire::crc32(&bytes) ^ 1), "another crc");
}

#[test]
fn a_numbering_of_no_bays_and_a_request_for_one_are_read() {
    let mut h = Harness::new(0x33);
    mesh_unit(&mut h);
    h.feed(
        op::MESH_CONFIG,
        &config_header(5, 0, 0, 0, crate::wire::crc32(&[]), uid_n(7)),
    );
    assert_eq!(
        h.state.mesh_bays.get(&uid_n(7)).map(|b| b.bays.len()),
        Some(0)
    );
    h.feed(op::MESH_CONFIG, &config_header(4, 0, 0, 0, 0, uid_n(9)));
    assert!(
        h.saw(|e| matches!(e, Event::MeshBaysRequested { subject, .. } if *subject == uid_n(9)))
    );
}

/// A `CONTROLLER_STATE` hello, byte by byte: header, role, part count, two
/// reserved bytes poisoned, the primary and one entry per part.
fn controller_hello(
    version: u8,
    kind: u8,
    role: u8,
    primary: DeviceUid,
    parts: &[(u8, u32, u32)],
) -> Vec<u8> {
    let mut p = vec![version, kind, 0, 0xA5];
    p.extend_from_slice(&0x0102_0304u32.to_le_bytes());
    p.extend_from_slice(&[0; 12]);
    p.extend_from_slice(&[role, parts.len() as u8, 0xA5, 0xA5]);
    p.extend_from_slice(primary.as_bytes());
    for &(part, length, crc) in parts {
        p.extend_from_slice(&[part, 0xA5, 0xA5, 0xA5]);
        p.extend_from_slice(&length.to_le_bytes());
        p.extend_from_slice(&crc.to_le_bytes());
    }
    p
}

/// A `CONTROLLER_STATE` hello (version 1, kind 0, 40 bytes or more) from a
/// known sender marks it as a dedicated controller (matrixos e958c49); one of
/// another version or kind, or shorter, is dropped, as is a stranger's.
#[test]
fn a_controller_hello_marks_a_dedicated_controller() {
    let mut h = Harness::new(84);
    let stranger = controller_hello(1, 0, 1, uid_n(84), &[]);
    h.feed(op::CONTROLLER_STATE, &stranger);
    assert!(h.state.device(h.sender).is_none(), "a stranger registered");

    h.feed(
        op::SYS_HELLO,
        &hello_payload(0x34, "Router", "SH0004", "1.0", DeviceFeature::MESH),
    );
    for (what, payload) in [
        ("version 2", controller_hello(2, 0, 1, uid_n(84), &[])),
        ("a request", controller_hello(1, 1, 1, uid_n(84), &[])),
        (
            "39 bytes",
            controller_hello(1, 0, 1, uid_n(84), &[])[..39].to_vec(),
        ),
    ] {
        h.feed(op::CONTROLLER_STATE, &payload);
        assert_eq!(h.device().controller_hello, None, "{what}");
    }

    let mut hello = controller_hello(
        1,
        0,
        2,
        uid_n(85),
        &[(0, 0x0000_1234, 0xCAFE_F00D), (3, 0x0000_0800, 0x0BAD_BEEF)],
    );
    // A part entry cut short is left out.
    hello.extend_from_slice(&[4, 0, 0, 0, 1]);
    hello[21] = 3;
    h.feed(op::CONTROLLER_STATE, &hello);
    let expected = ControllerHello {
        role: crate::ControllerRole::FAILSAFE,
        generation: 0x0102_0304,
        primary: uid_n(85),
        parts: vec![
            ControllerPart {
                part: 0,
                length: 0x1234,
                crc: 0xCAFE_F00D,
            },
            ControllerPart {
                part: 3,
                length: 0x0800,
                crc: 0x0BAD_BEEF,
            },
        ],
    };
    assert_eq!(h.device().controller_hello, Some(expected.clone()));
    assert!(
        h.saw(|e| matches!(e, Event::ControllerHelloChanged { hello, .. } if *hello == expected))
    );
}

/// A hello, or a dump of the sender's own settings, flags lost settings in
/// bit 1; a copy another unit keeps is never its subject's lost settings.
#[test]
fn lost_settings_are_flagged_on_a_hello_and_an_own_dump() {
    let mut h = Harness::new(0x31);
    mesh_unit(&mut h);
    h.feed(
        op::MESH_CONFIG,
        &config_header(0, 2, 0, 0x0602, 0x8899_AABB, h.sender),
    );
    assert_eq!(
        h.device().mesh_config_digest,
        Some(MeshConfigDigest {
            length: 0x0602,
            crc32: 0x8899_AABB,
            lost: true,
        })
    );
    h.feed(
        op::MESH_CONFIG,
        &config_header(0, 1, 0, 0x0602, 0x8899_AABB, h.sender),
    );
    assert!(
        !h.device().mesh_config_digest.expect("a hello").lost,
        "COPY read as LOST"
    );

    let (_, bytes) = settings();
    let crc = crate::wire::crc32(&bytes);
    for frame in pages(2, 2, h.sender, &bytes, crc) {
        h.feed(op::MESH_CONFIG, &frame);
    }
    assert!(h.state.mesh_configs[&h.sender].lost, "an own dump");
    for frame in pages(2, 3, uid_n(7), &bytes, crc) {
        h.feed(op::MESH_CONFIG, &frame);
    }
    let copy = &h.state.mesh_configs[&uid_n(7)];
    assert!(copy.copy && !copy.lost, "a copy was taken as lost");
}
