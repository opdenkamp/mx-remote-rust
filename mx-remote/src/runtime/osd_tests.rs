// Author: Lars Op den Kamp (lars@opdenkamp-it.nl)
// Copyright (c) 2026 Op den Kamp IT Solutions

//! Sending an overlay sync, and answering the units that take it.

use std::io::{Read, Write};
use std::net::{Ipv4Addr, TcpStream};
use std::sync::Arc;

use crate::testing::{datagram, hello_payload, uid_n};
use crate::types::{
    OsdCaption, OsdLogo, OsdSyncContent, OsdSyncEntry, OsdSyncImage, OsdSyncValue,
    OSD_MESSAGE_TEXT_MAX, OSD_SYNC_IMAGE_MAX,
};
use crate::wire::{
    crc32, op, Conn, DeviceFeature, DeviceUid, OsdCaptionAlign, OsdCaptionFormat,
    OsdMessagePriority, OsdSyncSize, OsdSyncWhat, HEADER_LEN, PROTOCOL_VERSION,
};

use super::super::tests::{client_with, Tap};
use super::*;

const FROM: Ipv4Addr = Ipv4Addr::new(10, 8, 8, 9);

/// Where the socket points. Deliberately not the MX Remote port, so nothing
/// here can reach a live AV network.
const SCRATCH_PORT: u16 = 18819;

/// This client, and two units that have announced themselves.
struct Mesh {
    remote: Remote,
    tap: Arc<Tap>,
    a: DeviceUid,
    b: DeviceUid,
}

fn mesh() -> Mesh {
    let (remote, tap) = client_with(0xFE, Arc::new(()));
    let conn = Conn::open(
        Ipv4Addr::LOCALHOST,
        SCRATCH_PORT,
        Some(Ipv4Addr::LOCALHOST),
        None,
    )
    .expect("a loopback socket could not be opened");
    lock(&remote.shared.tx).set_conn(Some(conn));
    let m = Mesh {
        remote,
        tap,
        a: uid_n(0x21),
        b: uid_n(0x22),
    };
    for (uid, serial) in [(m.a, "OI0021"), (m.b, "OI0022")] {
        m.feed(
            uid,
            op::SYS_HELLO,
            &hello_payload(
                0x2F,
                "OneIP",
                serial,
                "4.8.0",
                DeviceFeature::V2IP_SINK | DeviceFeature::OSD_SYNC,
            ),
        );
    }
    m.tap.clear();
    m
}

impl Mesh {
    fn feed(&self, sender: DeviceUid, opcode: crate::wire::Opcode, payload: &[u8]) {
        self.remote
            .shared
            .process_datagram(&datagram(sender, opcode, PROTOCOL_VERSION, payload), FROM);
    }

    /// `unit` asking for the settings of `seq` set in `missing`, naming
    /// `source` as who sent the sync.
    fn request(&self, unit: DeviceUid, seq: u16, what: u8, missing: &[u16], source: DeviceUid) {
        let mut p = vec![2, 1];
        p.extend_from_slice(&seq.to_le_bytes());
        p.extend_from_slice(unit.as_bytes());
        p.extend_from_slice(&[what, 0]);
        let mut bits = [0u8; 16];
        for &i in missing {
            bits[usize::from(i / 8)] |= 1 << (i % 8);
        }
        p.extend_from_slice(&bits);
        p.extend_from_slice(source.as_bytes());
        self.feed(unit, op::V2IP_OSD_SYNC, &p);
    }

    /// The overlay sync payloads sent since the tap was last cleared.
    fn sent(&self) -> Vec<Vec<u8>> {
        self.tap
            .frames()
            .into_iter()
            .filter(|f| f[20..22] == op::V2IP_OSD_SYNC.0.to_le_bytes())
            .map(|f| f[HEADER_LEN..].to_vec())
            .collect()
    }

