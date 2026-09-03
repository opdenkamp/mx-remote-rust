// Author: Lars Op den Kamp (lars@opdenkamp-it.nl)
// Copyright (c) 2026 Op den Kamp IT Solutions

//! Coverage over the opcode handlers.
//!
//! A handler nothing exercises has every field unpinned by construction, which
//! neither an offset sweep nor a poisoned fixture reports: both measure tests
//! that run, and an unexercised handler does not.

use crate::event::Event;
use crate::types::{
    ArcStatus, PowerStatus, AMP_TONE_HTTP_MAX, AMP_TONE_HTTP_MIN, VOLUME_UNCHANGED,
};
use crate::wire::{
    op, parse_bay_config, BayFeatures, BayStatus, DeviceFeature, EdidProfile, MxrSignalType,
    Opcode, RcAction, RcKey, RcType, PROTOCOL_VERSION, V2IP_PORT_VIDEO,
};

use crate::testing::{bay_config_rec, field, hello_payload, poisoned, stream_rec, uid_n};

use super::Harness;

/// A matrix with two inputs and one amplified output.
fn bay_state(n: u8) -> Harness {
    let mut h = Harness::new(n);
    h.hello(0x28, "FF88", "HD0001", DeviceFeature::VIDEO_ROUTING);
    let mut cfg = bay_config_rec(
        1,
        0,
        0,
        "Input 1",
        "Apple TV",
        BayStatus::NONE,
        BayFeatures::HDMI_IN,
    );
    cfg.extend(bay_config_rec(
        2,
        1,
        0,
        "Output 1",
        "TV",
        BayStatus::NONE,
        BayFeatures::HDMI_OUT | BayFeatures::AUDIO_AMP_OUT,
    ));
    cfg.extend(bay_config_rec(
        3,
        0,
        1,
        "Input 2",
        "Blu-ray",
        BayStatus::NONE,
        BayFeatures::HDMI_IN,
    ));
    h.feed(op::SYS_BAY_CONFIG, &cfg);
    h
}

/// A remote-control key or action frame is read at the width its length says.
///
/// The bay id widened from one byte to two at protocol 6, but these opcodes'
/// table entries predate that and were never raised, so both forms go out
/// stamped 0x01 and the stamp cannot tell them apart. Every case below is fed
/// at that stamp, which is what a real device sends: a decoder selecting on
/// the stamp reads every one of them as the narrow form.
#[test]
fn a_key_or_action_frame_is_read_at_the_width_its_length_says() {
    let mut h = bay_state(126);

    // Four bytes: a u16 bay and a u16 value. The value is above 255 so that a
    // one-byte read of it cannot pass, and the bay is 1 so that the narrow
    // and wide reads of the bay agree - this case is about the value.
    h.feed_proto(op::RC_KEY, 0x01, &[1, 0, 0x00, 0x05]);
    assert!(
        h.saw(|e| matches!(e, Event::KeyPressed { key, .. } if *key == RcKey::from_wire(0x0500))),
        "a wide key was truncated to its low byte"
    );
    h.feed_proto(op::RC_ACTION, 0x01, &[1, 0, 0x00, 0x05]);
    assert!(
        h.saw(
            |e| matches!(e, Event::ActionReceived { action, .. } if *action
                == RcAction::from_wire(0x0500))
        ),
        "a wide action was truncated to its low byte"
    );

    // Three bytes: a u8 bay, and the value one byte earlier.
    h.feed_proto(op::RC_KEY, 0x01, &[3, 0x42, 0x00]);
    assert!(
        h.saw(|e| matches!(e, Event::KeyPressed { bay, key }
            if bay.port == 3 && *key == RcKey::from_wire(0x42))),
        "the superseded three-byte form did not decode"
    );
    h.feed_proto(op::RC_ACTION, 0x01, &[3, 0x42, 0x00]);
    assert!(
        h.saw(|e| matches!(e, Event::ActionReceived { bay, action }
            if bay.port == 3 && *action == RcAction::from_wire(0x42))),
        "the superseded three-byte form did not decode"
    );

    // The bay field carries sentinels above 255. Read one byte wide they
    // become 0xFF and 0xFE, which land inside the range of real output ports
    // and name a bay that exists - worse than truncating, because nothing
    // downstream can tell it was invented.
    for sentinel in [0xFFFFu16, 0xFFFE] {
        let mut p = sentinel.to_le_bytes().to_vec();
        p.extend_from_slice(&[0x41, 0x00]);
        h.feed_proto(op::RC_KEY, 0x01, &p);
    }
    assert!(
        !h.saw(|e| matches!(e, Event::KeyPressed { .. })
            && !matches!(e, Event::KeyPressed { bay, .. } if bay.port == 1 || bay.port == 3)),
        "a sentinel bay id decoded as a real port"
    );
}

