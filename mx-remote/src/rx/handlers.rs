// Author: Lars Op den Kamp (lars@opdenkamp-it.nl)
// Copyright (c) 2026 Op den Kamp IT Solutions

//! Handlers for the frames that report device and bay state.

use std::net::Ipv4Addr;

use crate::event::Event;
use crate::state::{HelloInfo, State};
use crate::types::{
    BayMirrorStatus, ConnectStatus, DeviceV2ipDetails, DeviceV2ipSink, FirmwareVersion,
    HiddenStatus, MuteStatus, PowerStatus, StreamKind, TopologyEntry, V2ipDscpConfig,
    V2ipScalingSettings, V2ipStreamSource, V2ipStreamSources, V2ipTilingConfig, VolumeMuteStatus,
    SCALING_FLAGS_DEFINED, VOLUME_UNCHANGED,
};
use crate::wire::{
    parse_bay_config, BayStatus, BayUid, DeviceFeature, DeviceUid, FirmwareType, Frame,
    MxrSignalType, RcAction, RcKey, BAY_CONFIG_SIZE, FW_VERSION_LEN,
};

use super::Rx;

/// Payload length of a remote-control key or action frame with a one-byte bay
/// id, and of the one from protocol 6 that widened it to two.
///
/// The length is what tells the two apart, not the frame's stamp. These
/// opcodes' table entries were seeded after the widening had already happened
/// and were never raised for it, so both forms go out stamped 0x01 and the
/// stamp says nothing about which one arrived. A table entry is what a frame
/// is stamped with; it is not a record of when a layout changed.
const RC_NARROW_SIZE: usize = 3;
const RC_WIDE_SIZE: usize = 4;

/// Payload length of the superseded `AUDIO_SET_VOLUME` layout, which addresses
/// its target by serial and carries a one-byte bay, and of the current one,
/// which addresses it by uid and carries two.
const SET_VOLUME_LEGACY_SIZE: usize = 20;
const SET_VOLUME_SIZE: usize = 24;

/// The bay and the value a remote-control key or action frame carries.
fn rc_bay_and_value(f: &Frame) -> Option<(u16, u16)> {
    match f.payload().len() {
        RC_NARROW_SIZE => Some((u16::from(f.u8(0)?), f.u16(1)?)),
        n if n >= RC_WIDE_SIZE => Some((f.u16(0)?, f.u16(2)?)),
        _ => None,
    }
}

pub(super) fn hello(state: &mut State, rx: &Rx<'_>, ev: &mut Vec<Event>) {
    let f = &rx.frame;
    let hello = HelloInfo {
        supported_protocol: f.u16(0).unwrap_or(0),
        name: f.str(2, 16).unwrap_or_default(),
        serial: f.str(18, 16).unwrap_or_default(),
        version: f.str(34, 16).unwrap_or_default(),
        features: DeviceFeature::from_bits(f.u32(50).unwrap_or(0)),
        address: rx.address,
    };
    state.apply_hello(rx.sender(), hello, rx.timestamp, ev);
}

/// Merges one page of bay descriptors into the device's bay list.
///
/// A device pages its bays across several frames, sizing each page against the
/// payload it can send and shrinking it further under memory pressure, so the
/// record count varies from frame to frame and no single frame holds the whole
/// list. Merge records rather than replacing the list, and never read the
/// record count as the device's bay count.
pub(super) fn bay_config(state: &mut State, rx: &Rx<'_>, ev: &mut Vec<Event>) {
    let Some(device) = state.device_mut(rx.sender()) else {
        return;
    };
    let payload = rx.frame.payload();
    for record in payload.chunks_exact(BAY_CONFIG_SIZE) {
        if let Some(cfg) = parse_bay_config(record) {
            device.apply_bay_config(&cfg, rx.timestamp, ev);
        }
    }
}

pub(super) fn connect_status(state: &mut State, rx: &Rx<'_>, ev: &mut Vec<Event>) {
    let Some(port) = rx.frame.u8(0) else {
        return;
    };
    let status = if rx.frame.boolean(1) {
        ConnectStatus::Connected
    } else {
        ConnectStatus::Disconnected
    };
    if let Some(bay) = state.bay_mut(BayUid::new(rx.sender(), u16::from(port))) {
        bay.apply_connect_status(status, ev);
    }
}

