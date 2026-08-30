// Author: Lars Op den Kamp (lars@opdenkamp-it.nl)
// Copyright (c) 2026 Op den Kamp IT Solutions

//! The command and notification opcodes: the frames addressed to a device
//! rather than reporting its state.
//!
//! Most of these are unpacked structs whose padding the firmware never clears,
//! so a field read one byte too wide picks up live stack content rather than a
//! zero. The fixtures here are poisoned wherever that padding is in reach.

use std::net::Ipv4Addr;

use crate::event::Event;
use crate::types::{
    AudioChangeSource, MultiviewerCommand, V2ipDecoderState, VideoWallCommand, VideoWallOp,
    SCALING_FLAG_AUTO_SCALING, SCALING_FLAG_MODE_VALID, SCALING_FLAG_OPTIONS_VALID,
};
use crate::wire::{
    op, BayFeatures, BayStatus, DeviceFeature, DeviceUid, FirmwareType, MultiviewerViewMode,
    RcAction, RcKey, V2IP_DSCP_DEFAULT, V2IP_PORT_ANC, V2IP_PORT_AUDIO, V2IP_PORT_VIDEO,
};

use crate::testing::{bay_config_rec, poisoned, uid_n, Cfg};

use super::Harness;

/// A registry whose single peer is a current OneIP unit.
fn command_device(n: u8) -> Harness {
    let mut h = Harness::new(n);
    h.hello(0x28, "ONEIP", "CM0001", DeviceFeature::VIDEO_ROUTING);
    h
}

/// Builds the `V2IP_AUDIO` command header: a `u16` sub-opcode, two pad bytes
/// and the target uid.
fn audio_cmd(sub: u16, target: DeviceUid) -> Vec<u8> {
    let mut p = Vec::with_capacity(20);
    p.extend_from_slice(&sub.to_le_bytes());
    p.extend_from_slice(&[0, 0]);
    p.extend_from_slice(target.as_bytes());
    p
}

/// Builds the endpoint/value pair an audio command carries after its header.
fn audio_param(endpoint: u16, value: u32) -> [u8; 8] {
    let e = endpoint.to_le_bytes();
    let v = value.to_le_bytes();
    [e[0], e[1], 0, 0, v[0], v[1], v[2], v[3]]
}

/// Builds a multiviewer command body: target uid, sub-opcode, seven pad bytes
/// and the parameters.
fn mv_cmd(target: DeviceUid, sub: u8, args: &[u8]) -> Vec<u8> {
    let mut p = Vec::with_capacity(24 + args.len());
    p.extend_from_slice(target.as_bytes());
    p.push(sub);
    p.extend_from_slice(&[0; 7]);
    p.extend_from_slice(args);
    p
}

/// Writes an address and port into a `v2ip_stream_addr` at `at`.
fn stream_addr(p: &mut [u8], at: usize, ip: &str, port: u16) {
    let addr: Ipv4Addr = ip.parse().expect("test address");
    p[at..at + 4].copy_from_slice(&addr.octets());
    p[at + 4..at + 6].copy_from_slice(&port.to_le_bytes());
}

// ---- routing ----

#[test]
fn a_set_route_addresses_its_bays_as_u16() {
    let mut h = command_device(40);

    // mbay_port_id is a u16, so both bays are two bytes and no_power_on lands
    // at 20. Reading them as bytes at 16 and 17 would put the sink's high byte
    // in the source.
    let mut p = vec![0u8; 21];
    p[0..12].copy_from_slice(b"P9SN00000001");
    p[16..18].copy_from_slice(&300u16.to_le_bytes()); // a sink bay above a byte
    p[18..20].copy_from_slice(&7u16.to_le_bytes());
    p[20] = 1;
    h.feed(op::MX_SET_ROUTE, &p);

    let request = h
        .events
        .iter()
        .find_map(|e| match e {
            Event::SetRouteRequested { request, .. } => Some(request.clone()),
            _ => None,
        })
        .expect("no route request");
    assert_eq!(request.serial, "P9SN00000001");
    assert_eq!(request.sink_bay, 300);
    assert_eq!(request.source_bay, 7);
    assert!(request.no_power_on);
    assert!(!request.audio_only);
}

#[test]
fn an_audio_set_route_has_no_power_on_byte() {
    let mut h = command_device(41);

    // AUDIO_SET_ROUTE addresses its target by serial like MX_SET_ROUTE, but its
    // struct stops after the two bays.
    let mut p = vec![0u8; 20];
    p[0..12].copy_from_slice(b"P9SN00000002");
    p[16..18].copy_from_slice(&4u16.to_le_bytes());
    p[18..20].copy_from_slice(&2u16.to_le_bytes());
    h.feed(op::AUDIO_SET_ROUTE, &p);

    let request = h
        .events
        .iter()
        .find_map(|e| match e {
            Event::SetRouteRequested { request, .. } => Some(request.clone()),
            _ => None,
        })
        .expect("no route request");
    assert!(request.audio_only);
    assert_eq!((request.sink_bay, request.source_bay), (4, 2));
    assert!(!request.no_power_on);
}