/// No payload, at any length or stamp, takes a handler out of bounds.
///
/// A frame that does not parse is dropped, which means dropped rather than
/// panicked on: this runs in the receive thread, and a panic there takes the
/// client down over a datagram anybody on the segment can send. Every handler
/// bounds its own reads, but a length gate is easy to write in front of the
/// wrong thing, and the one on `V2IP_STATS` turned out to bound the slicing of
/// the counter blocks as well as the choice of layout - a fact no test saw
/// until it was looked for. This looks for all of them at once.
///
/// The sweep covers every opcode value rather than the declared ones, so an
/// opcode added without a handler is covered on the day it is declared. Three
/// byte patterns, because a poisoned payload and a zeroed one drive a
/// count-carrying field to different places, and an all-ones one drives it as
/// far as it goes.
#[test]
fn no_payload_length_takes_a_handler_out_of_bounds() {
    for opcode in 0u16..=0x60 {
        for stamp in [0x00, 0x11, PROTOCOL_VERSION] {
            // Rebuilt per stamp so a payload that registers something does not
            // change what a later length is read against.
            let mut h = bay_state(131);
            for len in 0..=200usize {
                for fill in [poisoned(len), vec![0u8; len], vec![0xFFu8; len]] {
                    h.events.clear();
                    h.feed_proto(Opcode(opcode), stamp, &fill);
                }
            }
        }
    }
}

/// The volume request has two layouts, and the length is what separates them.
#[test]
fn a_volume_request_is_read_at_the_layout_its_length_says() {
    let mut h = bay_state(127);
    let sender = h.sender;

    // The superseded 20-byte form: a serial rather than a uid, and a one-byte
    // bay. Nothing but the length says so - it is fed at the stamp the
    // current form uses, so a decoder selecting on the stamp reads it as the
    // current layout and takes the volume from the wrong offsets entirely.
    let mut legacy = vec![0u8; 20];
    field(&mut legacy, 0, 16, "HD0001");
    legacy[16..].copy_from_slice(&[2, 33, 44, 1]);
    h.feed_proto(op::AUDIO_SET_VOLUME, 0x11, &legacy);
    let v = h.bay(2).audio_volume.expect("the old form did not decode");
    assert_eq!((v.volume_left, v.volume_right), (Some(33), Some(44)));
    assert_eq!((v.muted_left, v.muted_right), (Some(true), Some(false)));

    // The current 24-byte form, for the paired direction: the two layouts put
    // the volume at different offsets, so a test on one alone would pass for a
    // decoder that had them the wrong way round.
    let mut current = sender.as_bytes().to_vec();
    current.extend_from_slice(&[2, 0, 55, 66, 2, 0, 0, 0]);
    assert_eq!(current.len(), 24);
    h.feed(op::AUDIO_SET_VOLUME, &current);
    let v = h
        .bay(2)
        .audio_volume
        .expect("the current form did not decode");
    assert_eq!((v.volume_left, v.volume_right), (Some(55), Some(66)));
    assert_eq!((v.muted_left, v.muted_right), (Some(false), Some(true)));

    // The current form stops at the mute byte, without the three bytes of tail
    // padding that round its struct to 24. Gating on the padded size rather
    // than on the last field would drop this, and the superseded layout is what
    // decides the boundary: anything past its length is the current one.
    let mut unpadded = sender.as_bytes().to_vec();
    unpadded.extend_from_slice(&[2, 0, 77, 88, 1]);
    assert_eq!(unpadded.len(), 21);
    h.feed(op::AUDIO_SET_VOLUME, &unpadded);
    let v = h
        .bay(2)
        .audio_volume
        .expect("an unpadded request was dropped");
    assert_eq!((v.volume_left, v.volume_right), (Some(77), Some(88)));
    assert_eq!((v.muted_left, v.muted_right), (Some(true), Some(false)));
}

/// A volume request names the bay it is for, and a controller is not it.
#[test]
fn a_volume_request_lands_on_the_bay_it_names() {
    let mut h = bay_state(129);
    let target = h.sender;
    let controller = uid_n(200);
    h.feed_as(
        controller,
        op::SYS_HELLO,
        &hello_payload(0x28, "Ctrl", "CTRL0001", "4.8.0", DeviceFeature::MANAGER),
    );

    // The frame every fixture here had the sender address itself, which is the
    // one shape that cannot tell the two uids apart. A controller owns no bay
    // the request could be about, so filing under the sender loses the setting
    // entirely rather than putting it somewhere visible.
    let mut current = target.as_bytes().to_vec();
    current.extend_from_slice(&[2, 0, 55, 66, 2, 0, 0, 0]);
    h.feed_as(controller, op::AUDIO_SET_VOLUME, &current);

    let v = h
        .bay(2)
        .audio_volume
        .expect("the addressed bay never saw the request");
    assert_eq!((v.volume_left, v.volume_right), (Some(55), Some(66)));
    assert_eq!((v.muted_left, v.muted_right), (Some(false), Some(true)));

    // The superseded form names its target by serial rather than by uid, and
    // resolves to the same bay.
    let mut legacy = vec![0u8; 20];
    field(&mut legacy, 0, 16, "HD0001");
    legacy[16..].copy_from_slice(&[2, 33, 44, 1]);
    h.feed_as(controller, op::AUDIO_SET_VOLUME, &legacy);
    let v = h.bay(2).audio_volume.expect("no volume");
    assert_eq!((v.volume_left, v.volume_right), (Some(33), Some(44)));
}

