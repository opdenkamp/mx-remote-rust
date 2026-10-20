// Author: Lars Op den Kamp (lars@opdenkamp-it.nl)
// Copyright (c) 2026 Op den Kamp IT Solutions

//! Overlay sync frames, read as a watcher.

use std::net::Ipv4Addr;

use crate::event::Event;
use crate::types::{OsdSyncSlot, OsdSyncValue};
use crate::wire::{
    op, DeviceFeature, DeviceUid, OsdCaptionAlign, OsdCaptionFormat, OsdMessageFlag,
    OsdMessagePriority, OsdSyncSize, OsdSyncState, OsdSyncWhat,
};

use crate::testing::{hello_payload, poisoned, uid_n};

use super::Harness;

/// A management application announcing itself, and a unit that is not one.
fn mesh() -> (Harness, DeviceUid, DeviceUid) {
    let mut h = Harness::new(160);
    h.hello(0x2F, "MANAGER", "MG0001", DeviceFeature::MANAGER);
    let source = h.sender;
    let unit = uid_n(161);
    h.feed_as(
        unit,
        op::SYS_HELLO,
        &hello_payload(0x2F, "ONEIP", "OI0001", "4.8.0", DeviceFeature::V2IP_SINK),
    );
    (h, source, unit)
}

/// A frame of `kind` and `size` bytes: a poisoned body under the header.
fn frame(kind: u8, seq: u16, target: DeviceUid, size: usize) -> Vec<u8> {
    let mut p = poisoned(size);
    p[0] = 2;
    p[1] = kind;
    p[2..4].copy_from_slice(&seq.to_le_bytes());
    p[4..20].copy_from_slice(target.as_bytes());
    p
}

/// A sync with every field distinct: a port and records with a high byte, an
/// address whose bytes differ, and slots that differ from each other.
fn sync_frame(seq: u16, target: DeviceUid, what: u8, records: u16) -> Vec<u8> {
    let mut p = frame(0, seq, target, 80);
    p[20] = what;
    p[22..24].copy_from_slice(&0x1F90u16.to_le_bytes());
    p[24..28].copy_from_slice(&[10, 1, 2, 3]);
    p[28..30].copy_from_slice(&records.to_le_bytes());
    for i in 0..4u8 {
        let at = 32 + usize::from(i) * 12;
        p[at..at + 4].copy_from_slice(&(0x0001_0000 + u32::from(i)).to_le_bytes());
        p[at + 4..at + 8].copy_from_slice(&(0xC0DE_0000 + u32::from(i)).to_le_bytes());
        p[at + 8] = 50 + i;
    }
    p
}

fn sync_events(h: &Harness) -> usize {
    h.events
        .iter()
        .filter(|e| matches!(e, Event::OsdSyncChanged { .. }))
        .count()
}

#[test]
fn a_sync_is_read_field_by_field() {
    let (mut h, source, unit) = mesh();
    h.feed_as(
        source,
        op::V2IP_OSD_SYNC,
        &sync_frame(0x1234, unit, 0x0F, 128),
    );

    let s = h.device().osd_sync.clone().expect("no sync was read");
    assert_eq!(s.seq, 0x1234);
    assert_eq!(s.target, unit);
    assert_eq!(s.what, OsdSyncWhat::from_bits(0x0F));
    assert_eq!(s.http_port, 0x1F90);
    assert_eq!(s.source_ip, Ipv4Addr::new(10, 1, 2, 3));
    assert_eq!(s.records, 128);
    let slots = s.slots.expect("the slots were not read");
    for (i, slot) in (0u8..).zip(slots) {
        assert_eq!(
            slot,
            OsdSyncSlot {
                bytes: 0x0001_0000 + u32::from(i),
                crc32: 0xC0DE_0000 + u32::from(i),
                percent: 50 + i,
            }
        );
    }
    assert_eq!(sync_events(&h), 1);

    // The same sync again, as it arrives on multicast and broadcast.
    h.feed_as(
        source,
        op::V2IP_OSD_SYNC,
        &sync_frame(0x1234, unit, 0x0F, 128),
    );
    assert_eq!(sync_events(&h), 1, "a repeated sync was announced");

    // Slots are read only when the sync copies them.
    h.feed_as(
        source,
        op::V2IP_OSD_SYNC,
        &sync_frame(0x1235, unit, 0x07, 3),
    );
    assert_eq!(h.device().osd_sync.clone().expect("no sync").slots, None);
}