pub(super) fn power_change(state: &mut State, rx: &Rx<'_>, ev: &mut Vec<Event>) {
    let Some(port) = rx.frame.u8(0) else {
        return;
    };
    let power = if rx.frame.boolean(1) {
        PowerStatus::On
    } else {
        PowerStatus::Off
    };
    if let Some(bay) = state.bay_mut(BayUid::new(rx.sender(), u16::from(port))) {
        bay.set_power_status(power, ev);
    }
}

/// Applies a routing change: which source a sink is now showing and hearing.
///
/// `mxr_routing_change` is packed and every bay in it is an `mbay_port_id`, so
/// each is two bytes: sink at 0, selected at 2, video at 4, scrambled at 6 and
/// audio at 7. Reading them a byte wide puts "selected" in video and "video" in
/// audio, which agrees with the wire only while the selected input is the one
/// actually being shown.
pub(super) fn routing_change(state: &mut State, rx: &Rx<'_>, ev: &mut Vec<Event>) {
    let f = &rx.frame;
    let Some(sink_port) = f.u16(0) else {
        return;
    };
    let sender = rx.sender();
    let Some(device) = state.device(sender) else {
        return;
    };
    if device.bay(sink_port).is_none() {
        return;
    }
    let resolve = |port: Option<u16>| {
        port.filter(|p| device.bay(*p).is_some())
            .map(|p| BayUid::new(sender, p))
    };
    let video = resolve(f.u16(4));
    let audio = resolve(f.u16(7));

    if let Some(bay) = state.bay_mut(BayUid::new(sender, sink_port)) {
        if let Some(video) = video {
            bay.set_video_source(Some(video), ev);
        }
        if let Some(audio) = audio {
            bay.set_audio_source(Some(audio), ev);
        }
    }
}

pub(super) fn rc_action(state: &mut State, rx: &Rx<'_>, ev: &mut Vec<Event>) {
    let Some(device) = state.device(rx.sender()) else {
        return;
    };
    let Some((port, action)) = rc_bay_and_value(&rx.frame) else {
        return;
    };
    if device.bay(port).is_some() {
        ev.push(Event::ActionReceived {
            bay: BayUid::new(rx.sender(), port),
            action: RcAction::from_wire(action),
        });
    }
}

/// Reports a remote-control key press.
pub(super) fn rc_key(state: &mut State, rx: &Rx<'_>, ev: &mut Vec<Event>) {
    let Some(device) = state.device(rx.sender()) else {
        return;
    };
    let Some((port, key)) = rc_bay_and_value(&rx.frame) else {
        return;
    };
    if device.bay(port).is_some() {
        ev.push(Event::KeyPressed {
            bay: BayUid::new(rx.sender(), port),
            key: RcKey::from_wire(key),
        });
    }
}

/// Applies an `AUDIO_SET_VOLUME` request: a volume per channel and a mute
/// bitmask, for one bay of the device that sent it.
///
/// Two layouts, told apart by length. The superseded form names its target by
/// serial and carries a one-byte bay; the current one names it by uid and
/// carries two. The stamp does separate them here - it was raised in the same
/// change, making this the one opcode whose stamp selects a layout - but the
/// length says the same thing without trusting the sender to stamp correctly,
/// and the two cannot be confused for each other: the first sixteen bytes are
/// a printable serial in one and a binary identifier in the other.
///
/// Either way the volume is filed under the sender rather than under the
/// target the payload names.
pub(super) fn volume_set(state: &mut State, rx: &Rx<'_>, ev: &mut Vec<Event>) {
    let f = &rx.frame;
    let (port, at) = match f.payload().len() {
        SET_VOLUME_LEGACY_SIZE => (f.u8(16).map(u16::from), 17),
        n if n >= SET_VOLUME_SIZE => (f.u16(16), 18),
        _ => return,
    };
    let Some(port) = port else {
        return;
    };
    // A volume above 100 is not a percentage, which drops the "leave this
    // alone" value along with anything else out of range. The mute byte has
    // no such range to fall outside of, so it is checked for that value
    // directly: read as a bitmask it would say both channels are muted, which
    // is the opposite of the "do not change" it means.
    let muted = f.u8(at + 2).filter(|m| *m != VOLUME_UNCHANGED);
    let volume = VolumeMuteStatus {
        volume_left: f.u8(at).filter(|v| *v <= 100),
        volume_right: f.u8(at + 1).filter(|v| *v <= 100),
        muted_left: muted.map(|m| MuteStatus::from_wire(m).left()),
        muted_right: muted.map(|m| MuteStatus::from_wire(m).right()),
    };
    if let Some(device) = state.device_mut(rx.sender()) {
        device.apply_bay_volume(port, volume, ev);
    }
}