/// The value that means "leave this alone" is not a volume and not a mute.
///
/// A sender changing only the volume writes it into the mute byte. Read as a
/// bitmask it sets both channel bits, which says the opposite of what was
/// sent: a bay reported fully muted by a request that was declining to touch
/// mute at all.
#[test]
fn the_unchanged_value_is_not_read_as_a_setting() {
    let mut h = bay_state(128);
    let sender = h.sender;
    let vol = |bytes: &[u8; 8]| {
        let mut p = sender.as_bytes().to_vec();
        p.extend_from_slice(bytes);
        p
    };

    // Leave the bay unmuted. Muting it first would not do: "both channels
    // muted" is exactly what the unchanged value decodes to when it is read as
    // a bitmask, so the assertion below would hold whether or not the value
    // was understood.
    h.feed(op::AUDIO_SET_VOLUME, &vol(&[2, 0, 20, 20, 0, 0, 0, 0]));
    assert_eq!(
        h.bay(2).audio_volume.expect("no volume").muted(),
        Some(false)
    );

    h.feed(
        op::AUDIO_SET_VOLUME,
        &vol(&[2, 0, 70, 70, VOLUME_UNCHANGED, 0, 0, 0]),
    );
    let v = h.bay(2).audio_volume.expect("no volume");
    // The volume it did carry is applied, so this is not a frame being dropped.
    assert_eq!((v.volume_left, v.volume_right), (Some(70), Some(70)));
    // And the mute it declined to name is the one that was already there.
    assert_eq!(
        v.muted(),
        Some(false),
        "the unchanged value was read as a mute state"
    );

    // The paired direction: a frame that does name a mute state moves it, so
    // the assertion above is not one that would hold for a decoder ignoring
    // the mute byte altogether.
    h.feed(op::AUDIO_SET_VOLUME, &vol(&[2, 0, 70, 70, 3, 0, 0, 0]));
    assert_eq!(
        h.bay(2).audio_volume.expect("no volume").muted(),
        Some(true)
    );

    // The same value in a volume field is outside the percentage range and
    // drops out with it, leaving the volume that was there rather than 255.
    h.feed(
        op::AUDIO_SET_VOLUME,
        &vol(&[2, 0, VOLUME_UNCHANGED, VOLUME_UNCHANGED, 0, 0, 0, 0]),
    );
    let v = h.bay(2).audio_volume.expect("no volume");
    assert_eq!((v.volume_left, v.volume_right), (Some(70), Some(70)));
    assert_eq!(v.muted(), Some(false), "this frame did name a mute state");
}