#[test]
fn a_sync_no_unit_would_take_is_not_read() {
    let (mut h, source, unit) = mesh();
    let cases: [(&str, DeviceUid, Vec<u8>); 4] = [
        ("too many records", source, sync_frame(1, unit, 1, 129)),
        (
            "one byte short",
            source,
            sync_frame(2, unit, 1, 1)[..79].to_vec(),
        ),
        ("another version", source, {
            let mut p = sync_frame(3, unit, 1, 1);
            p[0] = 3;
            p
        }),
        ("not management", unit, sync_frame(4, unit, 1, 1)),
    ];
    for (why, sender, p) in cases {
        h.feed_as(sender, op::V2IP_OSD_SYNC, &p);
        assert_eq!(sync_events(&h), 0, "{why}: the sync was read");
    }
}

#[test]
fn a_sync_from_the_controller_a_unit_names_is_read() {
    let (mut h, _, unit) = mesh();
    let controller = uid_n(162);
    h.feed_as(
        controller,
        op::SYS_HELLO,
        &hello_payload(0x2F, "ONEIP", "OI0002", "4.8.0", DeviceFeature::V2IP_SINK),
    );
    h.feed_as(controller, op::V2IP_OSD_SYNC, &sync_frame(7, unit, 1, 1));
    assert_eq!(sync_events(&h), 0, "a unit no one follows was management");

    h.state.device_mut(unit).expect("no unit").mesh_master = controller;
    h.feed_as(controller, op::V2IP_OSD_SYNC, &sync_frame(7, unit, 1, 1));
    assert_eq!(sync_events(&h), 1);
}

fn requests(h: &Harness) -> Vec<(DeviceUid, crate::types::OsdSyncRequest)> {
    h.events
        .iter()
        .filter_map(|e| match e {
            Event::OsdSyncRequested { device, request } => Some((*device, request.clone())),
            _ => None,
        })
        .collect()
}

/// A request is read with the source of the sync it asks about, and only
/// whole.
#[test]
fn a_request_is_read_with_its_source() {
    let (mut h, source, unit) = mesh();
    let mut p = frame(1, 0x0102, unit, 54);
    p[20] = 0x05;
    p[22..38].copy_from_slice(&[0x81; 16]);
    p[38..54].copy_from_slice(source.as_bytes());
    h.feed_as(unit, op::V2IP_OSD_SYNC, &p);
    h.feed_as(unit, op::V2IP_OSD_SYNC, &p[..53]);
    // A request names the unit asking; one naming another is not its own.
    h.feed_as(source, op::V2IP_OSD_SYNC, &p);

    let seen = requests(&h);
    assert_eq!(seen.len(), 1);
    let (device, long) = &seen[0];
    assert_eq!(*device, unit);
    assert_eq!(long.seq, 0x0102);
    assert_eq!(long.what, OsdSyncWhat::from_bits(0x05));
    assert_eq!(long.missing, [0x81; 16]);
    assert_eq!(long.source, source);
    assert!(long.is_missing(0) && long.is_missing(7) && long.is_missing(127));
    assert!(!long.is_missing(1) && !long.is_missing(128));
}

fn settings(h: &Harness) -> Vec<crate::types::OsdSyncSetting> {
    h.events
        .iter()
        .filter_map(|e| match e {
            Event::OsdSyncSettingReceived { setting, .. } => Some(setting.clone()),
            _ => None,
        })
        .collect()
}

/// A setting of `kind` with `body`, index 0x0102 of 0x0103.
fn setting_frame(unit: DeviceUid, kind: u8, body: &[u8]) -> Vec<u8> {
    let mut p = frame(2, 0x0304, unit, 30);
    p[20..22].copy_from_slice(&0x0102u16.to_le_bytes());
    p[22..24].copy_from_slice(&0x0103u16.to_le_bytes());
    p[24] = kind;
    p[25] = 2;
    p[26..28].copy_from_slice(&0x0105u16.to_le_bytes());
    p[28..30].copy_from_slice(&0x01FFu16.to_le_bytes());
    p.extend_from_slice(body);
    p
}

fn caption_body(text_len: u16, text: &[u8]) -> Vec<u8> {
    let mut b = poisoned(48);
    b[..32].fill(0);
    b[..4].copy_from_slice(b"Sans");
    b[32..34].copy_from_slice(&0x0123u16.to_le_bytes());
    b[34..36].copy_from_slice(&(-300i16).to_le_bytes());
    b[36..38].copy_from_slice(&i16::MIN.to_le_bytes());
    b[38..40].copy_from_slice(&(-2i16).to_le_bytes());
    b[40] = 1;
    b[41] = 0xFF;
    b[42..46].copy_from_slice(&0x00AB_CDEFu32.to_le_bytes());
    b[46..48].copy_from_slice(&text_len.to_le_bytes());
    b.extend_from_slice(text);
    b
}