// ---- infrared ----

#[test]
fn an_ir_capture_aligns_its_timestamp() {
    let mut h = command_device(42);
    h.feed(
        op::SYS_BAY_CONFIG,
        &bay_config_rec(
            3,
            0,
            0,
            "Input 1",
            "Sky",
            BayStatus::NONE,
            BayFeatures::HDMI_IN,
        ),
    );

    // mxr_ir_data is not packed, so the u32 timestamp aligns to 4 and two
    // padding bytes follow the port.
    let mut p = poisoned(24 + 8);
    p[0..2].copy_from_slice(&3u16.to_le_bytes());
    p[4..8].copy_from_slice(&0xAABB_CCDDu32.to_le_bytes());
    p[8..12].copy_from_slice(&0x1122_3344u32.to_le_bytes());
    p[12..14].copy_from_slice(&2u16.to_le_bytes()); // timer resolution
    p[14..16].copy_from_slice(&38000u16.to_le_bytes()); // carrier frequency
    p[16..18].copy_from_slice(&67u16.to_le_bytes()); // timing count
    p[18..20].copy_from_slice(&0u16.to_le_bytes()); // repeat offset
    p[20] = 1; // status
    p[24..].copy_from_slice(&[1, 2, 3, 4, 5, 6, 7, 8]);
    // RC_IR is gated on protocol 0x19 and up.
    h.feed_proto(op::RC_IR, 0x19, &p);

    let capture = h
        .events
        .iter()
        .find_map(|e| match e {
            Event::IrCaptured { capture, .. } => Some(capture.clone()),
            _ => None,
        })
        .expect("no capture");
    assert_eq!(capture.port, 3);
    assert_eq!(capture.timestamp, 0xAABB_CCDD);
    assert_eq!(capture.last_change, 0x1122_3344);
    assert_eq!(capture.meta.frequency, 38000);
    assert_eq!(capture.meta.nb_timings, 67);
    assert_eq!(capture.meta.status, 1);
    assert_eq!(capture.timings.len(), 8);
}

#[test]
fn ir_transmit_timings_start_at_the_struct_size() {
    let mut h = command_device(70);
    let target = uid_n(71);

    // mxr_tx_ir_data is unpacked and 4-aligned, so the firmware appends the
    // timings at sizeof = 36. Taking them from the end of the last field would
    // shift every u16 timing by two bytes.
    //
    // Poisoned so the padding at 18..20 and the struct tail at 33..36 are not
    // zero: a field read at the right offset but the wrong width shows here.
    let mut p = poisoned(36 + 6);
    p[0..16].copy_from_slice(target.as_bytes());
    p[16] = 1;
    p[17] = 2;
    p[20..24].copy_from_slice(&0xDEAD_BEEFu32.to_le_bytes());
    p[24..26].copy_from_slice(&0u16.to_le_bytes()); // meta.timer_resolution
    p[26..28].copy_from_slice(&38000u16.to_le_bytes()); // meta.frequency
    p[28..30].copy_from_slice(&3u16.to_le_bytes()); // meta.nb_timings
    p[30..32].copy_from_slice(&0u16.to_le_bytes()); // meta.repeat_offset
    p[32] = 0; // meta.status
    p[36..].copy_from_slice(&[1, 0, 2, 0, 3, 0]);
    h.feed(op::RC_IR_TX, &p);

    let request = h
        .events
        .iter()
        .find_map(|e| match e {
            Event::IrTransmitRequested { request, .. } => Some(request.clone()),
            _ => None,
        })
        .expect("no transmit request");
    assert_eq!(request.target, target);
    assert_eq!((request.local_mode, request.local_bay), (1, 2));
    assert_eq!(request.timestamp, 0xDEAD_BEEF);
    assert_eq!(request.meta.frequency, 38000);
    assert_eq!(request.meta.nb_timings, 3);
    assert_eq!(
        request.timings,
        [1, 0, 2, 0, 3, 0],
        "the timings start at the struct size, not at the end of the last field"
    );
}

// ---- EDID ----