#[test]
fn bay_targeted_handlers_reach_the_bay_they_name() {
    let mut h = bay_state(110);
    let sender = h.sender;

    // 0x04 DEV_CONNECT: an input reports signal, an output reports hot-plug.
    // The port and the flag differ and the other bay is asserted untouched, so
    // reading the port one byte over lands on the wrong bay rather than on the
    // same value.
    h.feed(op::DEV_CONNECT, &[2, 1]);
    assert_eq!(
        h.bay(2).hpd_detected,
        Some(true),
        "connect status did not set hot-plug on an output"
    );
    assert_ne!(
        h.bay(1).signal_detected,
        Some(true),
        "connect status for port 2 reached port 1"
    );

    // 0x05 DEV_POWER_CHANGE
    h.feed(op::DEV_POWER_CHANGE, &[2, 1]);
    assert_eq!(h.bay(2).power_status, Some(PowerStatus::On));
    assert_ne!(
        h.bay(1).power_status,
        Some(PowerStatus::On),
        "power change for port 2 reached port 1"
    );

    // 0x27 BAY_HIDE, addressed by uid then a u16 port
    let mut hide = sender.as_bytes().to_vec();
    hide.extend_from_slice(&[2, 0, 1]);
    h.feed(op::BAY_HIDE, &hide);
    assert_eq!(h.bay(2).hidden, Some(true));

    // 0x14 AUDIO_SET_VOLUME: uid, u16 port, then volume and mute
    let mut vol = sender.as_bytes().to_vec();
    vol.extend_from_slice(&[2, 0, 40, 45, 0, 0, 0, 0]);
    h.feed(op::AUDIO_SET_VOLUME, &vol);
    let volume = h.bay(2).audio_volume.expect("volume not recorded");
    assert_eq!(
        (volume.volume_left, volume.volume_right),
        (Some(40), Some(45))
    );

    // 0x12 AUDIO_VOLUME_MUTE: the notification form, port as a single byte
    h.feed(op::AUDIO_VOLUME_MUTE, &[2, 55, 60, 0]);
    let volume = h.bay(2).audio_volume.expect("volume not recorded");
    assert_eq!(
        (volume.volume_left, volume.volume_right),
        (Some(55), Some(60))
    );

    // 0x0B RC_KEY and 0x0D RC_ACTION in their four-byte form
    h.feed(op::RC_KEY, &[1, 0, 0x41, 0x00]);
    assert!(h.saw(|e| matches!(e, Event::KeyPressed { key, .. } if *key == RcKey::from_wire(0x41))));
    h.feed(
        op::RC_ACTION,
        &[1, 0, RcAction::POWER_ON.to_wire() as u8, 0],
    );
    assert!(h.saw(
        |e| matches!(e, Event::ActionReceived { action, .. } if *action == RcAction::POWER_ON)
    ));

    // 0x11 AUDIO_CLIP
    h.feed(op::AUDIO_CLIP, &[2, 1]);
    assert!(h.saw(
        |e| matches!(e, Event::AudioClipped { clip, .. } if clip.port == 2 && clip.clip == 1)
    ));

    // 0x39 BAY_STATUS: a u16 port, then mxr_cfg_signal, which is a 14-byte
    // description followed by a 2-byte signal type. The type bytes are
    // non-zero here, so a description read past 14 picks them up.
    let mut st = poisoned(28);
    st[0..2].copy_from_slice(&1u16.to_le_bytes());
    st[2..16].copy_from_slice(b"1080p60 444 8b"); // exactly 14: no terminator
    st[16] = 0x13;
    st[17] = 0x20; // signal type, not part of the description
    st[20..24].copy_from_slice(&BayStatus::SIGNAL_DETECTED.bits().to_le_bytes());
    st[24..28].copy_from_slice(&BayFeatures::HDMI_IN.bits().to_le_bytes());
    h.feed(op::BAY_STATUS, &st);
    assert_eq!(h.bay(1).signal_detected, Some(true));
    assert_eq!(h.bay(1).features, BayFeatures::HDMI_IN);
    assert_eq!(h.bay(1).signal_type.as_deref(), Some("1080p60 444 8b"));

    // The port is two bytes wide, and no bay can sit above 255, so a status
    // frame naming port 257 belongs to no bay at all. Either half of that port
    // read on its own lands on bay 1 and applies this description to it.
    st[0] = 1;
    st[1] = 1;
    st[2..16].copy_from_slice(b"480i60 422 12 ");
    h.feed(op::BAY_STATUS, &st);
    assert_eq!(
        h.bay(1).signal_type.as_deref(),
        Some("1080p60 444 8b"),
        "a status frame for port 257 reached port 1"
    );
}

#[test]
fn routing_and_device_handlers() {
    let mut h = bay_state(111);
    let sender = h.sender;

    // 0x08 MX_ROUTE, packed with u16 ports: sink at 0, selected at 2, video at
    // 4, scrambled at 6, audio at 7. Selected is deliberately a different bay
    // from video, so decoding it as the video source is visible.
    let mut rt = poisoned(9);
    rt[0..2].copy_from_slice(&2u16.to_le_bytes()); // sink
    rt[2..4].copy_from_slice(&2u16.to_le_bytes()); // selected: not the shown input
    rt[4..6].copy_from_slice(&1u16.to_le_bytes()); // video
    rt[7..9].copy_from_slice(&3u16.to_le_bytes()); // audio: a different bay again
    h.feed(op::MX_ROUTE, &rt);
    assert_eq!(h.bay(2).video_source.map(|b| b.port), Some(1));
    assert_eq!(h.bay(2).effective_audio_source().map(|b| b.port), Some(3));

    // 0x15 SYS_TEMPERATURE: a count then that many readings
    h.feed(op::SYS_TEMPERATURE, &[2, 41, 43]);
    assert_eq!(h.device().temperatures, vec![41, 43]);

    // 0x44 V2IP_BAY_MAPPINGS: count<<1|is_input, first bay, then uids from 8
    let mapped = uid_n(112);
    let mut bm = poisoned(24);
    bm[0..2].copy_from_slice(&((1u16 << 1) | 1).to_le_bytes());
    bm[2..4].copy_from_slice(&0u16.to_le_bytes()); // first bay number
    bm[8..24].copy_from_slice(mapped.as_bytes());
    h.feed(op::V2IP_BAY_MAPPINGS, &bm);
    assert_eq!(h.bay(1).v2ip_uid, mapped);

    // 0x40 V2IP_TILING addressed at a known device caches as its window
    let mut tl = sender.as_bytes().to_vec();
    tl.resize(24, 0);
    tl[16..18].copy_from_slice(&640u16.to_le_bytes());
    tl[20..22].copy_from_slice(&1920u16.to_le_bytes());
    tl[22..24].copy_from_slice(&1080u16.to_le_bytes());
    h.feed(op::V2IP_TILING, &tl);
    let tiling = h.device().tiling.expect("tiling not recorded");
    assert_eq!(
        (tiling.pos_x, tiling.width, tiling.height),
        (640, 1920, 1080)
    );
}