#[test]
fn a_setting_is_read_field_by_field() {
    let (mut h, source, unit) = mesh();
    h.feed_as(
        source,
        op::V2IP_OSD_SYNC,
        &setting_frame(unit, 0, &caption_body(5, "h\u{e9}llo!".as_bytes())),
    );
    h.feed_as(
        source,
        op::V2IP_OSD_SYNC,
        &setting_frame(unit, 0, &caption_body(9, b"short")),
    );
    h.feed_as(
        source,
        op::V2IP_OSD_SYNC,
        &setting_frame(unit, 1, &[0xFF, 0xEE, 0x34, 0x12, 0x00, 0x80]),
    );
    h.feed_as(
        source,
        op::V2IP_OSD_SYNC,
        &setting_frame(unit, 2, &0x0012_3456u32.to_le_bytes()),
    );

    // A unit does not take a caption missing text it counts.
    let seen = settings(&h);
    assert_eq!(seen.len(), 3);
    let s = &seen[0];
    assert_eq!(
        (s.seq, s.target, s.index, s.records),
        (0x0304, unit, 0x0102, 0x0103)
    );
    assert_eq!(s.size, OsdSyncSize::SMALL);
    assert_eq!((s.layers, s.set), (0x0105, 0x01FF));
    let OsdSyncValue::Caption(c) = &s.value else {
        panic!("not a caption: {:?}", s.value);
    };
    assert_eq!(c.font, "Sans");
    assert_eq!((c.size, c.x, c.y, c.wrap), (0x0123, -300, i16::MIN, -2));
    assert_eq!(c.align, OsdCaptionAlign::CENTRE);
    assert_eq!(c.format, OsdCaptionFormat::DEFAULT);
    assert_eq!(c.colour, 0x00AB_CDEF);
    // Five bytes of text, of the six that follow.
    assert_eq!(c.text, "h\u{e9}ll");
    assert_eq!(
        seen[1].value,
        OsdSyncValue::Logo(crate::types::OsdLogo {
            logo: -1,
            x: 0x1234,
            y: i16::MIN,
        })
    );
    assert_eq!(seen[2].value, OsdSyncValue::Background(0x0012_3456));
}

#[test]
fn a_setting_no_unit_would_take_is_not_read() {
    let (mut h, source, unit) = mesh();
    let mut past_records = setting_frame(unit, 2, &[0; 4]);
    past_records[20] = 3;
    let cases: [(&str, DeviceUid, Vec<u8>); 6] = [
        ("index past records", source, past_records),
        ("short caption", source, setting_frame(unit, 0, &[0; 47])),
        ("short logo", source, setting_frame(unit, 1, &[0; 5])),
        ("short background", source, setting_frame(unit, 2, &[0; 3])),
        ("unknown type", source, setting_frame(unit, 3, &[0; 48])),
        ("not management", unit, setting_frame(unit, 2, &[0; 4])),
    ];
    for (why, sender, p) in cases {
        h.feed_as(sender, op::V2IP_OSD_SYNC, &p);
        assert!(settings(&h).is_empty(), "{why}: the setting was read");
    }
}

fn report_frame(unit: DeviceUid) -> Vec<u8> {
    let mut p = frame(3, 0x0506, unit, 90);
    p[20] = 2;
    p[22..24].copy_from_slice(&0x0102u16.to_le_bytes());
    p[24..26].copy_from_slice(&0x0304u16.to_le_bytes());
    p[26..90].fill(b'r');
    p
}

#[test]
fn a_report_is_read_field_by_field() {
    let (mut h, source, unit) = mesh();
    h.feed_as(unit, op::V2IP_OSD_SYNC, &report_frame(unit));
    h.feed_as(unit, op::V2IP_OSD_SYNC, &report_frame(unit));

    let r = h
        .state
        .device(unit)
        .and_then(|d| d.osd_sync_report.clone())
        .expect("no report was read");
    assert_eq!(r.seq, 0x0506);
    assert_eq!(r.state, OsdSyncState::FAILED);
    assert_eq!((r.written, r.refused), (0x0102, 0x0304));
    assert_eq!(r.reason, "r".repeat(64));
    let reports = |h: &Harness| {
        h.events
            .iter()
            .filter(|e| matches!(e, Event::OsdSyncReportChanged { .. }))
            .count()
    };
    assert_eq!(reports(&h), 1, "a repeated report was announced");

    // One byte short, and one naming a unit other than its sender.
    let mut other = report_frame(unit);
    other[20] = 1;
    h.feed_as(unit, op::V2IP_OSD_SYNC, &other[..89]);
    h.feed_as(source, op::V2IP_OSD_SYNC, &other);
    assert_eq!(reports(&h), 1);
}

