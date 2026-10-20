// Author: Lars Op den Kamp (lars@opdenkamp-it.nl)
// Copyright (c) 2026 Op den Kamp IT Solutions

//! The overlay sync layout, which the decoder and the builders share.
//!
//! Every frame opens with a 20-byte header: the payload version, the kind,
//! the sync's `seq` and the unit it is for. The message kinds carry the
//! message on one unit's output instead, with a `seq` of 0.

use crate::types::{OsdSyncEntry, OsdSyncSlot, OsdSyncValue, OSD_SYNC_SLOTS};

use super::enums::{OsdMessageFlag, OsdMessagePriority, OsdSyncWhat};
use super::payload::append_fixed_str;
use super::uid::DeviceUid;

/// The payload version every frame opens with; a unit drops any other.
pub(crate) const OSD_SYNC_VERSION: u8 = 2;

/// Width of the header.
pub(crate) const OSD_HEADER_SIZE: usize = 20;

/// The kind at byte 1.
pub(crate) mod osd_kind {
    pub(crate) const SYNC: u8 = 0;
    pub(crate) const REQUEST: u8 = 1;
    pub(crate) const SETTING: u8 = 2;
    pub(crate) const REPORT: u8 = 3;
    pub(crate) const MESSAGE_REQUEST: u8 = 4;
    pub(crate) const MESSAGE_SET: u8 = 5;
    pub(crate) const MESSAGE_STATE: u8 = 6;
}

/// Width of a sync: the header, what, the HTTP port, source address,
/// records, and four 12-byte slots from 32.
pub(crate) const OSD_SYNC_SIZE: usize = 80;
pub(crate) const OSD_SYNC_SLOTS_AT: usize = 32;
pub(crate) const OSD_SYNC_SLOT_SIZE: usize = 12;

/// Width of a request without the source behind it, which units on an older
/// module send.
pub(crate) const OSD_REQUEST_SIZE: usize = 38;
pub(crate) const OSD_REQUEST_SOURCE_AT: usize = 38;

/// Width of a setting's own fields, where its body starts.
pub(crate) const OSD_SETTING_BODY_AT: usize = 30;

/// The setting type at byte 24.
pub(crate) mod osd_setting {
    pub(crate) const CAPTION: u8 = 0;
    pub(crate) const LOGO: u8 = 1;
    pub(crate) const BACKGROUND: u8 = 2;
}

/// A caption body: a 32-byte font, then size, x, y, wrap, align, format,
/// colour and the text's length, with the text behind it.
pub(crate) const OSD_CAPTION_SIZE: usize = 48;
pub(crate) const OSD_FONT_LEN: usize = 32;
pub(crate) const OSD_LOGO_SIZE: usize = 6;
pub(crate) const OSD_BACKGROUND_SIZE: usize = 4;

/// Width of a report: the header, state, written, refused and a 64-byte
/// reason.
pub(crate) const OSD_REPORT_SIZE: usize = 90;

/// Width of a message frame without its text: the header, flags, logo,
/// priority, a reserved byte and the text's length.
pub(crate) const OSD_MESSAGE_SIZE: usize = 26;

fn header(kind: u8, seq: u16, target: DeviceUid, size: usize) -> Vec<u8> {
    let mut p = Vec::with_capacity(size);
    p.extend_from_slice(&[OSD_SYNC_VERSION, kind]);
    p.extend_from_slice(&seq.to_le_bytes());
    p.extend_from_slice(target.as_bytes());
    p
}

/// Builds a sync, with every reserved byte zero.
///
/// The source address is left unspecified, which a unit reads as the
/// frame's own source. The slots are written only when `what` copies them.
pub(crate) fn build_osd_sync(
    seq: u16,
    target: DeviceUid,
    what: OsdSyncWhat,
    http_port: u16,
    records: u16,
    slots: &[OsdSyncSlot; OSD_SYNC_SLOTS],
) -> Vec<u8> {
    let mut p = header(osd_kind::SYNC, seq, target, OSD_SYNC_SIZE);
    p.extend_from_slice(&[what.bits(), 0]);
    p.extend_from_slice(&http_port.to_le_bytes());
    p.extend_from_slice(&[0; 4]);
    p.extend_from_slice(&records.to_le_bytes());
    p.resize(OSD_SYNC_SLOTS_AT, 0);
    if what.has(OsdSyncWhat::LOGO_SLOTS) {
        for slot in slots {
            p.extend_from_slice(&slot.bytes.to_le_bytes());
            p.extend_from_slice(&slot.crc32.to_le_bytes());
            p.extend_from_slice(&[slot.percent, 0, 0, 0]);
        }
    }
    p.resize(OSD_SYNC_SIZE, 0);
    p
}

