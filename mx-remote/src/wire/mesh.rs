// Author: Lars Op den Kamp (lars@opdenkamp-it.nl)
// Copyright (c) 2026 Op den Kamp IT Solutions

//! The layouts of the mesh module's two opcodes, which the decoders and the
//! builders share.
//!
//! `MESH_REPLACE` (0x56) has a unit outside any mesh replace an offline unit
//! of one. Every frame opens with an 8-byte header: the payload version, the
//! kind, and for a list of units the index of its first record, the length of
//! the whole list and its flags.
//!
//! `MESH_CONFIG` (0x57) carries a unit's kept settings. Every frame opens with
//! a 32-byte header: the payload version, the kind, the flags, the offset of
//! this frame's bytes in the settings, the settings' length and crc32, and the
//! unit the settings belong to at 16.

use crate::types::{
    MeshBay, MeshPort, MeshReplaceUnit, MeshSetting, PortMetadataField, PORT_METADATA_FIELDS_MAX,
    PORT_METADATA_KEY_MAX, PORT_METADATA_MAX, PORT_METADATA_VALUE_MAX,
};

use super::enums::{DeviceFeature, MeshReplaceResult};
use super::payload::append_fixed_str;
use super::uid::DeviceUid;

/// The payload version every `MESH_REPLACE` frame opens with; a unit drops
/// any other.
pub(crate) const MESH_REPLACE_VERSION: u8 = 1;

/// Width of the `MESH_REPLACE` header.
pub(crate) const MESH_REPLACE_HEADER_SIZE: usize = 8;

/// The `MESH_REPLACE` kind at byte 1.
pub(crate) mod replace_kind {
    pub(crate) const QUERY: u8 = 0;
    pub(crate) const UNITS: u8 = 1;
    pub(crate) const REQUEST: u8 = 2;
    pub(crate) const RESULT: u8 = 3;
}

/// The list flag at byte 6: the units listed are the sender's offline
/// controller.
pub(crate) const MESH_REPLACE_FLAG_CONTROLLER: u8 = 1 << 0;

/// Width of one record of a list: uid, features, input and output port, and
/// each port's 16-byte name.
pub(crate) const MESH_REPLACE_UNIT_SIZE: usize = 56;
pub(crate) const MESH_REPLACE_NAME_LEN: usize = 16;

/// How long a unit outside a mesh keeps a list after its last frame, and
/// how recently a unit has to have been listed for a request to replace it.
pub(crate) const MESH_REPLACE_KEPT: std::time::Duration = std::time::Duration::from_secs(30);

/// The most records one frame of a list carries.
pub(crate) const MESH_REPLACE_UNITS_PER_FRAME: usize = 24;

/// Width of a request or result: the header, the unit, the result and seven
/// reserved bytes.
pub(crate) const MESH_REPLACE_TARGET_SIZE: usize = 32;

/// The payload version every `MESH_CONFIG` frame opens with; a unit drops any
/// other.
pub(crate) const MESH_CONFIG_VERSION: u8 = 1;

/// Width of the `MESH_CONFIG` header, which is the whole of a hello and a
/// request.
pub(crate) const MESH_CONFIG_HEADER_SIZE: usize = 32;

/// The `MESH_CONFIG` kind at byte 1.
pub(crate) mod config_kind {
    pub(crate) const HELLO: u8 = 0;
    pub(crate) const REQUEST: u8 = 1;
    pub(crate) const DUMP: u8 = 2;
    pub(crate) const SET: u8 = 3;
    pub(crate) const BAYS_REQUEST: u8 = 4;
    pub(crate) const BAYS: u8 = 5;
    pub(crate) const METADATA: u8 = 6;
    pub(crate) const METADATA_INDEX: u8 = 7;
    pub(crate) const METADATA_REQUEST: u8 = 8;
    pub(crate) const SETUP: u8 = 9;
}

/// The flag at byte 2 of a dump: the settings are a copy the sender keeps of
/// the unit named, not its own.
pub(crate) const MESH_CONFIG_FLAG_COPY: u8 = 1 << 0;

/// The flag at byte 2 of a hello, or of a dump of the sender's own settings:
/// the sender lost its settings and holds defaults. A controller that keeps
/// a record of it sends that record back as a set.
pub(crate) const MESH_CONFIG_FLAG_LOST: u8 = 1 << 1;