#[test]
fn command_handlers_reach_their_events() {
    let mut h = bay_state(113);
    let target = uid_n(114);

    h.feed(op::SYS_DISCOVER, &[]);
    h.feed(op::V2IP_DETECT_BAYS, &[]);
    h.feed(op::V2IP_UPGRADE_FPGA, &[]);
    h.feed(op::SYS_MONITORING_PULSE, &[]);
    assert!(h.saw(|e| matches!(e, Event::DiscoverRequest { .. })));
    assert!(h.saw(|e| matches!(e, Event::DetectBaysRequested { .. })));
    assert!(h.saw(|e| matches!(e, Event::UpgradeFpgaRequested { .. })));
    assert!(h.saw(|e| matches!(e, Event::MonitoringPulse { .. })));

    h.feed(op::SYS_REBOOT, target.as_bytes());
    assert!(
        h.saw(|e| matches!(e, Event::RebootRequested { request, .. } if request.target == target))
    );

    let mut ep = target.as_bytes().to_vec();
    ep.extend_from_slice(&EdidProfile::UHD_4K.to_wire().to_le_bytes());
    ep.resize(24, 0);
    h.feed(op::BAY_EDID_PROFILE, &ep);
    assert!(h.saw(
        |e| matches!(e, Event::EdidProfileChangeRequested { change, .. }
        if change.target == target && change.profile == EdidProfile::UHD_4K)
    ));

    h.feed(op::V2IP_LINK_REMOTE, target.as_bytes());
    assert!(h.saw(|e| matches!(e, Event::V2ipLinkChanged { target: t, .. } if *t == target)));
}

/// 0x1F V2IP_SOURCE_SWITCH: a sink is told which groups to subscribe to, and
/// the sources are resolved back to the bays advertising those addresses.
#[test]
fn v2ip_source_switch_resolves_the_advertising_bay() {
    let mut h = Harness::new(120);
    let src = h.sender;
    h.hello(0x28, "ONEIP-TX", "TX9", DeviceFeature::V2IP_SOURCE);
    h.feed(
        op::SYS_BAY_CONFIG,
        &bay_config_rec(
            1,
            0,
            0,
            "Input 1",
            "Apple TV",
            BayStatus::NONE,
            BayFeatures::HDMI_IN,
        ),
    );
    h.feed(
        op::SYS_BAY_V2IP_SOURCES,
        &stream_rec(src, "239.5.5.1", "239.5.5.2", "239.5.5.3", V2IP_PORT_VIDEO),
    );

    let sink = uid_n(121);
    h.feed_as(
        sink,
        op::SYS_HELLO,
        &hello_payload(0x28, "ONEIP-RX", "RX9", "4.8.0", DeviceFeature::V2IP_SINK),
    );
    h.feed_as(
        sink,
        op::SYS_BAY_CONFIG,
        &bay_config_rec(
            1,
            1,
            0,
            "Output 1",
            "TV",
            BayStatus::NONE,
            BayFeatures::V2IP_SINK_LOCAL,
        ),
    );

    let mut p = sink.as_bytes().to_vec();
    p.extend_from_slice(&[239, 5, 5, 1]); // video group
    p.extend_from_slice(&[239, 5, 5, 2]); // audio group
    h.feed_as(sink, op::V2IP_SOURCE_SWITCH, &p);

    let out = h
        .state
        .bay(crate::wire::BayUid::new(sink, 1))
        .expect("sink bay");
    assert_eq!(
        out.video_source.map(|b| b.device),
        Some(src),
        "video source did not resolve to the advertising device"
    );
}