/// Builds setting `index` of a sync of `records`, for the unit `target`.
///
/// A caption's font and text are written as given, so the caller keeps the
/// font under 32 bytes and the text to what a `u16` length counts.
pub(crate) fn build_osd_setting(
    seq: u16,
    target: DeviceUid,
    index: u16,
    records: u16,
    entry: &OsdSyncEntry,
) -> Vec<u8> {
    let kind = match entry.value {
        OsdSyncValue::Caption(_) => osd_setting::CAPTION,
        OsdSyncValue::Logo(_) => osd_setting::LOGO,
        OsdSyncValue::Background(_) => osd_setting::BACKGROUND,
    };
    let mut p = header(osd_kind::SETTING, seq, target, OSD_SETTING_BODY_AT);
    p.extend_from_slice(&index.to_le_bytes());
    p.extend_from_slice(&records.to_le_bytes());
    p.extend_from_slice(&[kind, entry.size.to_wire()]);
    p.extend_from_slice(&entry.layers.to_le_bytes());
    p.extend_from_slice(&entry.set.to_le_bytes());
    match &entry.value {
        OsdSyncValue::Caption(c) => {
            append_fixed_str(&mut p, &c.font, OSD_FONT_LEN);
            p.extend_from_slice(&c.size.to_le_bytes());
            p.extend_from_slice(&c.x.to_le_bytes());
            p.extend_from_slice(&c.y.to_le_bytes());
            p.extend_from_slice(&c.wrap.to_le_bytes());
            p.extend_from_slice(&[c.align.to_wire(), c.format.to_wire()]);
            p.extend_from_slice(&c.colour.to_le_bytes());
            let text = c.text.as_bytes();
            p.extend_from_slice(&(text.len() as u16).to_le_bytes());
            p.extend_from_slice(text);
        }
        OsdSyncValue::Logo(l) => {
            p.extend_from_slice(&[l.logo as u8, 0]);
            p.extend_from_slice(&l.x.to_le_bytes());
            p.extend_from_slice(&l.y.to_le_bytes());
        }
        OsdSyncValue::Background(colour) => p.extend_from_slice(&colour.to_le_bytes()),
    }
    p
}

/// Builds a message frame of `kind` for the output of `target`, the text
/// written as given and not terminated.
pub(crate) fn build_osd_message(
    kind: u8,
    target: DeviceUid,
    flags: OsdMessageFlag,
    logo: i8,
    priority: OsdMessagePriority,
    text: &str,
) -> Vec<u8> {
    let mut p = header(kind, 0, target, OSD_MESSAGE_SIZE + text.len());
    p.extend_from_slice(&[flags.bits(), logo as u8, priority.to_wire(), 0]);
    p.extend_from_slice(&(text.len() as u16).to_le_bytes());
    p.extend_from_slice(text.as_bytes());
    p
}

/// The CRC-32 a unit checks a logo image against: the zlib one, reflected
/// polynomial 0xEDB88320, starting from and finishing with all ones.
pub(crate) fn crc32(data: &[u8]) -> u32 {
    let mut crc = u32::MAX;
    for &byte in data {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            crc = (crc >> 1) ^ (0xEDB8_8320 & (crc & 1).wrapping_neg());
        }
    }
    !crc
}

#[cfg(test)]
mod tests {
    //! The offsets here restate the layout the decoder reads, which was
    //! composed from the same description, so they rule out a slip between
    //! the two rather than a shared misreading.

    use super::*;
    use crate::testing::uid_n;
    use crate::types::{OsdCaption, OsdLogo};
    use crate::wire::{OsdCaptionAlign, OsdCaptionFormat, OsdSyncSize};

    fn u16_at(p: &[u8], at: usize) -> u16 {
        u16::from_le_bytes([p[at], p[at + 1]])
    }

    fn u32_at(p: &[u8], at: usize) -> u32 {
        u32::from_le_bytes([p[at], p[at + 1], p[at + 2], p[at + 3]])
    }

    fn slots() -> [OsdSyncSlot; OSD_SYNC_SLOTS] {
        core::array::from_fn(|i| OsdSyncSlot {
            bytes: 0x0102_0300 + i as u32,
            crc32: 0xC0DE_F000 + i as u32,
            percent: 90 + i as u8,
        })
    }

    #[test]
    fn a_sync_puts_each_field_at_its_offset() {
        let target = uid_n(7);
        let p = build_osd_sync(
            0xA1B2,
            target,
            OsdSyncWhat::from_bits(0x0B),
            0x1F90,
            0x0123,
            &slots(),
        );
        assert_eq!(p.len(), OSD_SYNC_SIZE);
        assert_eq!(&p[..2], &[2, 0]);
        assert_eq!(u16_at(&p, 2), 0xA1B2);
        assert_eq!(&p[4..20], target.as_bytes());
        assert_eq!(&p[20..22], &[0x0B, 0]);
        assert_eq!(u16_at(&p, 22), 0x1F90);
        assert_eq!(&p[24..28], &[0; 4], "the address is the frame's own");
        assert_eq!(u16_at(&p, 28), 0x0123);
        assert_eq!(&p[30..32], &[0; 2]);
        for i in 0..OSD_SYNC_SLOTS {
            let at = 32 + i * 12;
            assert_eq!(u32_at(&p, at), 0x0102_0300 + i as u32, "slot {i}");
            assert_eq!(u32_at(&p, at + 4), 0xC0DE_F000 + i as u32, "slot {i}");
            assert_eq!(&p[at + 8..at + 12], &[90 + i as u8, 0, 0, 0], "slot {i}");
        }
    }