    /// The feature bits of the hellos sent since the tap was last cleared.
    fn hellos(&self) -> Vec<u32> {
        self.tap
            .frames()
            .into_iter()
            .filter(|f| f[20..22] == op::SYS_HELLO.0.to_le_bytes())
            .map(|f| u32_at(&f[HEADER_LEN..], 50))
            .collect()
    }

    fn uid(&self) -> DeviceUid {
        self.remote.uid()
    }
}

fn u16_at(p: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([p[at], p[at + 1]])
}

fn u32_at(p: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([p[at], p[at + 1], p[at + 2], p[at + 3]])
}

fn target(p: &[u8]) -> DeviceUid {
    DeviceUid::from_array(p[4..20].try_into().expect("16 bytes"))
}

fn caption(text: &str) -> OsdSyncEntry {
    OsdSyncEntry {
        size: OsdSyncSize::LARGE,
        layers: 1 << 4,
        set: 0x0100,
        value: OsdSyncValue::Caption(OsdCaption {
            font: String::new(),
            size: 0,
            x: i16::MIN,
            y: i16::MIN,
            wrap: -2,
            align: OsdCaptionAlign::DEFAULT,
            format: OsdCaptionFormat::PLAIN,
            colour: u32::MAX,
            text: text.to_owned(),
        }),
    }
}

/// Captions and backgrounds, with `n` settings.
fn content(n: usize) -> OsdSyncContent {
    OsdSyncContent {
        what: OsdSyncWhat::CAPTIONS | OsdSyncWhat::BACKGROUNDS,
        entries: (0..n).map(|i| caption(&format!("caption {i}"))).collect(),
        images: None,
    }
}

/// A PNG's signature and header, for an image of `width` by `height`.
fn png(width: u32, height: u32) -> Vec<u8> {
    let mut p = b"\x89PNG\r\n\x1a\n\0\0\0\x0dIHDR".to_vec();
    p.extend_from_slice(&width.to_be_bytes());
    p.extend_from_slice(&height.to_be_bytes());
    p.extend_from_slice(&[8, 6, 0, 0, 0]);
    p
}

fn images() -> [OsdSyncImage; 4] {
    [
        OsdSyncImage {
            png: png(200, 100),
            percent: 25,
        },
        OsdSyncImage::default(),
        OsdSyncImage {
            png: png(1920, 1080),
            percent: 0,
        },
        OsdSyncImage {
            png: png(64, 64),
            percent: 100,
        },
    ]
}

#[test]
fn a_sync_goes_to_every_unit_and_names_its_settings() {
    let m = mesh();
    let seq = m.remote.send_osd_sync(content(3)).expect("sent");
    let sent = m.sent();
    assert_eq!(sent.len(), 1);
    let p = &sent[0];
    assert_eq!(&p[..2], &[2, 0]);
    assert_eq!(u16_at(p, 2), seq);
    assert_eq!(target(p), DeviceUid::ZERO);
    assert_eq!(p[20], 0x05);
    assert_eq!(u16_at(p, 22), 0, "no images, so no server");
    assert_eq!(u16_at(p, 28), 3);
}

#[test]
fn holding_a_sync_announces_the_manager_part() {
    let m = mesh();
    m.remote.send_osd_sync(content(1)).expect("sent");
    let manager = (DeviceFeature::MANAGER | DeviceFeature::CONFIG_INITIALISED).bits();
    let osd = DeviceFeature::OSD_SYNC.bits();
    assert_eq!(m.hellos(), vec![manager | osd]);
    m.tap.clear();
    m.remote.send_osd_sync(content(2)).expect("sent");
    assert_eq!(
        m.hellos(),
        vec![],
        "replacing it changes nothing to announce"
    );
    m.remote.clear_osd_sync();
    assert_eq!(m.hellos(), vec![manager]);
    m.tap.clear();
    m.remote.clear_osd_sync();
    assert_eq!(m.hellos(), vec![]);
}