/// The flag at byte 2 of a port's metadata or a request for it: the port is
/// the unit's input, and without it its output.
pub(crate) const MESH_CONFIG_FLAG_INPUT: u8 = 1 << 2;

/// The most settings bytes one frame carries.
pub(crate) const MESH_CONFIG_FRAME_MAX: usize = 1024;

/// Builds the 8-byte `MESH_REPLACE` header.
fn replace_header(kind: u8, first: u16, total: u16, flags: u8) -> Vec<u8> {
    let mut p = Vec::with_capacity(MESH_REPLACE_HEADER_SIZE);
    p.extend_from_slice(&[MESH_REPLACE_VERSION, kind]);
    p.extend_from_slice(&first.to_le_bytes());
    p.extend_from_slice(&total.to_le_bytes());
    p.extend_from_slice(&[flags, 0]);
    p
}

/// Builds a `MESH_REPLACE` query: the header alone.
pub(crate) fn build_mesh_replace_query() -> Vec<u8> {
    replace_header(replace_kind::QUERY, 0, 0, 0)
}

/// Builds the frames of a `MESH_REPLACE` list, in order: each the header, with
/// the index of its first record and the length of the whole list, then at
/// most [`MESH_REPLACE_UNITS_PER_FRAME`] records. An empty list is one frame
/// of no records.
///
/// `units` holds at most `u16::MAX` units, which the caller checks.
pub(crate) fn build_mesh_replace_units(
    units: &[MeshReplaceUnit],
    offline_controller: bool,
) -> Vec<Vec<u8>> {
    let flags = if offline_controller {
        MESH_REPLACE_FLAG_CONTROLLER
    } else {
        0
    };
    let total = units.len() as u16;
    let frame = |first: usize, chunk: &[MeshReplaceUnit]| {
        let mut p = replace_header(replace_kind::UNITS, first as u16, total, flags);
        for unit in chunk {
            p.extend_from_slice(unit.uid.as_bytes());
            p.extend_from_slice(&unit.features.bits().to_le_bytes());
            p.extend_from_slice(&unit.input.to_le_bytes());
            p.extend_from_slice(&unit.output.to_le_bytes());
            append_fixed_str(&mut p, &unit.input_name, MESH_REPLACE_NAME_LEN);
            append_fixed_str(&mut p, &unit.output_name, MESH_REPLACE_NAME_LEN);
        }
        p
    };
    if units.is_empty() {
        return vec![frame(0, &[])];
    }
    units
        .chunks(MESH_REPLACE_UNITS_PER_FRAME)
        .enumerate()
        .map(|(i, chunk)| frame(i * MESH_REPLACE_UNITS_PER_FRAME, chunk))
        .collect()
}

/// Builds a `MESH_REPLACE` request or result: the header, the unit, the
/// result (0 in a request) and seven reserved zero bytes.
pub(crate) fn build_mesh_replace_target(
    kind: u8,
    unit: DeviceUid,
    result: MeshReplaceResult,
) -> Vec<u8> {
    let mut p = replace_header(kind, 0, 0, 0);
    p.extend_from_slice(unit.as_bytes());
    p.push(result.to_wire());
    p.resize(MESH_REPLACE_TARGET_SIZE, 0);
    p
}

/// Builds a `MESH_CONFIG` header.
fn config_header(
    kind: u8,
    flags: u8,
    offset: u16,
    total: u16,
    crc: u32,
    subject: DeviceUid,
) -> Vec<u8> {
    let mut p = Vec::with_capacity(MESH_CONFIG_HEADER_SIZE);
    p.extend_from_slice(&[MESH_CONFIG_VERSION, kind, flags, 0]);
    p.extend_from_slice(&offset.to_le_bytes());
    p.extend_from_slice(&total.to_le_bytes());
    p.extend_from_slice(&crc.to_le_bytes());
    p.extend_from_slice(&[0; 4]);
    p.extend_from_slice(subject.as_bytes());
    p
}

/// Builds a `MESH_CONFIG` hello: the length and crc32 of the sender's own
/// settings, `encoded`, which the sender names as their unit.
pub(crate) fn build_mesh_config_hello(own: DeviceUid, encoded: &[u8]) -> Vec<u8> {
    let crc = super::osd::crc32(encoded);
    config_header(config_kind::HELLO, 0, 0, encoded.len() as u16, crc, own)
}