pub(super) fn temperature(state: &mut State, rx: &Rx<'_>, ev: &mut Vec<Event>) {
    let f = &rx.frame;
    let Some(count) = f.u8(0) else {
        return;
    };
    let temperatures = (1..=usize::from(count)).filter_map(|i| f.u8(i)).collect();
    if let Some(device) = state.device_mut(rx.sender()) {
        device.set_temperatures(temperatures, ev);
    }
}

/// Width of one record in the V2IP sources frame.
const V2IP_SOURCE_RECORD: usize = 40;

pub(super) fn v2ip_sources(state: &mut State, rx: &Rx<'_>, ev: &mut Vec<Event>) {
    let sources: Vec<V2ipStreamSources> = rx
        .frame
        .payload()
        .chunks_exact(V2IP_SOURCE_RECORD)
        .map(|record| V2ipStreamSources {
            uid: uid_at(record, 0),
            video: stream_source(StreamKind::Video, record, 16),
            audio: stream_source(StreamKind::Audio, record, 24),
            anc: stream_source(StreamKind::Anc, record, 32),
            arc: None,
        })
        .collect();
    if let Some(device) = state.device_mut(rx.sender()) {
        device.set_v2ip_sources(sources, ev);
    }
}

/// Applies the source switch a mesh master broadcasts, which names the sink by
/// uid and the streams it is now subscribed to by group address.
pub(super) fn v2ip_source_switch(state: &mut State, rx: &Rx<'_>, ev: &mut Vec<Event>) {
    let target = rx.uid_or_zero(0);
    let payload = rx.frame.payload();
    let Some(body) = payload.get(16..24) else {
        return;
    };
    let video = ipv4_at(body, 0);
    let audio = ipv4_at(body, 4);
    let Some(sink) = state
        .device(target)
        .and_then(|d| d.first_output_port())
        .map(|port| BayUid::new(target, port))
    else {
        return;
    };
    apply_stream_route(state, sink, video, audio, ev);
}

/// Applies a manual source switch, which carries the full stream triple and
/// optionally the audio format the sink should expect.
///
/// The frame is a route request addressed to the sink, and is applied to the
/// sink here rather than only announced as a request: every device on the mesh
/// writes an observed switch into its own record of the addressee, so leaving
/// the registry alone would make this client the only participant that does not
/// know where a sink was pointed. What the sink did about it is not
/// acknowledged - see `DeviceV2ipSink`.
pub(super) fn v2ip_manual_source_switch(state: &mut State, rx: &Rx<'_>, ev: &mut Vec<Event>) {
    let target = rx.uid_or_zero(0);
    let payload = rx.frame.payload();
    if payload.len() < 38 {
        return;
    }
    let video = stream_source(StreamKind::Video, payload, 16);
    let audio = stream_source(StreamKind::Audio, payload, 24);
    let anc = stream_source(StreamKind::Anc, payload, 32);

    if let Some(sink) = state
        .device(target)
        .and_then(|d| d.first_output_port())
        .map(|port| BayUid::new(target, port))
    {
        apply_stream_route(state, sink, video.ip, audio.ip, ev);
    }

    let sink = DeviceV2ipSink {
        addresses: V2ipStreamSources {
            uid: DeviceUid::ZERO,
            video,
            audio,
            anc,
            arc: None,
        },
        audio_fmt: payload.get(40..48).and_then(audio_format),
    };
    if let Some(device) = state.device_mut(target) {
        device.set_v2ip_sink(sink, ev);
    }
}