#[test]
fn settings_go_only_to_the_unit_that_asked_and_only_what_it_lacks() {
    let m = mesh();
    let seq = m.remote.send_osd_sync(content(10)).expect("sent");
    m.tap.clear();
    m.request(m.a, seq, 0x05, &[1, 4, 6, 7, 9], m.uid());
    assert!(m.sent().is_empty(), "nothing is sent until the pacer runs");
    assert_eq!(m.remote.shared.osd_pass(), 4, "four to a pass");
    assert_eq!(m.remote.shared.osd_pass(), 1);
    assert_eq!(m.remote.shared.osd_pass(), 0);
    let sent = m.sent();
    let indexes: Vec<u16> = sent.iter().map(|p| u16_at(p, 20)).collect();
    assert_eq!(indexes, vec![1, 4, 6, 7, 9]);
    for p in &sent {
        assert_eq!(&p[..2], &[2, 2]);
        assert_eq!(u16_at(p, 2), seq);
        assert_eq!(target(p), m.a);
        assert_eq!(u16_at(p, 22), 10);
    }
    let text = |p: &Vec<u8>| String::from_utf8(p[30 + 48..].to_vec()).expect("utf-8");
    assert_eq!(text(&sent[2]), "caption 6");
}

#[test]
fn a_repeated_request_queues_nothing_twice() {
    let m = mesh();
    let seq = m.remote.send_osd_sync(content(3)).expect("sent");
    m.tap.clear();
    m.request(m.a, seq, 0x05, &[0, 2], m.uid());
    m.request(m.a, seq, 0x05, &[0, 2], m.uid());
    m.request(m.b, seq, 0x05, &[2], m.uid());
    assert_eq!(m.remote.shared.osd_pass(), 3);
    assert_eq!(m.remote.shared.osd_pass(), 0);
}

/// A request for settings is answered by the source it names, and one
/// naming none by nobody.
#[test]
fn a_request_for_another_source_is_left_to_it() {
    let m = mesh();
    let seq = m.remote.send_osd_sync(content(3)).expect("sent");
    m.tap.clear();
    m.request(m.a, seq, 0x05, &[0], m.b);
    m.request(m.a, seq, 0x05, &[0], DeviceUid::ZERO);
    assert_eq!(m.remote.shared.osd_pass(), 0);
    assert!(m.sent().is_empty());
}

#[test]
fn a_request_for_a_sync_no_longer_held_gets_a_new_one() {
    let m = mesh();
    let old = m.remote.send_osd_sync(content(3)).expect("sent");
    let held = m.remote.send_osd_sync(content(2)).expect("sent");
    m.tap.clear();
    m.request(m.a, old, 0x01, &[0, 1, 2], m.uid());
    assert_eq!(m.remote.shared.osd_pass(), 0, "no settings of the old one");
    let sent = m.sent();
    assert_eq!(sent.len(), 1);
    let p = &sent[0];
    assert_eq!(p[1], 0, "a sync");
    assert_ne!(u16_at(p, 2), old);
    assert_ne!(
        u16_at(p, 2),
        held,
        "a unit takes no sync numbered as its last"
    );
    assert_eq!(target(p), m.a);
    assert_eq!(p[20], 0x01, "what it asked, of what is held");
    assert_eq!(u16_at(p, 28), 2);

    // The new one is answered like the first.
    let new = u16_at(p, 2);
    m.tap.clear();
    m.request(m.a, new, 0x05, &[1], m.uid());
    assert_eq!(m.remote.shared.osd_pass(), 1);

    // Asking for nothing held gets nothing.
    m.tap.clear();
    m.request(m.a, old, 0x02, &[0], m.uid());
    assert!(m.sent().is_empty(), "a sync copying nothing");
}