/// Builds a `MESH_CONFIG` request for the settings of `subject`: the header
/// alone, with no offset, length or crc.
pub(crate) fn build_mesh_config_request(subject: DeviceUid) -> Vec<u8> {
    config_header(config_kind::REQUEST, 0, 0, 0, 0, subject)
}

/// Builds the frames of a `MESH_CONFIG` dump or set, in order: each the
/// header, with the offset of its bytes, then at most
/// [`MESH_CONFIG_FRAME_MAX`] bytes of `encoded`. Settings of no bytes are one
/// frame.
///
/// `encoded` comes from [`encode_mesh_settings`], which bounds its length.
pub(crate) fn build_mesh_config_pages(
    kind: u8,
    flags: u8,
    subject: DeviceUid,
    encoded: &[u8],
) -> Vec<Vec<u8>> {
    let total = encoded.len() as u16;
    let crc = super::osd::crc32(encoded);
    let frame = |offset: usize, chunk: &[u8]| {
        let mut p = config_header(kind, flags, offset as u16, total, crc, subject);
        p.extend_from_slice(chunk);
        p
    };
    if encoded.is_empty() {
        return vec![frame(0, &[])];
    }
    encoded
        .chunks(MESH_CONFIG_FRAME_MAX)
        .enumerate()
        .map(|(i, chunk)| frame(i * MESH_CONFIG_FRAME_MAX, chunk))
        .collect()
}

/// Encodes settings as a unit carries them: each a u8 name length, the name,
/// a u8 value length and the value, none terminated.
///
/// `None` for a name or value over 255 bytes, or one holding a NUL, which a
/// unit cannot store, and for settings over
/// [`MESH_CONFIG_MAX`](crate::MESH_CONFIG_MAX) bytes, which no unit takes.
pub(crate) fn encode_mesh_settings(settings: &[MeshSetting]) -> Option<Vec<u8>> {
    let mut p = Vec::new();
    for setting in settings {
        for text in [&setting.name, &setting.value] {
            let bytes = text.as_bytes();
            if bytes.len() > usize::from(u8::MAX) || bytes.contains(&0) {
                return None;
            }
            p.push(bytes.len() as u8);
            p.extend_from_slice(bytes);
        }
    }
    (p.len() <= crate::types::MESH_CONFIG_MAX).then_some(p)
}

/// Reads settings as a unit carries them, `None` for bytes that end inside a
/// pair. A name or value is read as UTF-8, and a sequence that is not is
/// replaced.
pub(crate) fn decode_mesh_settings(mut p: &[u8]) -> Option<Vec<MeshSetting>> {
    let mut settings = Vec::new();
    let take = |p: &mut &[u8]| -> Option<String> {
        let (&len, rest) = p.split_first()?;
        let text = rest.get(..usize::from(len))?;
        *p = &rest[usize::from(len)..];
        Some(String::from_utf8_lossy(text).into_owned())
    };
    while !p.is_empty() {
        let name = take(&mut p)?;
        let value = take(&mut p)?;
        settings.push(MeshSetting { name, value });
    }
    Some(settings)
}

/// Width of one `mesh_bays_entry`: the mode, a reserved byte, the bay number,
/// the unit's features and its uid, on natural alignment with no padding.
pub(crate) const MESH_BAYS_ENTRY_SIZE: usize = 24;

/// The most entries one `MESH_CONFIG` bays frame carries.
pub(crate) const MESH_BAYS_PER_FRAME: usize = 42;

/// The mode byte of an input entry; an output's is 0.
const MESH_BAYS_INPUT: u8 = 1;

/// Builds a `MESH_CONFIG` setup frame, which marks member `member`'s setup
/// completed or not: the header alone, the mark in the length field.
pub(crate) fn build_mesh_setup(member: DeviceUid, completed: bool) -> Vec<u8> {
    config_header(config_kind::SETUP, 0, 0, u16::from(completed), 0, member)
}

/// Builds a `MESH_CONFIG` bays request for the numbering of controller
/// `subject`: the header alone.
pub(crate) fn build_mesh_bays_request(subject: DeviceUid) -> Vec<u8> {
    config_header(config_kind::BAYS_REQUEST, 0, 0, 0, 0, subject)
}