/// A frame too short for its header is not read.
#[test]
fn a_frame_without_a_header_is_not_read() {
    let (mut h, _, unit) = mesh();
    h.feed_as(unit, op::V2IP_OSD_SYNC, &[]);
    h.feed_as(unit, op::V2IP_OSD_SYNC, &[2]);
    assert!(h.events.iter().all(|e| !matches!(
        e,
        Event::OsdSyncRequested { .. } | Event::OsdSyncReportChanged { .. }
    )));
}

/// A unit claiming to control a mesh is taken for management only until the
/// units say they follow another.
#[test]
fn a_sync_from_a_controller_units_do_not_follow_is_not_read() {
    let (mut h, _, unit) = mesh();
    let other = uid_n(162);
    h.feed_as(
        other,
        op::SYS_HELLO,
        &hello_payload(0x2F, "ONEIP", "OI0002", "4.8.0", DeviceFeature::MESH_MASTER),
    );
    h.feed_as(
        other,
        op::V2IP_OSD_SYNC,
        &sync_frame(7, DeviceUid::ZERO, 1, 2),
    );
    assert_eq!(sync_events(&h), 1, "a controller's own claim stands");

    let mut mesh = poisoned(40);
    mesh[0] = 0xFF;
    mesh[4..20].copy_from_slice(uid_n(163).as_bytes());
    h.feed_as(unit, op::MESH_OPERATION, &mesh);
    h.feed_as(
        other,
        op::V2IP_OSD_SYNC,
        &sync_frame(8, DeviceUid::ZERO, 1, 3),
    );
    assert_eq!(sync_events(&h), 1, "a controller no unit follows");
}

/// A message of `kind` from `unit`'s output: every flag but REFUSED, no logo,
/// the top priority, and `text` counted, with poisoned bytes behind it.
fn message_frame(kind: u8, unit: DeviceUid, text: &[u8]) -> Vec<u8> {
    let mut p = frame(kind, 0, unit, 26 + text.len() + 4);
    p[20..24].copy_from_slice(&[0x07, 0xFF, 2, 0xA5]);
    p[24..26].copy_from_slice(&(text.len() as u16).to_le_bytes());
    p[26..26 + text.len()].copy_from_slice(text);
    p
}

fn message_of(h: &Harness, unit: DeviceUid) -> Option<crate::types::OsdMessage> {
    h.state.device(unit).and_then(|d| d.osd_message.clone())
}

/// A unit's message is read field by field, the text as long as it counts,
/// and announced only when it changes.
#[test]
fn a_message_is_read_field_by_field() {
    let (mut h, _, unit) = mesh();
    let text = b"Input <b>3</b> is back";
    h.feed_as(unit, op::V2IP_OSD_SYNC, &message_frame(6, unit, text));

    let m = message_of(&h, unit).expect("no message was read");
    assert_eq!(m.flags, OsdMessageFlag::from_bits(0x07));
    assert_eq!(m.logo, -1);
    assert_eq!(m.priority, OsdMessagePriority::TOP);
    assert_eq!(m.text.as_bytes(), text);
    let changes = |h: &Harness| {
        h.events
            .iter()
            .filter(|e| matches!(e, Event::OsdMessageChanged { device, .. } if *device == unit))
            .count()
    };
    assert_eq!(changes(&h), 1);
    h.feed_as(unit, op::V2IP_OSD_SYNC, &message_frame(6, unit, text));
    assert_eq!(changes(&h), 1, "an unchanged message was announced");
}

/// A unit answers for its own output only, and with the whole text; a
/// request or a write is for a unit with an output.
#[test]
fn only_a_units_own_whole_message_is_kept() {
    let (mut h, source, unit) = mesh();
    let text = b"Back soon";
    let mut short = message_frame(6, unit, text);
    short.truncate(26 + text.len() - 1);
    let mut long = message_frame(6, unit, text);
    long[25] = 1;
    for (what, from, frame) in [
        ("another unit's", source, message_frame(6, unit, text)),
        ("a cut text", unit, short),
        ("a text counted past the frame", unit, long),
        ("a request", unit, message_frame(4, unit, text)),
        ("a write", unit, message_frame(5, unit, text)),
    ] {
        h.feed_as(from, op::V2IP_OSD_SYNC, &frame);
        assert_eq!(message_of(&h, unit), None, "{what} was kept");
        assert_eq!(message_of(&h, source), None, "{what} was kept");
    }
}