#[test]
fn a_unit_asking_for_a_new_sync_gets_what_it_asks_of_what_is_held() {
    let m = mesh();
    let seq = m.remote.send_osd_sync(content(2)).expect("sent");
    m.tap.clear();
    m.request(m.b, 0, 0x0F, &[], DeviceUid::ZERO);
    let sent = m.sent();
    assert_eq!(sent.len(), 1);
    let p = &sent[0];
    assert_eq!(p[1], 0);
    assert_ne!(u16_at(p, 2), seq);
    assert_eq!(target(p), m.b);
    assert_eq!(p[20], 0x05);

    m.tap.clear();
    m.request(m.b, 0, 0x01, &[], DeviceUid::ZERO);
    assert_eq!(m.sent()[0][20], 0x01);

    m.tap.clear();
    m.request(m.b, 0, 0x02, &[], DeviceUid::ZERO);
    assert!(m.sent().is_empty(), "it asks for nothing this client holds");
}

#[test]
fn nothing_is_answered_while_no_sync_is_held() {
    let m = mesh();
    m.request(m.a, 0, 0x0F, &[], DeviceUid::ZERO);
    assert!(m.sent().is_empty());
    let seq = m.remote.send_osd_sync(content(2)).expect("sent");
    m.remote.clear_osd_sync();
    m.tap.clear();
    m.request(m.a, 0, 0x0F, &[], DeviceUid::ZERO);
    m.request(m.a, seq, 0x05, &[0], m.uid());
    assert_eq!(m.remote.shared.osd_pass(), 0);
    assert!(m.sent().is_empty());
}

#[test]
fn settings_queued_for_a_replaced_sync_are_not_sent() {
    let m = mesh();
    let seq = m.remote.send_osd_sync(content(3)).expect("sent");
    m.request(m.a, seq, 0x05, &[0, 1, 2], m.uid());
    m.remote.send_osd_sync(content(3)).expect("sent");
    assert_eq!(m.remote.shared.osd_pass(), 0);
}

#[test]
fn settings_queued_for_a_sync_aged_out_are_not_sent() {
    let m = mesh();
    let seq = m.remote.send_osd_sync(content(3)).expect("sent");
    m.request(m.a, seq, 0x05, &[0], m.uid());
    for _ in 0..SEQS_KEPT - 1 {
        m.remote.resend_osd_sync(m.b).expect("sent");
    }
    m.request(m.a, seq, 0x05, &[0, 1], m.uid());
    assert_eq!(m.remote.shared.osd_pass(), 2, "still the last {SEQS_KEPT}");
    m.request(m.a, seq, 0x05, &[0], m.uid());
    m.remote.resend_osd_sync(m.b).expect("sent");
    assert_eq!(m.remote.shared.osd_pass(), 0);
}

#[test]
fn clearing_drops_settings_already_queued() {
    let m = mesh();
    let seq = m.remote.send_osd_sync(content(2)).expect("sent");
    m.request(m.a, seq, 0x05, &[0, 1], m.uid());
    m.remote.clear_osd_sync();
    assert_eq!(m.remote.shared.osd_pass(), 0);
}

#[test]
fn a_resend_goes_to_one_unit_under_a_new_number() {
    let m = mesh();
    assert!(matches!(
        m.remote.resend_osd_sync(m.a),
        Err(ControlError::InvalidRequest(_))
    ));
    let seq = m.remote.send_osd_sync(content(2)).expect("sent");
    m.tap.clear();
    let again = m.remote.resend_osd_sync(m.a).expect("sent");
    assert_ne!(again, seq);
    let sent = m.sent();
    assert_eq!(sent.len(), 1);
    assert_eq!(u16_at(&sent[0], 2), again);
    assert_eq!(target(&sent[0]), m.a);
    assert_eq!(sent[0][20], 0x05);
    assert!(matches!(
        m.remote.resend_osd_sync(uid_n(0x7F)),
        Err(ControlError::UnknownDevice(_))
    ));
}

#[test]
fn the_number_skips_zero() {
    let m = mesh();
    lock(&m.remote.shared.osd).next_seq = u16::MAX;
    assert_eq!(m.remote.send_osd_sync(content(1)).expect("sent"), u16::MAX);
    assert_eq!(m.remote.resend_osd_sync(m.a).expect("sent"), 1);
}