/// Lays out a numbering's entries as a unit carries them.
pub(crate) fn encode_mesh_bays(bays: &[MeshBay]) -> Vec<u8> {
    let mut p = Vec::with_capacity(bays.len() * MESH_BAYS_ENTRY_SIZE);
    for bay in bays {
        p.push(if bay.input { MESH_BAYS_INPUT } else { 0 });
        p.push(0);
        p.extend_from_slice(&bay.bay.to_le_bytes());
        p.extend_from_slice(&bay.features.bits().to_le_bytes());
        p.extend_from_slice(bay.unit.as_bytes());
    }
    p
}

/// Builds the frames of a `MESH_CONFIG` bays answer, in order: each the
/// header, with the offset of its bytes, then at most
/// [`MESH_BAYS_PER_FRAME`] entries. A numbering of no bays is one frame.
///
/// `encoded` comes from [`encode_mesh_bays`] of at most
/// [`MESH_BAYS_MAX`](crate::MESH_BAYS_MAX) entries, which the caller checks.
pub(crate) fn build_mesh_bays_pages(flags: u8, subject: DeviceUid, encoded: &[u8]) -> Vec<Vec<u8>> {
    let total = encoded.len() as u16;
    let crc = super::osd::crc32(encoded);
    let frame = |offset: usize, chunk: &[u8]| {
        let mut p = config_header(config_kind::BAYS, flags, offset as u16, total, crc, subject);
        p.extend_from_slice(chunk);
        p
    };
    if encoded.is_empty() {
        return vec![frame(0, &[])];
    }
    let page = MESH_BAYS_PER_FRAME * MESH_BAYS_ENTRY_SIZE;
    encoded
        .chunks(page)
        .enumerate()
        .map(|(i, chunk)| frame(i * page, chunk))
        .collect()
}

/// Reads a numbering's entries, `None` for bytes that end inside one.
pub(crate) fn decode_mesh_bays(p: &[u8]) -> Option<Vec<MeshBay>> {
    if p.len() % MESH_BAYS_ENTRY_SIZE != 0 {
        return None;
    }
    Some(
        p.chunks_exact(MESH_BAYS_ENTRY_SIZE)
            .map(|e| {
                let mut uid = [0; 16];
                uid.copy_from_slice(&e[8..24]);
                MeshBay {
                    input: e[0] == MESH_BAYS_INPUT,
                    bay: u16::from_le_bytes([e[2], e[3]]),
                    features: DeviceFeature::from_bits(u32::from_le_bytes([
                        e[4], e[5], e[6], e[7],
                    ])),
                    unit: DeviceUid::from_array(uid),
                }
            })
            .collect(),
    )
}