    #[test]
    fn a_sync_copying_no_images_describes_no_slots() {
        let p = build_osd_sync(1, DeviceUid::ZERO, OsdSyncWhat::CAPTIONS, 0, 3, &slots());
        assert_eq!(p.len(), OSD_SYNC_SIZE);
        assert!(p[32..].iter().all(|&b| b == 0));
    }

    fn entry(value: OsdSyncValue) -> OsdSyncEntry {
        OsdSyncEntry {
            size: OsdSyncSize::SMALL,
            layers: 0x2010,
            set: 0x0155,
            value,
        }
    }

    /// The setting's own fields, which every type shares.
    fn assert_setting_head(p: &[u8], kind: u8, target: DeviceUid) {
        assert_eq!(&p[..2], &[2, 2]);
        assert_eq!(u16_at(p, 2), 0xA1B2);
        assert_eq!(&p[4..20], target.as_bytes());
        assert_eq!(u16_at(p, 20), 0x0105);
        assert_eq!(u16_at(p, 22), 0x0180);
        assert_eq!(p[24], kind);
        assert_eq!(p[25], 2);
        assert_eq!(u16_at(p, 26), 0x2010);
        assert_eq!(u16_at(p, 28), 0x0155);
    }

    #[test]
    fn a_caption_setting_carries_its_text_exactly() {
        let target = uid_n(9);
        let caption = OsdCaption {
            font: "DejaVu Sans".to_owned(),
            size: 0x0124,
            x: -0x0203,
            y: 0x0405,
            wrap: -2,
            align: OsdCaptionAlign::RIGHT,
            format: OsdCaptionFormat::MARKDOWN,
            colour: 0x00A1_B2C3,
            text: "No signal on **Input 3**".to_owned(),
        };
        let text = caption.text.clone();
        let p = build_osd_setting(
            0xA1B2,
            target,
            0x0105,
            0x0180,
            &entry(OsdSyncValue::Caption(caption)),
        );
        assert_setting_head(&p, 0, target);
        let body = &p[30..];
        assert_eq!(&body[..11], b"DejaVu Sans");
        assert!(body[11..32].iter().all(|&b| b == 0));
        assert_eq!(u16_at(body, 32), 0x0124);
        assert_eq!(u16_at(body, 34) as i16, -0x0203);
        assert_eq!(u16_at(body, 36), 0x0405);
        assert_eq!(u16_at(body, 38) as i16, -2);
        assert_eq!(&body[40..42], &[2, 1]);
        assert_eq!(u32_at(body, 42), 0x00A1_B2C3);
        assert_eq!(usize::from(u16_at(body, 46)), text.len());
        assert_eq!(&body[48..], text.as_bytes(), "the text has no terminator");
    }

    #[test]
    fn a_logo_and_a_background_setting_fill_their_bodies() {
        let target = uid_n(9);
        let logo = OsdLogo {
            logo: -1,
            x: 0x0302,
            y: -0x0405,
        };
        let p = build_osd_setting(
            0xA1B2,
            target,
            0x0105,
            0x0180,
            &entry(OsdSyncValue::Logo(logo)),
        );
        assert_setting_head(&p, 1, target);
        assert_eq!(p.len(), 30 + OSD_LOGO_SIZE);
        assert_eq!(&p[30..32], &[0xFF, 0]);
        assert_eq!(u16_at(&p, 32), 0x0302);
        assert_eq!(u16_at(&p, 34) as i16, -0x0405);

        let p = build_osd_setting(
            0xA1B2,
            target,
            0x0105,
            0x0180,
            &entry(OsdSyncValue::Background(0x00C1_D2E3)),
        );
        assert_setting_head(&p, 2, target);
        assert_eq!(p.len(), 30 + OSD_BACKGROUND_SIZE);
        assert_eq!(u32_at(&p, 30), 0x00C1_D2E3);
    }

    #[test]
    fn a_message_puts_each_field_at_its_offset() {
        let target = uid_n(9);
        let p = build_osd_message(
            osd_kind::MESSAGE_SET,
            target,
            OsdMessageFlag::SET,
            -1,
            OsdMessagePriority::TOP,
            "Back at <b>3</b>",
        );
        assert_eq!(&p[..4], &[2, 5, 0, 0]);
        assert_eq!(&p[4..20], target.as_bytes());
        assert_eq!(&p[20..24], &[1, 0xFF, 2, 0]);
        assert_eq!(usize::from(u16_at(&p, 24)), "Back at <b>3</b>".len());
        assert_eq!(&p[26..], b"Back at <b>3</b>", "the text has no terminator");
    }

    /// The check value every CRC-32 catalogue gives for this variant, which
    /// predates this code.
    #[test]
    fn the_crc_is_the_zlib_one() {
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
        assert_eq!(crc32(b""), 0);
    }
}