/// Points a sink at the source bays advertising the given groups, leaving a
/// stream that maps to no known bay alone.
fn apply_stream_route(
    state: &mut State,
    sink: BayUid,
    video_ip: Ipv4Addr,
    audio_ip: Ipv4Addr,
    ev: &mut Vec<Event>,
) {
    let video = state.bay_by_stream_ip(video_ip, false);
    let audio = state.bay_by_stream_ip(audio_ip, true);
    if let Some(bay) = state.bay_mut(sink) {
        if let Some(video) = video {
            bay.set_video_source(Some(video), ev);
        }
        if let Some(audio) = audio {
            bay.set_audio_source(Some(audio), ev);
        }
    }
}

/// Applies a device's V2IP encoder configuration, and the tiling and sink
/// blocks a longer frame appends to it.
pub(super) fn v2ip_device_configuration(state: &mut State, rx: &Rx<'_>, ev: &mut Vec<Event>) {
    let p = rx.frame.payload();
    if p.len() < 61 {
        return;
    }
    let tx_rate = p.get(40).copied().filter(|rate| {
        (crate::wire::V2IP_SOURCE_RATE_MIN..=crate::wire::V2IP_SOURCE_RATE_MAX).contains(rate)
    });
    let details = DeviceV2ipDetails {
        video: stream_source(StreamKind::Video, p, 16),
        audio: stream_source(StreamKind::Audio, p, 24),
        anc: stream_source(StreamKind::Anc, p, 32),
        arc: stream_source(StreamKind::Arc, p, 48),
        tx_rate,
        dscp: V2ipDscpConfig {
            video: crate::types::parse_dscp(byte(p, 41)),
            audio: crate::types::parse_dscp(byte(p, 42)),
            anc: crate::types::parse_dscp(byte(p, 43)),
        },
        scaling: V2ipScalingSettings {
            mode: MxrSignalType::from_wire(u16_at(p, 56)),
            refresh: u16_at(p, 58),
            // Only three bits are defined. Firmware that does not initialise
            // this frame builds it from an uninitialised stack local, so the
            // rest is noise and must not reach the cache even on a first frame.
            flags: byte(p, 60) & SCALING_FLAGS_DEFINED,
        },
    };
    if let Some(device) = state.device_mut(rx.sender()) {
        device.set_v2ip_details(details, ev);
    }

    // The tiling block carries no validity flag of its own; its uid is the
    // marker. Every path that produces a real window stamps it, and a
    // controller writing any other field sends the block zeroed - so a zero uid
    // means "not carried" while a set uid with zero geometry is a real clear.
    if p.len() >= 88 {
        let target = uid_at(p, 64);
        if !target.is_zero() {
            let tiling = V2ipTilingConfig {
                target,
                pos_x: u16_at(p, 80),
                pos_y: u16_at(p, 82),
                width: u16_at(p, 84),
                height: u16_at(p, 86),
            };
            if let Some(device) = state.device_mut(rx.sender()) {
                device.set_tiling(tiling, ev);
            }
        }
    }

    if p.len() >= 120 {
        let sink = DeviceV2ipSink {
            addresses: V2ipStreamSources {
                uid: DeviceUid::ZERO,
                video: stream_source(StreamKind::Video, p, 88),
                audio: stream_source(StreamKind::Audio, p, 96),
                anc: stream_source(StreamKind::Anc, p, 104),
                arc: None,
            },
            audio_fmt: p.get(112..120).and_then(audio_format),
        };
        if let Some(device) = state.device_mut(rx.sender()) {
            device.set_v2ip_sink(sink, ev);
        }
    }
}

pub(super) fn bay_hide(state: &mut State, rx: &Rx<'_>, ev: &mut Vec<Event>) {
    let target = rx.uid_or_zero(0);
    let Some(port) = rx.frame.u16(16) else {
        return;
    };
    let hidden = if rx.frame.boolean(18) {
        HiddenStatus::Hidden
    } else {
        HiddenStatus::Visible
    };
    if let Some(bay) = state.bay_mut(BayUid::new(target, port)) {
        bay.apply_hidden(hidden, ev);
    }
}