#[test]
fn a_sync_that_cannot_be_sent_is_not_held() {
    let (remote, tap) = client_with(0xFE, Arc::new(()));
    assert!(matches!(
        remote.send_osd_sync(content(1)),
        Err(ControlError::Send(crate::wire::SendError::NotConnected))
    ));
    assert!(!lock(&remote.shared.osd).holds());
    let _ = tap;
}

#[test]
fn content_a_unit_would_refuse_is_refused() {
    let m = mesh();
    let with = |f: &dyn Fn(&mut OsdSyncContent)| {
        let mut c = content(1);
        f(&mut c);
        c
    };
    let caption_with = |f: &dyn Fn(&mut OsdCaption)| {
        with(&|c: &mut OsdSyncContent| {
            if let OsdSyncValue::Caption(cap) = &mut c.entries[0].value {
                f(cap);
            }
        })
    };
    let logo = |logo: i8| {
        with(&|c: &mut OsdSyncContent| {
            c.entries[0].value = OsdSyncValue::Logo(OsdLogo { logo, x: 0, y: 0 });
        })
    };
    let image = |png: Vec<u8>, percent: u8| {
        with(&|c: &mut OsdSyncContent| {
            let mut i = images();
            i[1] = OsdSyncImage {
                png: png.clone(),
                percent,
            };
            c.images = Some(i);
        })
    };
    // Each is a PNG of an accepted size but for the one part named.
    let mut not_png = png(16, 16);
    not_png[1] = b'G';
    let mut no_ihdr = png(16, 16);
    no_ihdr[12..16].copy_from_slice(b"IDAT");
    let mut oversized = png(16, 16);
    oversized.resize(OSD_SYNC_IMAGE_MAX + 1, 0);
    let refused = [
        ("nothing", OsdSyncContent::default()),
        ("129 settings", content(129)),
        ("font", caption_with(&|c| c.font = "f".repeat(32))),
        ("text", caption_with(&|c| c.text = "t".repeat(361))),
        ("logo 5", logo(5)),
        ("logo -2", logo(-2)),
        ("percent", image(png(16, 16), 101)),
        (
            "only slots, no images",
            OsdSyncContent {
                what: OsdSyncWhat::LOGO_SLOTS,
                ..OsdSyncContent::default()
            },
        ),
        ("not a png", image(not_png.clone(), 0)),
        ("no header", image(no_ihdr.clone(), 0)),
        ("too wide", image(png(1921, 16), 0)),
        ("too tall", image(png(16, 1081), 0)),
        ("no width", image(png(0, 16), 0)),
        ("too big", image(oversized, 0)),
    ];
    for (why, c) in refused {
        assert!(
            matches!(
                m.remote.send_osd_sync(c),
                Err(ControlError::InvalidRequest(_))
            ),
            "{why}"
        );
    }
    assert!(m.sent().is_empty());
    assert!(m.hellos().is_empty());

    let accepted = [
        content(128),
        caption_with(&|c| c.font = "f".repeat(31)),
        caption_with(&|c| c.text = "t".repeat(360)),
        logo(-1),
        logo(4),
        image(Vec::new(), 100),
    ];
    for c in accepted {
        m.remote.send_osd_sync(c).expect("accepted");
    }
}