#[test]
fn a_combined_edid_reply_carries_a_mode_per_record() {
    let mut h = command_device(44);

    // A combined reply is two 257-byte records, so the mode byte leads both
    // halves rather than one mode covering the pair.
    let mut p = vec![0u8; 2 * 257];
    p[0] = 0; // input
    p[1] = 0xAB;
    p[257] = 1; // output
    p[258] = 0xCD;
    h.feed(op::DEV_EDID, &p);

    let records: Vec<_> = h
        .events
        .iter()
        .filter_map(|e| match e {
            Event::EdidReceived { edid, .. } => Some(edid.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(records.len(), 2);
    assert!(!records[0].output);
    assert_eq!(records[0].data.len(), 256);
    assert_eq!(records[0].data[0], 0xAB);
    assert!(records[1].output);
    assert_eq!(records[1].data[0], 0xCD);
}

#[test]
fn an_edid_request_is_not_a_record() {
    let mut h = command_device(45);
    let target = uid_n(99);

    let mut p = target.as_bytes().to_vec();
    p.push(1);
    h.feed(op::DEV_EDID, &p);

    assert!(h.saw(
        |e| matches!(e, Event::EdidRequested { request, .. } if request.target == target && request.output)
    ));
    assert!(
        !h.saw(|e| matches!(e, Event::EdidReceived { .. })),
        "a 17-byte request also decoded as an EDID record"
    );
}

// ---- video wall ----

#[test]
fn a_video_wall_command_separates_a_clear_from_a_revert() {
    let mut h = command_device(46);
    let target = uid_n(77);

    let wall = |op: VideoWallOp, w: u16, h: u16| {
        // Poisoned, so the three pad bytes after the op byte are not zero.
        let mut p = poisoned(32);
        p[0..16].copy_from_slice(target.as_bytes());
        p[16..18].copy_from_slice(&1920u16.to_le_bytes());
        p[18..20].copy_from_slice(&0u16.to_le_bytes());
        p[20..22].copy_from_slice(&w.to_le_bytes());
        p[22..24].copy_from_slice(&h.to_le_bytes());
        p[24..26].copy_from_slice(&3840u16.to_le_bytes());
        p[26..28].copy_from_slice(&2160u16.to_le_bytes());
        p[28] = op.to_wire();
        p
    };
    let latest = |h: &Harness| -> VideoWallCommand {
        h.events
            .iter()
            .rev()
            .find_map(|e| match e {
                Event::VideoWallCommand { command, .. } => Some(*command),
                _ => None,
            })
            .expect("no wall command")
    };

    h.feed(op::V2IP_VIDEO_WALL, &wall(VideoWallOp::STORE, 1920, 1080));
    let got = latest(&h);
    assert_eq!(got.target, target);
    assert_eq!((got.pos_x, got.pos_y), (1920, 0));
    assert_eq!((got.width, got.height), (1920, 1080));
    assert_eq!((got.raster_w, got.raster_h), (3840, 2160));
    assert_eq!(got.op, VideoWallOp::STORE);
    assert!(got.has_window() && !got.is_cleared());

    // A zero width is the wire spelling of "clear the wall", not "unset".
    h.feed(op::V2IP_VIDEO_WALL, &wall(VideoWallOp::PREVIEW, 0, 0));
    assert!(latest(&h).is_cleared());

    // A revert zeroes the geometry and the receiver ignores it, so those zeros
    // are not a clear.
    h.feed(op::V2IP_VIDEO_WALL, &wall(VideoWallOp::REVERT, 0, 0));
    let got = latest(&h);
    assert!(!got.has_window());
    assert!(!got.is_cleared());
}

// ---- remote control ----

#[test]
fn a_key_and_an_action_request_share_their_layout() {
    let mut h = command_device(47);
    let target = uid_n(88);

    let mk = |value: u16| {
        let mut p = vec![0u8; 20];
        p[0..16].copy_from_slice(target.as_bytes());
        p[16..18].copy_from_slice(&300u16.to_le_bytes());
        p[18..20].copy_from_slice(&value.to_le_bytes());
        p
    };
    h.feed(op::RC_TX_KEY, &mk(0x0041));
    h.feed(op::RC_TX_ACTION, &mk(RcAction::POWER_ON.to_wire()));

    assert!(h.saw(|e| matches!(e, Event::KeyTransmitRequested { request, .. }
        if request.target == target && request.local_bay == 300 && request.key == RcKey::from_wire(0x41))));
    assert!(h.saw(
        |e| matches!(e, Event::ActionTransmitRequested { request, .. }
        if request.local_bay == 300 && request.action == RcAction::POWER_ON)
    ));
}

#[test]
fn rc_settings_read_one_flag_byte() {
    let mut h = command_device(53);
    let sender = h.sender;

    let mut p = vec![0u8; 48];
    p[0..16].copy_from_slice(sender.as_bytes());
    p[16..20].copy_from_slice(&7u32.to_le_bytes()); // RC_TARGET_MX_REMOTE
    p[20..24].copy_from_slice(&[10, 8, 80, 30]);
    // CEC on, RC forwarded, status 3.
    p[24..26].copy_from_slice(&(1u16 | (1 << 2) | (3 << 4)).to_le_bytes());
    h.feed(op::RC_SETTINGS, &p);

    let s = h.device().rc_settings.clone().expect("no rc settings");
    assert_eq!(s.rc_target, 7);
    assert_eq!(s.ip, Some(Ipv4Addr::new(10, 8, 80, 30)));
    assert!(s.cec_enabled);
    assert!(!s.cec_auto_on);
    assert!(s.forward_rc);
    assert!(!s.forward_ir);
    assert_eq!(s.rc_status, 3);
}

#[test]
fn an_rc_status_name_starts_past_the_reserved_bits() {
    let mut h = command_device(72);
    let sender = h.sender;

    let mut p = vec![0u8; 48];
    p[0..16].copy_from_slice(sender.as_bytes());
    p[16..20].copy_from_slice(&7u32.to_le_bytes());
    p[20..24].copy_from_slice(&[10, 8, 80, 30]);
    p[24] = 1 | (1 << 3) | (5 << 4); // CEC on, IR forwarded, status 5
                                     // Byte 25 is dead space in the same bitfield container; a decoder reading
                                     // 24..26 as one little-endian u16 and shifting would pick this up.
    p[25] = 0xFF;
    p[28..37].copy_from_slice(b"Detecting");
    h.feed(op::RC_SETTINGS, &p);

    let s = h.device().rc_settings.clone().expect("no rc settings");
    assert!(s.cec_enabled);
    assert!(!s.cec_auto_on);
    assert!(!s.forward_rc);
    assert!(s.forward_ir);
    assert_eq!(s.rc_status, 5);
    assert_eq!(s.status_name, "Detecting");
}

/// Bytes 16..19 and 24..27 of three real `RC_SETTINGS` frames, from units all
/// configured for CEC.
///
/// `rc_target_t` is one byte and the three that follow are padding the
/// firmware never clears, so they carry live stack content that differs per
/// frame. Reading the field as a u32 makes one unchanged setting decode as
/// three different values; the expectation below comes from the units' known
/// configuration, not from what the decoder produces.
const RC_SETTINGS_CAPTURES: [([u8; 4], [u8; 4]); 3] = [
    ([0x01, 0x73, 0x20, 0x28], [0x0f, 0x6f, 0x05, 0x28]),
    ([0x01, 0x6e, 0x1e, 0x28], [0x0f, 0x6f, 0x05, 0x28]),
    ([0x01, 0xb5, 0x1b, 0x28], [0x0f, 0x00, 0x00, 0x00]),
];

#[test]
fn rc_settings_padding_is_not_part_of_the_field() {
    for (index, (rc_target, flags)) in RC_SETTINGS_CAPTURES.iter().enumerate() {
        let mut h = command_device(90 + index as u8);
        let sender = h.sender;

        let mut p = vec![0u8; 48];
        p[0..16].copy_from_slice(sender.as_bytes());
        p[16..20].copy_from_slice(rc_target);
        p[20..24].copy_from_slice(&[10, 8, 80, 30]);
        p[24..28].copy_from_slice(flags);
        // status_name stays empty: firmware writes a NUL and returns for any
        // non-network target, so a CEC unit cannot legitimately report one.
        h.feed(op::RC_SETTINGS, &p);

        let s = h.device().rc_settings.clone().expect("no rc settings");
        // RC_TARGET_CEC, identical across all three despite the padding.
        assert_eq!(s.rc_target, 1, "capture {index} swallowed the padding");
        assert!(
            s.cec_enabled && s.cec_auto_on && s.forward_rc && s.forward_ir,
            "capture {index}: 0x0f should set all four flags"
        );
        assert_eq!(s.rc_status, 0, "capture {index}");
        assert_eq!(s.status_name, "", "capture {index}");
    }
}

// ---- device state ----

#[test]
fn setup_status_installer_and_pdu() {
    let mut h = command_device(48);

    h.feed(op::SETUP_STATUS, &[1]);
    assert_eq!(h.device().setup_done, Some(true));

    h.feed(op::SET_INSTALLER, &[0x34, 0x12]);
    assert_eq!(h.device().installer_id, Some(0x1234));

    let mut p = vec![0u8; 32];
    p[0..4].copy_from_slice(&0x4000_0000u32.to_le_bytes()); // 2.0 A
    p[4..8].copy_from_slice(&0x42F0_0000u32.to_le_bytes()); // 120.0 V
    p[20..24].copy_from_slice(&0x4248_0000u32.to_le_bytes()); // 50.0 Hz
    p[24] = 1;
    p[25] = 0;
    h.feed(op::PDU_STATE, &p);

    let state = h.device().pdu_state.expect("no pdu state");
    assert_eq!(state.current, 2.0);
    assert_eq!(state.voltage, 120.0);
    assert_eq!(state.frequency, 50.0);
    assert_eq!(state.outlets[0], 1);
    assert_eq!(state.outlets[1], 0);
}

#[test]
fn a_filter_status_lists_every_uid_past_the_target() {
    let mut h = command_device(49);
    h.feed(
        op::SYS_BAY_CONFIG,
        &bay_config_rec(
            2,
            1,
            0,
            "Output 1",
            "TV",
            BayStatus::NONE,
            BayFeatures::HDMI_OUT,
        ),
    );

    let (a, b) = (uid_n(61), uid_n(62));
    let mut p = h.sender.as_bytes().to_vec();
    p.extend_from_slice(a.as_bytes());
    p.extend_from_slice(b.as_bytes());
    h.feed(op::BAY_FILTER_STATUS, &p);

    assert_eq!(h.bay(2).filtered, [a, b]);
}

#[test]
fn power_save_arrives_in_both_forms() {
    let mut h = command_device(50);

    h.feed(op::V2IP_POWER_SAVE, &[1]);
    assert!(
        h.saw(|e| matches!(e, Event::PowerSaveRequested { request, .. }
        if request.target.is_none() && request.enabled))
    );

    let target = uid_n(55);
    let mut p = target.as_bytes().to_vec();
    p.push(0);
    h.feed(op::V2IP_POWER_SAVE, &p);
    assert!(
        h.saw(|e| matches!(e, Event::PowerSaveRequested { request, .. }
        if request.target == Some(target) && !request.enabled))
    );
}

#[test]
fn a_factory_reset_arrives_in_three_forms() {
    let mut h = command_device(51);
    let latest = |h: &Harness| {
        h.events
            .iter()
            .rev()
            .find_map(|e| match e {
                Event::FactoryResetRequested { request, .. } => Some(*request),
                _ => None,
            })
            .expect("no reset request")
    };

    h.feed(op::SYS_FACTORY_RESET, &[0xFF]);
    let got = latest(&h);
    assert!(got.all);
    assert_eq!(got.target, None);

    let target = uid_n(56);
    h.feed(op::SYS_FACTORY_RESET, target.as_bytes());
    let got = latest(&h);
    assert!(!got.all);
    assert_eq!(got.target, Some(target));

    // Neither form: the request addresses only the sender.
    h.feed(op::SYS_FACTORY_RESET, &[]);
    let got = latest(&h);
    assert!(!got.all);
    assert_eq!(got.target, None);
}

#[test]
fn a_bay_name_filling_its_field_keeps_every_character() {
    let mut h = command_device(52);
    let target = uid_n(57);

    let full = "0123456789ABCDEF"; // fills the field, so it carries no terminator
    let mut p = target.as_bytes().to_vec();
    p.extend_from_slice(&300u16.to_le_bytes());
    p.extend_from_slice(full.as_bytes());
    h.feed(op::CHANGE_BAY_NAME, &p);

    assert!(
        h.saw(|e| matches!(e, Event::BayNameChangeRequested { change, .. }
        if change.target == target && change.port == 300 && change.name == full))
    );
}

// ---- audio ----

#[test]
fn an_audio_select_input_names_its_sink_twice() {
    let mut h = command_device(60);
    let sender = h.sender;
    let source = uid_n(61);

    // The body names the sink again at 20 with the source at 36. Decoding
    // those the other way round swaps source and sink.
    let mut p = audio_cmd(3, sender);
    p.extend_from_slice(sender.as_bytes());
    p.extend_from_slice(source.as_bytes());
    p.extend_from_slice(&[7, 0, 9, 0]); // sink endpoint 7, source endpoint 9
    h.feed(op::V2IP_AUDIO, &p);

    let want = AudioChangeSource {
        source_uid: source,
        source_id: 9,
        target_uid: sender,
        target_id: 7,
    };
    assert!(h.saw(|e| matches!(e, Event::AudioSelectInput { change, .. } if *change == want)));
    assert_eq!(h.device().audio_select, Some(want));
}

#[test]
fn audio_endpoint_commands_carry_an_endpoint_and_a_value() {
    let mut h = command_device(64);
    let sender = h.sender;
    let mut send = |sub: u16, endpoint: u16, value: u32| {
        let mut p = audio_cmd(sub, sender);
        p.extend_from_slice(&audio_param(endpoint, value));
        h.feed(op::V2IP_AUDIO, &p);
    };
    send(1, 2, 1); // mute
    send(2, 3, 0); // trigger
    send(4, 4, 80); // volume

    assert!(h.saw(|e| matches!(
        e,
        Event::AudioEndpointMute {
            endpoint: 2,
            muted: true,
            ..
        }
    )));
    assert!(h.saw(|e| matches!(
        e,
        Event::AudioEndpointTrigger {
            endpoint: 3,
            active: false,
            ..
        }
    )));
    assert!(h.saw(|e| matches!(
        e,
        Event::AudioEndpointVolume {
            endpoint: 4,
            volume: 80,
            ..
        }
    )));
}

// ---- multiviewer ----

#[test]
fn every_multiviewer_sub_command_surfaces() {
    let mut h = command_device(74);
    let sender = h.sender;

    for sub in 0..16u8 {
        h.feed(op::V2IP_MULTIVIEWER, &mv_cmd(sender, sub, &[1, 2, 3]));
    }

    let seen: Vec<u8> = h
        .events
        .iter()
        .filter_map(|e| match e {
            Event::MultiviewerCommand { command, .. } => Some(command.op),
            _ => None,
        })
        .collect();
    assert_eq!(seen, (0..16u8).collect::<Vec<_>>());
    assert!(
        h.saw(|e| matches!(e, Event::MultiviewerCommand { command, .. }
        if *command == MultiviewerCommand { target: sender, op: 15, params: vec![1, 2, 3] }))
    );
}

#[test]
fn an_unnamed_enum_value_reaches_the_caller_as_itself() {
    let mut h = command_device(73);
    h.hello(
        0x28,
        "ONEIP-MV",
        "MV9",
        DeviceFeature::V2IP_SINK | DeviceFeature::MULTIVIEWER,
    );

    let mut p = vec![0u8; 190];
    p[169] = 200; // a view mode far beyond anything named
    h.feed(op::V2IP_MULTIVIEWER, &p);
    let status = h.device().multiviewer.clone().expect("no multiviewer");
    assert_eq!(status.view_mode, MultiviewerViewMode::from_wire(200));

    // A firmware type must not read back as a known one either.
    let mut fw = vec![0u8; 12 + 8];
    fw[0] = 42;
    fw[12..17].copy_from_slice(b"9.9.9");
    h.feed(op::FIRMWARE_VERSION, &fw);
    assert!(h
        .device()
        .firmware
        .contains_key(&FirmwareType::from_wire(42)));
}

// ---- statistics ----

#[test]
fn the_stats_blocks_are_twenty_and_forty_four() {
    let mut h = command_device(80);

    // fpga_tx_stats and fpga_rx_stats carry their ALIGN(8) before the struct
    // keyword, where GCC ignores it, so the blocks are 20 and 44 rather than 24
    // and 48. The 128-byte total is stable by accident, so this pins the block
    // boundaries by reading a field from each rather than the total.
    let mut p = vec![0u8; 128];
    p[0..4].copy_from_slice(&11u32.to_le_bytes()); // tx totals, video
    p[20..24].copy_from_slice(&22u32.to_le_bytes()); // tx per minute, video
    p[40..44].copy_from_slice(&33u32.to_le_bytes()); // rx totals, video total
    p[76..80].copy_from_slice(&44u32.to_le_bytes()); // rx totals, anc seq errors
    p[80..84].copy_from_slice(&u32::from(V2ipDecoderState::STARTING.to_wire()).to_le_bytes());
    p[84..88].copy_from_slice(&55u32.to_le_bytes()); // rx per minute, video total
    p[124..128].copy_from_slice(&u32::from(V2ipDecoderState::BAD.to_wire()).to_le_bytes());
    h.feed(op::V2IP_STATS, &p);

    let stats = h.device().v2ip_stats.expect("no stats");
    assert_eq!(stats.tx.video, 11);
    assert_eq!(stats.tx_per_minute.video, 22);
    assert_eq!(stats.rx.video_total, 33);
    assert_eq!(stats.rx.anc_seq_errors, 44);
    assert_eq!(stats.rx_per_minute.video_total, 55);
    assert_eq!(stats.rx.decoder_state, V2ipDecoderState::STARTING);
    assert_eq!(stats.rx_per_minute.decoder_state, V2ipDecoderState::BAD);
}

#[test]
fn a_starting_decoder_is_not_a_verdict() {
    assert_eq!(V2ipDecoderState::STARTING.to_wire(), 3);
    for state in [V2ipDecoderState::UNKNOWN, V2ipDecoderState::STARTING] {
        assert!(!state.is_settled(), "{state} should not be a verdict");
    }
    for state in [V2ipDecoderState::HEALTHY, V2ipDecoderState::BAD] {
        assert!(state.is_settled(), "{state} should be a verdict");
    }
    assert_eq!(
        V2ipDecoderState::STARTING.to_string(),
        "Starting",
        "conflating a starting decoder with an unknown one loses the distinction"
    );
    assert_eq!(V2ipDecoderState::from_wire(9).to_string(), "state 9");
}

// ---- V2IP stream configuration ----

#[test]
fn a_manual_source_switch_and_a_config_sink_block_decode_alike() {
    let mut h = command_device(95);
    h.feed(
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

    // A manual switch: the uid, then video, audio and ancillary at 16, 24 and
    // 32, with an optional audio format at 40.
    let mut p = vec![0u8; 48];
    p[0..16].copy_from_slice(h.sender.as_bytes());
    stream_addr(&mut p, 16, "239.1.1.1", V2IP_PORT_VIDEO);
    stream_addr(&mut p, 24, "239.1.1.2", V2IP_PORT_AUDIO);
    stream_addr(&mut p, 32, "239.1.1.3", V2IP_PORT_ANC);
    p[40..44].copy_from_slice(&96000u32.to_le_bytes());
    p[44] = 6;
    h.feed(op::V2IP_MANUAL_SRC_SWITCH, &p);

    let sink = h.device().v2ip_sink.expect("no sink");
    assert_eq!(sink.addresses.video.ip, Ipv4Addr::new(239, 1, 1, 1));
    assert_eq!(sink.addresses.video.port, V2IP_PORT_VIDEO);
    assert_eq!(sink.addresses.audio.ip, Ipv4Addr::new(239, 1, 1, 2));
    assert_eq!(sink.addresses.anc.ip, Ipv4Addr::new(239, 1, 1, 3));
    let fmt = sink.audio_fmt.expect("no audio format");
    assert_eq!((fmt.sample_rate, fmt.channels), (96000, 6));

    // The same block appended to a device config, at 88 with its format at 112.
    let mut c = vec![0u8; 120];
    c[..88].copy_from_slice(&Cfg::addresses(h.sender, "239.2.2.2").bytes());
    stream_addr(&mut c, 88, "239.3.3.1", V2IP_PORT_VIDEO);
    stream_addr(&mut c, 96, "239.3.3.2", V2IP_PORT_AUDIO);
    stream_addr(&mut c, 104, "239.3.3.3", V2IP_PORT_ANC);
    c[112..116].copy_from_slice(&44100u32.to_le_bytes());
    c[116] = 2;
    h.feed(op::V2IP_DEVICE_CFG, &c);

    let sink = h.device().v2ip_sink.expect("no sink");
    assert_eq!(sink.addresses.video.ip, Ipv4Addr::new(239, 3, 3, 1));
    assert_eq!(sink.addresses.anc.ip, Ipv4Addr::new(239, 3, 3, 3));
    let fmt = sink.audio_fmt.expect("no audio format");
    assert_eq!((fmt.sample_rate, fmt.channels), (44100, 2));
}

#[test]
fn an_options_write_caches_no_noise_bits() {
    let mut h = command_device(96);
    let sender = h.sender;

    // Firmware predating the fix builds this frame from an uninitialised stack
    // local and ORs its scaling flags onto whatever was there, so bits 2..6
    // arrive as noise on any receiver-capable unit. Only bit 7 carries meaning.
    let mut base = Cfg::addresses(sender, "239.1.2.3");
    base.mode = 16;
    base.refresh = 60;
    base.flags = SCALING_FLAG_MODE_VALID | SCALING_FLAG_OPTIONS_VALID | SCALING_FLAG_AUTO_SCALING;
    h.feed(op::V2IP_DEVICE_CFG, &base.bytes());

    // An options-only write carrying garbage in the undefined bits, with
    // auto-scaling genuinely off.
    let noisy = Cfg {
        uid: sender,
        flags: SCALING_FLAG_OPTIONS_VALID | 0x7C,
        ..Cfg::default()
    };
    h.feed(op::V2IP_DEVICE_CFG, &noisy.bytes());

    let scaling = h.device().v2ip_details.expect("no details").scaling;
    assert_eq!(scaling.flags & SCALING_FLAG_AUTO_SCALING, 0);
    assert_eq!(
        scaling.flags & !(SCALING_FLAG_MODE_VALID | SCALING_FLAG_OPTIONS_VALID),
        0,
        "undefined flag bits survived a merge: {:#04x}",
        scaling.flags
    );
    assert_eq!(scaling.mode.svd(), 16);
    assert_eq!(scaling.refresh, 60);

    // A genuine auto-scaling bit still arrives beside the noise.
    let on = Cfg {
        uid: sender,
        flags: SCALING_FLAG_OPTIONS_VALID | SCALING_FLAG_AUTO_SCALING | 0x34,
        ..Cfg::default()
    };
    h.feed(op::V2IP_DEVICE_CFG, &on.bytes());
    let scaling = h.device().v2ip_details.expect("no details").scaling;
    assert_ne!(scaling.flags & SCALING_FLAG_AUTO_SCALING, 0);
}

/// A real `V2IP_DEVICE_CFG` from a 10.12.32-1 unit, captured off a live mesh.
///
/// The expected values below come from the sending unit's own configuration
/// and from firmware behaviour, not from what this decoder produces.
#[rustfmt::skip]
const DEVICE_CFG_CAPTURE: [u8; 120] = [
    0x27, 0x40, 0x01, 0x04, 0x85, 0xac, 0xb7, 0xaa, 0x3e, 0x7d, 0x2c, 0x67, 0xc6, 0x07, 0x00, 0xf5,
    0xea, 0xda, 0x44, 0xf4, 0x64, 0xc3, 0x00, 0x00, // source.video 234.218.68.244:50020
    0xea, 0xda, 0x44, 0xf5, 0x66, 0xc3, 0x00, 0x00, // source.audio 234.218.68.245:50022
    0xea, 0xda, 0x44, 0xf4, 0x65, 0xc3, 0x00, 0x00, // source.anc   234.218.68.244:50021
    0x5a, 0x90, 0x90, 0x90, 0x00, 0x00, 0x00, 0x00, // tx_rate 90, dscp SET|16 on all three
    0xea, 0xda, 0x44, 0xf6, 0x67, 0xc3, 0x00, 0x00, // audio_return 234.218.68.246:50023
    0x13, 0x20, 0x32, 0x00, 0xdf, 0x1b, 0x00, 0x10, // scaling: svd 19, 8bpp, 50Hz, flags 0xdf
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,             // tiling, uid zero: not carried
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0xea, 0xda, 0x44, 0xf4, 0x64, 0xc3, 0x00, 0x00, // sink.video
    0xea, 0xda, 0x44, 0xf5, 0x66, 0xc3, 0x00, 0x00, // sink.audio
    0xea, 0xda, 0x44, 0xf4, 0x65, 0xc3, 0x00, 0x00, // sink.anc
    0, 0, 0, 0, 0, 0, 0, 0,                         // sink_audio_fmt
];

#[test]
fn a_captured_device_config_decodes_field_for_field() {
    let mut h = command_device(97);
    h.feed(op::V2IP_DEVICE_CFG, &DEVICE_CFG_CAPTURE);

    let details = h.device().v2ip_details.expect("no details");
    // v2ip_stream_source is 8 bytes: the port is a uint_fast16_t, four bytes on
    // ARM, so the two bytes after it belong to the port's field rather than to
    // the next one.
    assert_eq!(details.video.ip, Ipv4Addr::new(234, 218, 68, 244));
    assert_eq!(details.video.port, 50020);
    assert_eq!(details.audio.port, 50022);
    assert_eq!(details.anc.port, 50021);
    assert_eq!(details.arc.ip, Ipv4Addr::new(234, 218, 68, 246));
    assert_eq!(details.arc.port, 50023);
    assert_eq!(details.tx_rate, Some(90));

    // 0x90 is V2IP_DSCP_SET | 16, and 16 is CS2, the boot default.
    assert!(details.dscp.is_complete());
    assert_eq!(details.dscp.video, Some(V2IP_DSCP_DEFAULT));
    assert_eq!(details.dscp.audio, Some(V2IP_DSCP_DEFAULT));
    assert_eq!(details.dscp.anc, Some(V2IP_DSCP_DEFAULT));

    assert_eq!(details.scaling.mode.svd(), 19);
    assert_eq!(details.scaling.mode.bpp(), Some(8));
    assert_eq!(details.scaling.refresh, 50);
    // Flags 0xdf carries bits 2, 3, 4 and 6 as well: this unit predates the fix
    // for the uninitialised mxr_scaling_config, so only bits 0, 1 and 7 mean
    // anything.
    assert_ne!(details.scaling.flags & SCALING_FLAG_AUTO_SCALING, 0);
    assert_eq!(
        details.scaling.flags
            & !(SCALING_FLAG_MODE_VALID | SCALING_FLAG_OPTIONS_VALID | SCALING_FLAG_AUTO_SCALING),
        0,
        "undefined flag bits cached from a 0xdf frame"
    );

    // The tiling block is zeroed, so its uid is zero: not carried, not a clear.
    assert_eq!(h.device().tiling, None);

    let sink = h.device().v2ip_sink.expect("no sink block");
    assert_eq!(sink.addresses.video.port, 50020);
    assert_eq!(sink.addresses.anc.port, 50021);
}

#[test]
fn a_stamped_tiling_block_is_told_from_an_absent_one() {
    let mut h = command_device(98);
    let target = uid_n(99);

    let mut p = DEVICE_CFG_CAPTURE;
    p[64..80].copy_from_slice(target.as_bytes());
    p[80..82].copy_from_slice(&1920u16.to_le_bytes());
    p[84..86].copy_from_slice(&3840u16.to_le_bytes());
    p[86..88].copy_from_slice(&2160u16.to_le_bytes());
    h.feed(op::V2IP_DEVICE_CFG, &p);

    let tiling = h.device().tiling.expect("no window");
    assert_eq!(tiling.target, target);
    assert_eq!(tiling.pos_x, 1920);
    assert_eq!((tiling.width, tiling.height), (3840, 2160));

    // A stamped uid with zero geometry is a real clear, and must still cache.
    p[80..88].fill(0);
    h.feed(op::V2IP_DEVICE_CFG, &p);
    let tiling = h.device().tiling.expect("a stamped clear was dropped");
    assert_eq!((tiling.width, tiling.height), (0, 0));

    // A zero-uid block must leave that cached clear alone.
    p[64..80].fill(0);
    p[84..86].copy_from_slice(&1234u16.to_le_bytes());
    h.feed(op::V2IP_DEVICE_CFG, &p);
    let tiling = h
        .device()
        .tiling
        .expect("an uncarried block cleared the cache");
    assert_eq!(tiling.width, 0);
}