/// Applies a bay status report.
///
/// `mxr_bay_status.local_bay` is an `mbay_port_id`, so it is a `u16` at 0.
/// Reading its high byte at 1 yields zero for every real port and finds no bay
/// at all.
pub(super) fn bay_status(state: &mut State, rx: &Rx<'_>, ev: &mut Vec<Event>) {
    let f = &rx.frame;
    let Some(port) = f.u16(0) else {
        return;
    };
    let sender = rx.sender();
    let Some(device) = state.device(sender) else {
        return;
    };
    if device.bay(port).is_none() {
        return;
    }
    let is_v2ip = device.is_v2ip();
    let features = f.u32(24);
    let Some(status) = f.u32(20).map(BayStatus::from_bits) else {
        return;
    };
    // `mxr_cfg_signal` is a 14-byte description followed by a 2-byte signal
    // type, so the description stops at 14 rather than running into the type.
    let description = f.str(2, 14);

    if let Some(bay) = state.bay_mut(BayUid::new(sender, port)) {
        if let Some(features) = features {
            bay.features = crate::wire::BayFeatures::from_bits(features);
        }
        bay.apply_bay_status(status, ev);
        // A V2IP source reporting a signal describes it in its own detailed
        // report, which carries the frame rate this field has no room for.
        if !status.has(BayStatus::SIGNAL_DETECTED) || !is_v2ip {
            bay.apply_signal_status(status.has(BayStatus::SIGNAL_DETECTED), description, ev);
        }
    }
}

/// Width of one record in the link configuration frame.
const LINK_RECORD: usize = 38;

/// Merges one page of link descriptors into the link registry, paged the same
/// way as the bay configuration.
pub(super) fn links(state: &mut State, rx: &Rx<'_>, ev: &mut Vec<Event>) {
    let sender = rx.sender();
    let records: Vec<(u16, String, String, u32)> = rx
        .frame
        .payload()
        .chunks_exact(LINK_RECORD)
        .map(|record| {
            (
                u16::from(byte(record, 0)),
                crate::wire::cstr(&record[2..18]),
                crate::wire::cstr(&record[18..34]),
                u32_at(record, 34),
            )
        })
        .collect();
    for (port, linked_serial, linked_bay, features) in records {
        let origin = BayUid::new(sender, port);
        if state.bay(origin).is_some() {
            state.update_link(origin, linked_serial, linked_bay, features, ev);
        }
    }
    if let Some(device) = state.device_mut(sender) {
        device.on_link_config_received(ev);
    }
}

/// Applies a mirror report: which device's output this one is repeating.
pub(super) fn mirror_status(state: &mut State, rx: &Rx<'_>, ev: &mut Vec<Event>) {
    let sender = rx.sender();
    let target = rx.uid_or_zero(0);
    if sender != target {
        return;
    }
    let Some(port) = state.device(sender).and_then(|d| d.first_output_port()) else {
        return;
    };
    let master = rx.uid_or_zero(16);
    let mirror = BayMirrorStatus {
        target: (!master.is_zero() && master != target).then(|| BayUid::new(master, 0)),
    };
    if let Some(bay) = state.bay_mut(BayUid::new(sender, port)) {
        bay.set_mirroring(mirror, ev);
    }
}

/// The mesh sub-opcode that reports which device is the mesh master.
const MESH_REPORT_MEMBERSHIP: u8 = 0xFF;

pub(super) fn mesh_operation(state: &mut State, rx: &Rx<'_>, ev: &mut Vec<Event>) {
    if rx.frame.u8(0) != Some(MESH_REPORT_MEMBERSHIP) {
        return;
    }
    let master = rx.uid_or_zero(4);
    if let Some(device) = state.device_mut(rx.sender()) {
        device.set_mesh_master(master, ev);
    }
}

/// Records which source device each V2IP bay stands in for.
///
/// The first `u16` packs the record count above a direction bit, and the
/// second is the bay number the run starts at.
pub(super) fn v2ip_bay_mapping(state: &mut State, rx: &Rx<'_>, _ev: &mut [Event]) {
    let f = &rx.frame;
    let (Some(header), Some(first)) = (f.u16(0), f.u16(2)) else {
        return;
    };
    let count = header >> 1;
    let mode = if header & 1 == 1 { "Input" } else { "Output" };
    let sender = rx.sender();

    let mut mappings: Vec<(u16, DeviceUid)> = Vec::new();
    for i in 0..count {
        let Some(uid) = f.uid(8 + 16 * usize::from(i)) else {
            break;
        };
        let Some(number) = first.checked_add(i).and_then(|n| u8::try_from(n).ok()) else {
            break;
        };
        let Some(device) = state.device(sender) else {
            return;
        };
        if let Some(bay) = device.bay_by_mode_num(mode, number) {
            mappings.push((bay.port, uid));
        }
    }
    for (port, uid) in mappings {
        if let Some(bay) = state.bay_mut(BayUid::new(sender, port)) {
            bay.v2ip_uid = uid;
        }
    }
}