#[test]
fn images_are_described_in_the_sync_and_served_as_described() {
    let m = mesh();
    let mut c = content(1);
    c.images = Some(images());
    let seq = m.remote.send_osd_sync(c).expect("sent");
    let p = m.sent().remove(0);
    assert_eq!(u16_at(&p, 2), seq);
    assert_eq!(p[20], 0x0D, "the slots follow from the images");
    let port = u16_at(&p, 22);
    assert_ne!(port, 0);
    for (i, image) in images().iter().enumerate() {
        let at = 32 + i * 12;
        assert_eq!(u32_at(&p, at), image.png.len() as u32, "slot {i}");
        assert_eq!(u32_at(&p, at + 4), crc32(&image.png), "slot {i}");
        assert_eq!(p[at + 8], image.percent, "slot {i}");

        let mut s = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).expect("connect");
        write!(s, "GET /osd-sync/slot{}.png HTTP/1.1\r\n\r\n", i + 1).expect("send");
        let mut response = Vec::new();
        s.read_to_end(&mut response).expect("read");
        if image.png.is_empty() {
            assert!(response.starts_with(b"HTTP/1.1 404 "), "slot {i}");
        } else {
            assert!(response.ends_with(&image.png), "slot {i}");
        }
    }

    // Content without images stops describing them, and asking for a
    // LOGO_SLOTS sync without any does not bring them back.
    m.tap.clear();
    let mut c = content(1);
    c.what |= OsdSyncWhat::LOGO_SLOTS;
    m.remote.send_osd_sync(c).expect("sent");
    let p = m.sent().remove(0);
    assert_eq!(p[20], 0x05);
    assert!(p[32..].iter().all(|&b| b == 0));
}

#[test]
fn closing_drops_the_sync() {
    let m = mesh();
    let mut c = content(1);
    c.images = Some(images());
    m.remote.send_osd_sync(c).expect("sent");
    m.remote.close();
    let osd = lock(&m.remote.shared.osd);
    assert!(!osd.holds());
    assert!(osd.server.is_none());
}

#[test]
fn a_new_ask_replaces_what_was_still_queued_for_the_unit() {
    let m = mesh();
    let seq = m.remote.send_osd_sync(content(3)).expect("sent");
    m.request(m.a, seq, 0x05, &[0, 1, 2], m.uid());
    m.request(m.b, seq, 0x05, &[0], m.uid());
    m.request(m.a, seq, 0x05, &[2], m.uid());
    m.tap.clear();
    assert_eq!(m.remote.shared.osd_pass(), 2, "one for each unit");
    let sent: Vec<_> = m
        .sent()
        .iter()
        .map(|p| (target(p), u16_at(p, 20)))
        .collect();
    assert_eq!(sent, [(m.b, 0), (m.a, 2)]);
}

#[test]
fn the_units_take_turns() {
    let m = mesh();
    let seq = m.remote.send_osd_sync(content(3)).expect("sent");
    m.request(m.a, seq, 0x05, &[0, 1, 2], m.uid());
    m.request(m.b, seq, 0x05, &[0, 1, 2], m.uid());
    m.tap.clear();
    assert_eq!(m.remote.shared.osd_pass(), SETTINGS_PER_PASS);
    assert_eq!(m.remote.shared.osd_pass(), 2);
    let order: Vec<_> = m
        .sent()
        .iter()
        .map(|p| (target(p), u16_at(p, 20)))
        .collect();
    let (a, b) = (m.a, m.b);
    assert_eq!(order, [(a, 0), (b, 0), (a, 1), (b, 1), (a, 2), (b, 2)]);
}

#[test]
fn management_may_ask_a_sync_for_another_unit() {
    let m = mesh();
    m.remote.send_osd_sync(content(2)).expect("sent");
    let manager = uid_n(0x23);
    m.feed(
        manager,
        op::SYS_HELLO,
        &hello_payload(0x2F, "Mgr", "MG0023", "4.8.0", DeviceFeature::MANAGER),
    );
    let ask_for = |from: DeviceUid, unit: DeviceUid| {
        let mut p = vec![2, 1, 0, 0];
        p.extend_from_slice(unit.as_bytes());
        p.extend_from_slice(&[0x01, 0]);
        p.extend_from_slice(&[0; 32]);
        m.feed(from, op::V2IP_OSD_SYNC, &p);
    };
    let ask = |from: DeviceUid| ask_for(from, m.b);
    m.tap.clear();
    ask_for(manager, DeviceUid::ZERO);
    ask_for(manager, m.uid());
    assert!(
        m.sent().is_empty(),
        "a sync for no unit, or for this client"
    );
    ask(m.a);
    assert!(m.sent().is_empty(), "a unit asked for another");
    ask(manager);
    let sent = m.sent();
    assert_eq!(sent.len(), 1);
    assert_eq!((sent[0][1], target(&sent[0])), (0, m.b));
}