/// The bay descriptor underpins most of the read API - names, ports, sources,
/// EDID profile, remote-control target, status and features all come from this
/// one record. Every field gets a distinct value, so reading any of them at a
/// neighbour's offset changes the result.
#[test]
fn bay_config_decodes_every_field() {
    // Packed: port 0, mode 1, bay 2, a 2-byte union at 3, name 5, user name
    // 21, mxr_cfg_signal 37 (a 14-byte description then a 2-byte type),
    // status 53, features 57. 61 bytes.
    let mut rec = poisoned(crate::wire::BAY_CONFIG_SIZE);
    rec[0] = 7; // port
    rec[1] = 1; // mode: output
    rec[2] = 3; // bay number
    rec[3] = 11; // video source, or the low byte of the EDID/RC union
    rec[4] = 22; // audio source, or its high byte
    rec[5..21].copy_from_slice(b"0123456789ABCDEF"); // fills the field: no terminator
    field(&mut rec, 21, 16, "Living Room TV");
    rec[37..51].copy_from_slice(b"1080p60 444 8b"); // exactly 14: no terminator either
    rec[51..53].copy_from_slice(&0x2013u16.to_le_bytes());
    rec[53..57].copy_from_slice(&BayStatus::HIDDEN.bits().to_le_bytes());
    rec[57..61].copy_from_slice(&BayFeatures::HDMI_OUT.bits().to_le_bytes());

    let c = parse_bay_config(&rec).expect("record did not parse");
    assert_eq!((c.port, c.modenum, c.bay), (7, 1, 3));
    assert_eq!((c.video_source, c.audio_source), (11, 22));
    // The same two bytes are a 12-bit EDID profile and a 4-bit remote-control
    // target on a source bay.
    assert_eq!(
        c.edid_profile,
        EdidProfile::from_wire((22 & 0x0F) << 8 | 11)
    );
    assert_eq!(c.rc_type, RcType::from_wire(1));
    assert_eq!(c.bay_name, "0123456789ABCDEF");
    assert_eq!(c.user_name, "Living Room TV");
    // The description stops at 14; the two bytes after it are the signal type.
    assert_eq!(c.signal_type, "1080p60 444 8b");
    assert_eq!(c.signal_mode.svd(), 0x13);
    assert_eq!(c.signal_mode.bpp(), Some(8));
    assert_eq!(c.status, BayStatus::HIDDEN);
    assert_eq!(c.features, BayFeatures::HDMI_OUT);
}

/// The same record reaching a bay through the dispatcher.
#[test]
fn bay_config_reaches_the_bay() {
    let mut h = Harness::new(130);
    h.hello(0x28, "FF88", "BC0001", DeviceFeature::VIDEO_ROUTING);

    let mut rec = bay_config_rec(
        9,
        1,
        2,
        "Output 9",
        "Kitchen",
        BayStatus::HIDDEN,
        BayFeatures::HDMI_OUT,
    );
    rec[37..51].copy_from_slice(b"2160p50 420 10");
    rec[51..53].copy_from_slice(&0x4062u16.to_le_bytes()); // svd 98, bpp index 2 = 10
    h.feed(op::SYS_BAY_CONFIG, &rec);

    let bay = h.bay(9);
    assert_eq!(bay.user_name(), "Kitchen");
    assert_eq!(bay.port_name, "Output 9");
    assert_eq!(bay.hidden, Some(true));
    assert_eq!(bay.features, BayFeatures::HDMI_OUT);
    assert_eq!(bay.signal_type.as_deref(), Some("2160p50 420 10"));
    assert_eq!(bay.signal_mode.svd(), 98);
    assert_eq!(bay.signal_mode.bpp(), Some(10));
    assert_eq!(bay.arc, ArcStatus::Inactive);
    assert_eq!(bay.signal_mode, MxrSignalType::from_wire(0x4062));
}

/// An amplifier with one zone on the given port.
fn amp_state(n: u8, port: u8) -> Harness {
    let mut h = Harness::new(n);
    h.hello(0x28, "ProAmp8", "AMP0001", DeviceFeature::AUDIO_AMPLIFIER);
    h.feed(
        op::SYS_BAY_CONFIG,
        &bay_config_rec(
            port,
            1,
            0,
            "Zone 4",
            "Kitchen",
            BayStatus::NONE,
            BayFeatures::AUDIO_AMP_OUT,
        ),
    );
    h
}