/// Width of one entry in a topology report.
const TOPOLOGY_ENTRY: usize = 20;

pub(super) fn topology(state: &mut State, rx: &Rx<'_>, ev: &mut Vec<Event>) {
    let topology: Vec<TopologyEntry> = rx
        .frame
        .payload()
        .chunks_exact(TOPOLOGY_ENTRY)
        .map(|entry| TopologyEntry {
            uid: uid_at(entry, 0),
            mask: u32_at(entry, 16),
        })
        .collect();
    if let Some(device) = state.device_mut(rx.sender()) {
        device.set_topology(topology, ev);
    }
}

/// Applies a firmware version report for one component.
pub(super) fn firmware_version(state: &mut State, rx: &Rx<'_>, ev: &mut Vec<Event>) {
    let f = &rx.frame;
    let Some(firmware_type) = f.u8(0) else {
        return;
    };
    // Read at most the field's own width, but settle for what a peer sending a
    // short frame did give us rather than losing the whole report over a name.
    let available = f.payload().len().saturating_sub(12);
    let name_len = available.min(FW_VERSION_LEN);
    if name_len == 0 {
        return;
    }
    let (Some(version), Some(timestamp)) = (f.str(12, name_len), f.u32(8)) else {
        return;
    };
    let version = FirmwareVersion {
        firmware_type: FirmwareType::from_wire(firmware_type),
        timestamp,
        version,
        hash: f.u32(4).unwrap_or(0),
    };
    if let Some(device) = state.device_mut(rx.sender()) {
        device.set_firmware_version(version, ev);
    }
}

pub(super) fn system_status(state: &mut State, rx: &Rx<'_>, ev: &mut Vec<Event>) {
    let Some(status) = rx.frame.u16(16) else {
        return;
    };
    let message = rx.frame.str_to_end(18).unwrap_or_default();
    if let Some(device) = state.device_mut(rx.sender()) {
        device.set_system_status(status, message, ev);
    }
}

// ---- shared readers ----

pub(super) fn byte(p: &[u8], idx: usize) -> u8 {
    p.get(idx).copied().unwrap_or(0)
}

pub(super) fn u16_at(p: &[u8], idx: usize) -> u16 {
    p.get(idx..idx + 2)
        .and_then(|b| <[u8; 2]>::try_from(b).ok())
        .map_or(0, u16::from_le_bytes)
}

pub(super) fn u32_at(p: &[u8], idx: usize) -> u32 {
    p.get(idx..idx + 4)
        .and_then(|b| <[u8; 4]>::try_from(b).ok())
        .map_or(0, u32::from_le_bytes)
}

pub(super) fn uid_at(p: &[u8], idx: usize) -> DeviceUid {
    p.get(idx..idx + 16)
        .and_then(|b| <[u8; 16]>::try_from(b).ok())
        .map_or(DeviceUid::ZERO, DeviceUid::from_array)
}

/// Reads a network-order IPv4 address.
pub(super) fn ipv4_at(p: &[u8], idx: usize) -> Ipv4Addr {
    Ipv4Addr::new(
        byte(p, idx),
        byte(p, idx + 1),
        byte(p, idx + 2),
        byte(p, idx + 3),
    )
}

/// Reads a `v2ip_stream_addr`: an address and a port.
pub(super) fn stream_source(kind: StreamKind, p: &[u8], idx: usize) -> V2ipStreamSource {
    V2ipStreamSource {
        kind,
        ip: ipv4_at(p, idx),
        port: u16_at(p, idx + 4),
    }
}

/// Reads an audio format block, which is absent when it reports no channels.
fn audio_format(p: &[u8]) -> Option<crate::types::V2ipAudioFormat> {
    let channels = byte(p, 4);
    (channels != 0).then(|| crate::types::V2ipAudioFormat {
        sample_rate: u32_at(p, 0),
        channels,
    })
}