#[test]
fn a_resend_goes_only_to_a_unit_taking_overlay_syncs() {
    let m = mesh();
    m.remote.send_osd_sync(content(2)).expect("sent");
    let c = uid_n(0x24);
    m.feed(
        c,
        op::SYS_HELLO,
        &hello_payload(0x2F, "OneIP", "OI0024", "4.8.0", DeviceFeature::V2IP_SINK),
    );
    m.tap.clear();
    assert!(matches!(
        m.remote.resend_osd_sync(c),
        Err(ControlError::Unsupported(_))
    ));
    assert!(m.sent().is_empty());
}

/// The message frames go to the unit they name, the text counted and not
/// terminated, and a request and a take-down carry nothing else.
#[test]
fn a_message_names_the_unit_it_is_for() {
    let m = mesh();
    let text = "x".repeat(OSD_MESSAGE_TEXT_MAX);
    m.remote
        .show_osd_message(m.a, 5, OsdMessagePriority::NORMAL, &text)
        .expect("a message the unit takes");
    m.remote
        .clear_osd_message(m.b)
        .expect("a unit with an output");
    m.remote
        .request_osd_message(m.a)
        .expect("a unit with an output");

    let sent = m.sent();
    assert_eq!(sent.len(), 3);
    let head = |kind: u8, unit: DeviceUid| {
        let mut p = vec![2, kind, 0, 0];
        p.extend_from_slice(unit.as_bytes());
        p
    };
    let mut show = head(5, m.a);
    show.extend_from_slice(&[1, 5, 1, 0, 255, 0]);
    show.extend_from_slice(text.as_bytes());
    assert_eq!(sent[0], show);
    let mut clear = head(5, m.b);
    clear.extend_from_slice(&[0; 6]);
    assert_eq!(sent[1], clear);
    let mut request = head(4, m.a);
    request.extend_from_slice(&[0; 6]);
    assert_eq!(sent[2], request);
}

/// What a unit would refuse, or has no output to show, is not sent.
#[test]
fn a_message_a_unit_would_refuse_is_not_sent() {
    let m = mesh();
    let source = uid_n(0x23);
    m.feed(
        source,
        op::SYS_HELLO,
        &hello_payload(0x2F, "OneIP", "OI0023", "4.8.0", DeviceFeature::V2IP_SOURCE),
    );
    m.tap.clear();
    let normal = OsdMessagePriority::NORMAL;
    let show =
        |unit, logo, priority, text: &str| m.remote.show_osd_message(unit, logo, priority, text);
    let long = "x".repeat(OSD_MESSAGE_TEXT_MAX + 1);
    for (what, result) in [
        ("logo 6", show(m.a, 6, normal, "")),
        ("logo -2", show(m.a, -2, normal, "")),
        (
            "priority 3",
            show(m.a, 0, OsdMessagePriority::from_wire(3), ""),
        ),
        ("a long text", show(m.a, 0, normal, &long)),
        ("a line break", show(m.a, 0, normal, "two\nlines")),
    ] {
        assert!(
            matches!(result, Err(ControlError::InvalidRequest(_))),
            "{what}: {result:?}"
        );
    }
    for result in [
        show(source, 0, normal, "hi"),
        m.remote.clear_osd_message(source),
        m.remote.request_osd_message(source),
    ] {
        assert!(
            matches!(result, Err(ControlError::Unsupported(_))),
            "{result:?}"
        );
    }
    assert!(matches!(
        m.remote.request_osd_message(uid_n(0x24)),
        Err(ControlError::UnknownDevice(_))
    ));
    assert!(m.sent().is_empty());
}