/// Whether `key` is a key a unit takes for port metadata: 1 to
/// [`PORT_METADATA_KEY_MAX`] characters of `A-Z`, `a-z`, `0-9`, `_` and `-`.
pub(crate) fn valid_metadata_key(key: &str) -> bool {
    (1..=PORT_METADATA_KEY_MAX).contains(&key.len())
        && key
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

/// Whether `value` is a value a unit takes for port metadata: 1 to
/// [`PORT_METADATA_VALUE_MAX`] bytes of UTF-8, with no control character,
/// `"` or `\`.
pub(crate) fn valid_metadata_value(value: &str) -> bool {
    (1..=PORT_METADATA_VALUE_MAX).contains(&value.len())
        && value
            .bytes()
            .all(|b| b >= 0x20 && b != 0x7F && b != b'"' && b != b'\\')
}

/// Encodes a port's metadata as a unit carries it: each field its key, a
/// NUL, its value and a NUL, in the order given. Order counts, as the crc32
/// is taken over these bytes; a repeated key is carried as it is.
///
/// `None` for an invalid key or value, more than
/// [`PORT_METADATA_FIELDS_MAX`] fields, or more than [`PORT_METADATA_MAX`]
/// bytes, none of which a unit takes.
pub(crate) fn encode_port_metadata(fields: &[PortMetadataField]) -> Option<Vec<u8>> {
    if fields.len() > PORT_METADATA_FIELDS_MAX {
        return None;
    }
    let mut p = Vec::new();
    for field in fields {
        if !valid_metadata_key(&field.key) || !valid_metadata_value(&field.value) {
            return None;
        }
        p.extend_from_slice(field.key.as_bytes());
        p.push(0);
        p.extend_from_slice(field.value.as_bytes());
        p.push(0);
    }
    (p.len() <= PORT_METADATA_MAX).then_some(p)
}

/// Reads a port's metadata as a unit carries it, `None` for anything a unit
/// would refuse: bytes not ending in a field's NUL, a key without a value,
/// an invalid key or value, or more fields or bytes than a unit takes.
pub(crate) fn decode_port_metadata(p: &[u8]) -> Option<Vec<PortMetadataField>> {
    if p.len() > PORT_METADATA_MAX {
        return None;
    }
    if p.is_empty() {
        return Some(Vec::new());
    }
    let body = p.strip_suffix(&[0])?;
    let mut parts = body.split(|&b| b == 0);
    let mut fields = Vec::new();
    while let Some(key) = parts.next() {
        let value = parts.next()?;
        let key = std::str::from_utf8(key).ok()?;
        let value = std::str::from_utf8(value).ok()?;
        if !valid_metadata_key(key) || !valid_metadata_value(value) {
            return None;
        }
        fields.push(PortMetadataField::new(key, value));
    }
    (fields.len() <= PORT_METADATA_FIELDS_MAX).then_some(fields)
}

/// The flags naming a port's direction.
fn port_flags(port: MeshPort) -> u8 {
    if port.input {
        MESH_CONFIG_FLAG_INPUT
    } else {
        0
    }
}

/// Builds a `MESH_CONFIG` metadata frame: the header naming the port, with
/// the length and crc32 of `encoded`, then `encoded` itself. Metadata of no
/// fields, which removes the port's, has length and crc 0.
///
/// `encoded` comes from [`encode_port_metadata`], which bounds its length.
pub(crate) fn build_port_metadata(port: MeshPort, encoded: &[u8]) -> Vec<u8> {
    let crc = super::osd::crc32(encoded);
    let mut p = config_header(
        config_kind::METADATA,
        port_flags(port),
        0,
        encoded.len() as u16,
        crc,
        port.unit,
    );
    p.extend_from_slice(encoded);
    p
}

/// Builds a `MESH_CONFIG` metadata request for `port`: the header alone.
pub(crate) fn build_port_metadata_request(port: MeshPort) -> Vec<u8> {
    config_header(
        config_kind::METADATA_REQUEST,
        port_flags(port),
        0,
        0,
        0,
        port.unit,
    )
}

/// Width of one metadata index entry: the unit, the mode, three reserved
/// bytes and the crc32 of the port's metadata.
pub(crate) const METADATA_INDEX_ENTRY_SIZE: usize = 24;

/// The most entries one metadata index frame carries.
pub(crate) const METADATA_INDEX_PER_FRAME: usize = 48;

/// The width of the generation every metadata index frame's body opens with.
pub(crate) const METADATA_INDEX_GENERATION_SIZE: usize = 4;

/// The mode byte of an input's index entry; an output's is 0.
const METADATA_INDEX_INPUT: u8 = 1;

/// One entry of a metadata index: a port holding metadata, and the crc32 of
/// that metadata as it is carried.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct MetadataIndexEntry {
    pub(crate) port: MeshPort,
    pub(crate) crc: u32,
}

/// Builds the frames of a `MESH_CONFIG` metadata index from `sender`, in
/// order: each the header, with the index of its first entry, the number of
/// entries and the crc32 of them all, then the generation and at most
/// [`METADATA_INDEX_PER_FRAME`] entries. An index of no ports is one frame of
/// no entries, with crc 0.
///
/// `entries` holds at most `u16::MAX` entries, which the caller checks.
pub(crate) fn build_metadata_index_pages(
    sender: DeviceUid,
    generation: u32,
    entries: &[MetadataIndexEntry],
) -> Vec<Vec<u8>> {
    let mut encoded = Vec::with_capacity(entries.len() * METADATA_INDEX_ENTRY_SIZE);
    for entry in entries {
        encoded.extend_from_slice(entry.port.unit.as_bytes());
        encoded.push(if entry.port.input {
            METADATA_INDEX_INPUT
        } else {
            0
        });
        encoded.extend_from_slice(&[0; 3]);
        encoded.extend_from_slice(&entry.crc.to_le_bytes());
    }
    let total = entries.len() as u16;
    let crc = super::osd::crc32(&encoded);
    let frame = |first: usize, chunk: &[u8]| {
        let mut p = config_header(
            config_kind::METADATA_INDEX,
            0,
            first as u16,
            total,
            crc,
            sender,
        );
        p.extend_from_slice(&generation.to_le_bytes());
        p.extend_from_slice(chunk);
        p
    };
    if encoded.is_empty() {
        return vec![frame(0, &[])];
    }
    encoded
        .chunks(METADATA_INDEX_PER_FRAME * METADATA_INDEX_ENTRY_SIZE)
        .enumerate()
        .map(|(i, chunk)| frame(i * METADATA_INDEX_PER_FRAME, chunk))
        .collect()
}