/// Amp zone settings, laid out from the C declaration. Every field gets a
/// distinct value, and the delays exceed 65535 samples - the range a reading
/// that took the two padding bytes as part of the value could not represent.
#[test]
fn amp_zone_settings_decode() {
    let mut h = amp_state(140, 4);
    let sender = h.sender;

    const DELAY_L: u32 = 96_000; // 2s at 48kHz
    const DELAY_R: u32 = 144_000; // 3s
    let mut p = poisoned(56);
    p[0..16].copy_from_slice(sender.as_bytes());
    p[16..18].copy_from_slice(&4u16.to_le_bytes()); // zone
    p[18..22].copy_from_slice(&[200, 201, 12, 220]);
    p[24..28].copy_from_slice(&DELAY_L.to_le_bytes());
    p[28..32].copy_from_slice(&DELAY_R.to_le_bytes());
    p[32..37].copy_from_slice(&[130, 131, 1, 2, 33]);
    p[40..44].copy_from_slice(&900u32.to_le_bytes());
    p[44..49].copy_from_slice(&[120, 121, 122, 123, 124]);
    p[49..54].copy_from_slice(&[140, 141, 142, 143, 144]);
    h.feed(op::AMP_ZONE_SETTINGS, &p);

    let s = h.bay(4).amp_settings.expect("no amp settings");
    assert!(
        h.saw(|e| matches!(e, Event::AmpZoneSettingsChanged { .. })),
        "event did not fire"
    );
    assert_eq!((s.gain_left, s.gain_right), (200, 201));
    assert_eq!((s.volume_min, s.volume_max), (12, 220));
    assert_eq!(
        (s.delay_left, s.delay_right),
        (DELAY_L, DELAY_R),
        "padding read as part of the delay?"
    );
    assert_eq!((s.bass, s.treble), (130, 131));
    assert_eq!((s.bridged, s.power_mode, s.power_level), (1, 2, 33));
    assert_eq!(s.power_timeout, 900);
    assert_eq!(s.eq_left, [120, 121, 122, 123, 124]);
    assert_eq!(s.eq_right, [140, 141, 142, 143, 144]);
}

/// 0x3E: a target uid, then the Dolby mode byte and a flags byte.
#[test]
fn amp_dolby_decode() {
    let mut h = Harness::new(141);
    h.hello(0x28, "ProAmp8", "AMP0002", DeviceFeature::AUDIO_AMPLIFIER);
    let sender = h.sender;

    let mut p = poisoned(24);
    p[0..16].copy_from_slice(sender.as_bytes());
    p[16] = 2; // four-zone Dolby mode
    p[17] = 0x1 | 0x4; // upmix and upmix active; Dolby not detected
    h.feed(op::AMP_DOLBY_STATE, &p);

    let d = h.device().dolby_settings.expect("no dolby settings");
    assert_eq!(d.mode, 2);
    assert!(d.pcm_upmix && !d.dolby_detected && d.pcm_upmix_active);

    // A frame short of the struct changed nothing on the amp, and its flag
    // byte would otherwise fall back to zero and report every flag clear.
    let mut short = poisoned(18);
    short[0..16].copy_from_slice(sender.as_bytes());
    short[16] = 1;
    short[17] = 0x2;
    h.feed(op::AMP_DOLBY_STATE, &short);
    let d = h.device().dolby_settings.expect("no dolby settings");
    assert_eq!(
        d.mode, 2,
        "a frame short of the struct replaced the settings"
    );
}

/// The amp allocates the whole struct and writes through a struct pointer, so
/// the wire image carries the compiler's padding and 2-byte tail. A frame of
/// only the 54 bytes the fields occupy is not one the amp sends.
#[test]
fn amp_zone_settings_require_the_full_struct() {
    let mut h = amp_state(142, 4);
    let sender = h.sender;

    let mut short = poisoned(55);
    short[0..16].copy_from_slice(sender.as_bytes());
    short[16..18].copy_from_slice(&4u16.to_le_bytes());
    short[18] = 200;
    h.feed(op::AMP_ZONE_SETTINGS, &short);
    assert!(h.bay(4).amp_settings.is_none(), "a short frame was decoded");
}

/// An amp leaves the payload target zeroed and identifies itself in the frame
/// header, so the fallback to the sender is the normal path rather than an
/// edge case.
#[test]
fn amp_settings_with_a_zero_target_use_the_sender() {
    let mut h = amp_state(143, 2);

    let mut p = poisoned(56);
    p[0..16].fill(0); // target left zero, as the amp sends it
    p[16..18].copy_from_slice(&2u16.to_le_bytes());
    p[18] = 190;
    p[19] = 191;
    p[24..28].copy_from_slice(&48_000u32.to_le_bytes());
    h.feed(op::AMP_ZONE_SETTINGS, &p);

    let s = h
        .bay(2)
        .amp_settings
        .expect("a zero target should resolve to the sender");
    assert_eq!((s.gain_left, s.delay_left), (190, 48_000));
}