/// Reads the body of a metadata index frame: the generation, then its
/// entries. `None` for a body that is not the generation and whole entries.
pub(crate) fn decode_metadata_index(p: &[u8]) -> Option<(u32, Vec<MetadataIndexEntry>)> {
    let g = p.get(..METADATA_INDEX_GENERATION_SIZE)?;
    let generation = u32::from_le_bytes([g[0], g[1], g[2], g[3]]);
    let rest = &p[METADATA_INDEX_GENERATION_SIZE..];
    if rest.len() % METADATA_INDEX_ENTRY_SIZE != 0 {
        return None;
    }
    let entries = rest
        .chunks_exact(METADATA_INDEX_ENTRY_SIZE)
        .map(|e| {
            let mut uid = [0; 16];
            uid.copy_from_slice(&e[..16]);
            MetadataIndexEntry {
                port: MeshPort {
                    unit: DeviceUid::from_array(uid),
                    input: e[16] == METADATA_INDEX_INPUT,
                },
                crc: u32::from_le_bytes([e[20], e[21], e[22], e[23]]),
            }
        })
        .collect();
    Some((generation, entries))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setting(name: &str, value: &str) -> MeshSetting {
        MeshSetting {
            name: name.to_owned(),
            value: value.to_owned(),
        }
    }

    /// Composed from the layout these builders encode, so it rules out a
    /// slip, not a shared misreading of that layout.
    #[test]
    fn settings_are_length_prefixed_pairs() {
        let encoded =
            encode_mesh_settings(&[setting("led.status", "0"), setting("ir.profile", "")]).unwrap();
        assert_eq!(encoded, b"\x0aled.status\x010\x0air.profile\x00");
        assert_eq!(
            decode_mesh_settings(&encoded).unwrap(),
            [setting("led.status", "0"), setting("ir.profile", "")]
        );
        assert_eq!(decode_mesh_settings(b"\x02ab\x03cd"), None, "a short value");
        assert_eq!(
            decode_mesh_settings(b"\x02ab"),
            None,
            "a name without a value"
        );
    }

    #[test]
    fn settings_a_unit_cannot_store_are_refused() {
        let long = "x".repeat(256);
        assert!(encode_mesh_settings(&[setting(&long, "")]).is_none());
        assert!(encode_mesh_settings(&[setting("a", &long)]).is_none());
        assert!(encode_mesh_settings(&[setting("a", &"x".repeat(255))]).is_some());
        assert!(encode_mesh_settings(&[setting("a\0b", "")]).is_none());
        // 1984 bytes and no more: eight pairs of 248, then one of 0 too many.
        let fill = |n: usize| vec![setting("", &"v".repeat(246)); n];
        assert_eq!(encode_mesh_settings(&fill(8)).unwrap().len(), 1984);
        let mut over = fill(8);
        over.push(setting("", ""));
        assert!(encode_mesh_settings(&over).is_none());
    }

    #[test]
    fn a_dump_goes_in_frames_of_1024_bytes() {
        let encoded: Vec<u8> = (0..1500u32).map(|i| i as u8).collect();
        let subject = DeviceUid::from_array([0x5A; 16]);
        let pages =
            build_mesh_config_pages(config_kind::DUMP, MESH_CONFIG_FLAG_COPY, subject, &encoded);
        assert_eq!(pages.len(), 2);
        let crc = super::super::osd::crc32(&encoded).to_le_bytes();
        for (page, (offset, len)) in pages.iter().zip([(0u16, 1024), (1024, 476)]) {
            assert_eq!(page.len(), MESH_CONFIG_HEADER_SIZE + len);
            assert_eq!(
                &page[..4],
                &[1, config_kind::DUMP, MESH_CONFIG_FLAG_COPY, 0]
            );
            assert_eq!(&page[4..6], &offset.to_le_bytes());
            assert_eq!(&page[6..8], &1500u16.to_le_bytes());
            assert_eq!(&page[8..12], &crc);
            assert_eq!(&page[12..16], &[0; 4]);
            assert_eq!(&page[16..32], subject.as_bytes());
            assert_eq!(&page[32..], &encoded[usize::from(offset)..][..len]);
        }
        let empty = build_mesh_config_pages(config_kind::SET, 0, subject, &[]);
        assert_eq!(empty.len(), 1);
        assert_eq!(empty[0].len(), MESH_CONFIG_HEADER_SIZE);
    }

    #[test]
    fn a_list_goes_in_frames_of_24_units() {
        let unit = |i: u8| MeshReplaceUnit {
            uid: DeviceUid::from_array([i; 16]),
            features: crate::wire::DeviceFeature::from_bits(0x0102_0304 + u32::from(i)),
            input: 0x1100 + u16::from(i),
            output: 0x2200 + u16::from(i),
            input_name: format!("in{i}"),
            output_name: "o".repeat(16),
        };
        let units: Vec<_> = (0..30).map(unit).collect();
        let frames = build_mesh_replace_units(&units, true);
        assert_eq!(frames.len(), 2);
        assert_eq!(frames[0].len(), 8 + 24 * 56);
        assert_eq!(frames[1].len(), 8 + 6 * 56);
        assert_eq!(&frames[1][..8], &[1, 1, 24, 0, 30, 0, 1, 0]);
        let r = &frames[1][8..64];
        assert_eq!(&r[..16], &[24; 16]);
        assert_eq!(&r[16..20], &(0x0102_0304u32 + 24).to_le_bytes());
        assert_eq!(&r[20..22], &0x1118u16.to_le_bytes());
        assert_eq!(&r[22..24], &0x2218u16.to_le_bytes());
        assert_eq!(&r[24..40], b"in24\0\0\0\0\0\0\0\0\0\0\0\0");
        assert_eq!(&r[40..56], "o".repeat(16).as_bytes());
        let none = build_mesh_replace_units(&[], false);
        assert_eq!(none, [vec![1, 1, 0, 0, 0, 0, 0, 0]]);
    }

    #[test]
    fn a_request_is_32_bytes() {
        let unit = DeviceUid::from_array([0x3C; 16]);
        let p = build_mesh_replace_target(replace_kind::RESULT, unit, MeshReplaceResult::BUSY);
        assert_eq!(p.len(), MESH_REPLACE_TARGET_SIZE);
        assert_eq!(&p[..8], &[1, 3, 0, 0, 0, 0, 0, 0]);
        assert_eq!(&p[8..24], unit.as_bytes());
        assert_eq!(p[24], 5);
        assert_eq!(&p[25..], &[0; 7]);
        assert_eq!(build_mesh_replace_query(), [1, 0, 0, 0, 0, 0, 0, 0]);
        let r = build_mesh_config_request(unit);
        assert_eq!(&r[..16], &[1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        assert_eq!(&r[16..], unit.as_bytes());
    }

    #[test]
    fn metadata_is_key_nul_value_nul_in_order() {
        let fields = [
            PortMetadataField::new("logo", "http://x"),
            PortMetadataField::new("device_type", "PS4"),
        ];
        let encoded = encode_port_metadata(&fields).unwrap();
        assert_eq!(encoded, b"logo\0http://x\0device_type\0PS4\0");
        assert_eq!(decode_port_metadata(&encoded).unwrap(), fields);
        assert_eq!(encode_port_metadata(&[]).unwrap(), b"");
        let port = MeshPort::input(DeviceUid::from_array([0x5A; 16]));
        let p = build_port_metadata(port, &encoded);
        assert_eq!(
            &p[..4],
            &[1, config_kind::METADATA, MESH_CONFIG_FLAG_INPUT, 0]
        );
        assert_eq!(&p[4..6], &[0, 0]);
        assert_eq!(&p[6..8], &(encoded.len() as u16).to_le_bytes());
        assert_eq!(&p[8..12], &super::super::osd::crc32(&encoded).to_le_bytes());
        assert_eq!(&p[32..], &encoded[..]);
        let none = build_port_metadata(MeshPort::output(port.unit), &[]);
        assert_eq!(&none[..12], &[1, 6, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    }

    #[test]
    fn metadata_a_unit_refuses_is_refused() {
        let f = |k: &str, v: &str| vec![PortMetadataField::new(k, v)];
        assert!(encode_port_metadata(&f(&"k".repeat(31), &"v".repeat(255))).is_some());
        for (k, v) in [
            ("", "v"),
            (&"k".repeat(32)[..], "v"),
            ("a.b", "v"),
            ("k", ""),
            ("k", &"v".repeat(256)[..]),
            ("k", "a\"b"),
            ("k", r"a\b"),
            ("k", "a\nb"),
            ("k", "a\x7Fb"),
        ] {
            assert!(encode_port_metadata(&f(k, v)).is_none(), "{k:?}={v:?}");
        }
        let many = vec![PortMetadataField::new("k", "v"); 17];
        assert!(encode_port_metadata(&many).is_none());
        assert!(encode_port_metadata(&many[..16]).is_some());
        // 4 fields of 3 + 253 bytes are 1024, and one byte more is too many.
        let wide = |n: usize| PortMetadataField::new("k", "v".repeat(n));
        assert_eq!(
            encode_port_metadata(&vec![wide(253); 4]).unwrap().len(),
            1024
        );
        let mut over = vec![wide(253); 3];
        over.push(wide(254));
        assert!(encode_port_metadata(&over).is_none());
        assert_eq!(decode_port_metadata(b"k\0v"), None);
        assert_eq!(decode_port_metadata(b"k\0"), None);
        assert_eq!(decode_port_metadata(b"k\0\xFF\0"), None, "not UTF-8");
    }

    #[test]
    fn an_index_goes_in_frames_of_48_entries() {
        let sender = DeviceUid::from_array([0x3C; 16]);
        let entries: Vec<_> = (0..50u8)
            .map(|i| MetadataIndexEntry {
                port: MeshPort {
                    unit: DeviceUid::from_array([i; 16]),
                    input: i % 2 == 1,
                },
                crc: 0x0102_0304 * u32::from(i),
            })
            .collect();
        let pages = build_metadata_index_pages(sender, 0xA1B2_C3D4, &entries);
        assert_eq!(pages.len(), 2);
        assert_eq!(pages[0].len(), 32 + 4 + 48 * 24);
        assert_eq!(pages[1].len(), 32 + 4 + 2 * 24);
        let mut all = Vec::new();
        for page in &pages {
            all.extend_from_slice(&page[36..]);
        }
        let crc = super::super::osd::crc32(&all).to_le_bytes();
        for (page, first) in pages.iter().zip([0u16, 48]) {
            assert_eq!(&page[..4], &[1, config_kind::METADATA_INDEX, 0, 0]);
            assert_eq!(&page[4..6], &first.to_le_bytes());
            assert_eq!(&page[6..8], &50u16.to_le_bytes());
            assert_eq!(&page[8..12], &crc);
            assert_eq!(&page[16..32], sender.as_bytes());
            assert_eq!(&page[32..36], &0xA1B2_C3D4u32.to_le_bytes());
        }
        let e = &pages[1][36..60];
        assert_eq!(&e[..16], &[48; 16]);
        assert_eq!(&e[16..20], &[0, 0, 0, 0]);
        assert_eq!(&pages[1][60 + 16..60 + 20], &[1, 0, 0, 0]);
        assert_eq!(&e[20..], &(0x0102_0304u32 * 48).to_le_bytes());
        let (generation, back) = decode_metadata_index(&pages[1][32..]).unwrap();
        assert_eq!(generation, 0xA1B2_C3D4);
        assert_eq!(back, entries[48..]);
        let none = build_metadata_index_pages(sender, 7, &[]);
        assert_eq!(none.len(), 1);
        assert_eq!(none[0].len(), 36);
        assert_eq!(&none[0][4..12], &[0; 8], "no entries and crc 0");
    }
}