/// A tone byte outside the amp's HTTP bounds still decodes: the mesh receive
/// path copies these through without a range check, so the wire can carry any
/// value and this library reports what arrived rather than what one device's
/// own API would have accepted.
#[test]
fn amp_tone_outside_the_http_bounds_is_not_clamped() {
    let mut h = amp_state(144, 1);
    let sender = h.sender;

    let mut p = poisoned(56);
    p[0..16].copy_from_slice(sender.as_bytes());
    p[16..18].copy_from_slice(&1u16.to_le_bytes());
    p[32] = AMP_TONE_HTTP_MAX + 12; // above what the HTTP API allows
    p[33] = AMP_TONE_HTTP_MIN - 12; // below it
    h.feed(op::AMP_ZONE_SETTINGS, &p);

    let s = h.bay(1).amp_settings.expect("no amp settings");
    assert_eq!(
        (s.bass, s.treble),
        (AMP_TONE_HTTP_MAX + 12, AMP_TONE_HTTP_MIN - 12),
        "tone should be reported unclamped"
    );
}

/// The 0x22 layout reorders the struct and is the only form this decodes: the
/// three older ones are extensions of each other and are read elsewhere.
#[test]
fn network_status_layouts() {
    use crate::rx::network::parse_modern;
    use crate::types::MacAddress;
    use std::net::Ipv4Addr;

    // The later 0x22 form, as measured on a live mesh: name at 4, MAC at 21.
    let mut late = poisoned(144);
    late[0..2].copy_from_slice(&4u16.to_le_bytes());
    late[2..4].copy_from_slice(&((1u16 << 3) | (1 << 6)).to_le_bytes()); // igmp + uplink
    field(&mut late, 4, 17, "UTP PoE+");
    late[21..27].copy_from_slice(&[0x00, 0x15, 0x82, 0x13, 0x89, 0xae]);
    late[28..32].copy_from_slice(&[10, 8, 83, 228]);
    late[32..36].copy_from_slice(&[10, 8, 8, 254]);

    let s = parse_modern(&late).expect("late form did not parse");
    assert_eq!((s.port, s.name.as_str()), (4, "UTP PoE+"));
    assert_eq!(
        s.mac_address,
        Some(MacAddress([0x00, 0x15, 0x82, 0x13, 0x89, 0xae]))
    );
    assert_eq!(s.ip, Some(Ipv4Addr::new(10, 8, 83, 228)));
    assert_eq!(s.querier, Some(Ipv4Addr::new(10, 8, 8, 254)));

    // A payload whose feature word runs past a byte is still this layout:
    // the field was widened so it could, and nothing about a set high byte
    // makes a report older.
    let mut wide_features = late.clone();
    wide_features[3] = 0x01;
    let s = parse_modern(&wide_features).expect("a wide feature word did not parse");
    assert_eq!(
        (s.port, s.name.as_str()),
        (4, "UTP PoE+"),
        "a set high byte on the feature word moved the fields"
    );
}

/// The legacy struct grew by appending, so a field only exists from the version
/// that added it, and each form is measured at its own size.
///
/// The sizes are what this pins: a device stamping 0x12 sends 144 bytes and one
/// stamping 0x06 sends 136, so a single floor set at the largest form drops
/// every report older than the newest. Each frame here is the length its own
/// stamp produces rather than a buffer long enough for all three.
#[test]
fn network_status_legacy_gating() {
    use crate::rx::network::parse_legacy;
    use std::net::Ipv4Addr;

    let fill = |len: usize| {
        let mut d = poisoned(len);
        field(&mut d, 112, 16, "UTP PoE+");
        if len >= 140 {
            d[132..136].copy_from_slice(&[10, 8, 83, 228]);
            d[136..140].copy_from_slice(&[10, 8, 8, 254]);
        }
        if len >= 152 {
            d[140..146].copy_from_slice(&[0x00, 0x15, 0x82, 0x13, 0x89, 0xae]);
        }
        d
    };

    let s = parse_legacy(&fill(152), 0x21).expect("0x21 form did not parse");
    assert_eq!(s.name, "UTP PoE+");
    assert_eq!(s.ip, Some(Ipv4Addr::new(10, 8, 83, 228)));
    assert!(s.mac_address.is_some());

    // At 0x12 the struct ends before the MAC, so the frame is eight bytes
    // shorter and offset 140 is not part of it.
    let s = parse_legacy(&fill(144), 0x12).expect("0x12 form did not parse");
    assert_eq!(
        s.ip,
        Some(Ipv4Addr::new(10, 8, 83, 228)),
        "0x12 should still carry an address"
    );
    assert_eq!(s.mac_address, None, "0x12 predates the MAC");

    // At 0x06 there are no addresses either, and the frame is shorter again.
    let s = parse_legacy(&fill(136), 0x06).expect("0x06 form did not parse");
    assert_eq!((s.ip, s.querier, s.mac_address), (None, None, None));
    assert_eq!(s.name, "UTP PoE+");

    // A form measured against the next one up is refused outright.
    assert!(
        parse_legacy(&fill(144), 0x21).is_none(),
        "a 0x12-length frame passed the 0x21 floor"
    );
}
