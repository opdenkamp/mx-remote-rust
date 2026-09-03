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
    AudioChangeSource, MultiviewerCommand, V2ipDecoderDetail, V2ipDecoderFormat, V2ipDecoderReason,
    V2ipDecoderState, VideoWallCommand, VideoWallOp, SCALING_FLAG_AUTO_SCALING,
    SCALING_FLAG_MODE_VALID, SCALING_FLAG_OPTIONS_VALID,
};
use crate::wire::{
    op, BayFeatures, BayStatus, DeviceFeature, DeviceUid, FirmwareType, MultiviewerViewMode,
    RcAction, RcKey, PROTOCOL_VERSION, V2IP_DSCP_DEFAULT, V2IP_PORT_ANC, V2IP_PORT_AUDIO,
    V2IP_PORT_VIDEO,
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

/// A burst with no timings is not a capture.
///
/// The struct ends at 24 and a receiver measures one timing past it before it
/// reads a field, so a frame that stops at the struct is one nothing acted on
/// - and it carries no burst to hand a caller either.
#[test]
fn an_ir_frame_with_no_timings_is_not_a_capture() {
    let mut h = command_device(43);
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

    let mut p = poisoned(24);
    p[0..2].copy_from_slice(&3u16.to_le_bytes());
    h.feed_proto(op::RC_IR, 0x19, &p);
    assert!(
        !h.saw(|e| matches!(e, Event::IrCaptured { .. })),
        "a frame that stops at the struct was reported as a burst"
    );
}

/// A blast request with no timings is not a request.
///
/// The addressed device measures the struct plus one timing before it looks at
/// anything else, so a request that stops at the struct asks it for nothing.
#[test]
fn an_ir_request_with_no_timings_is_not_a_request() {
    let mut h = command_device(44);
    let target = uid_n(45);

    let mut p = poisoned(36);
    p[0..16].copy_from_slice(target.as_bytes());
    h.feed(op::RC_IR_TX, &p);
    assert!(
        !h.saw(|e| matches!(e, Event::IrTransmitRequested { .. })),
        "a request that stops at the struct reached the caller"
    );
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

/// The length check is a floor, matching the device's.
///
/// A sink accepts a payload longer than the struct and ignores the tail, which
/// is the room the frame has to grow in. A decoder that demanded the exact
/// size would refuse traffic the sinks are already honouring, and would do it
/// on the wire rather than at a version boundary anyone could see coming.
#[test]
fn a_longer_video_wall_frame_is_read_and_its_tail_ignored() {
    let mut h = command_device(47);
    let target = uid_n(78);

    let mut p = poisoned(48);
    p[0..16].copy_from_slice(target.as_bytes());
    p[16..18].copy_from_slice(&64u16.to_le_bytes());
    p[18..20].copy_from_slice(&128u16.to_le_bytes());
    p[20..22].copy_from_slice(&1920u16.to_le_bytes());
    p[22..24].copy_from_slice(&1080u16.to_le_bytes());
    p[24..26].copy_from_slice(&3840u16.to_le_bytes());
    p[26..28].copy_from_slice(&2160u16.to_le_bytes());
    p[28] = VideoWallOp::STORE.to_wire();
    // Everything from 29 on stays poisoned: a field read past the struct picks
    // it up, and the assertions below are all inside the struct.
    h.feed(op::V2IP_VIDEO_WALL, &p);

    let got = h
        .events
        .iter()
        .rev()
        .find_map(|e| match e {
            Event::VideoWallCommand { command, .. } => Some(*command),
            _ => None,
        })
        .expect("a frame longer than the struct was dropped");
    assert_eq!(got.target, target);
    assert_eq!((got.pos_x, got.pos_y), (64, 128));
    assert_eq!((got.width, got.height), (1920, 1080));
    assert_eq!((got.raster_w, got.raster_h), (3840, 2160));
    assert_eq!(got.op, VideoWallOp::STORE);

    // The paired direction: one byte short of the struct is still dropped, so
    // the acceptance above is not one a decoder without any check would give.
    let before = h.events.len();
    h.feed(op::V2IP_VIDEO_WALL, &poisoned(31));
    assert_eq!(
        h.events.len(),
        before,
        "a payload shorter than the struct produced an event"
    );
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
/// The control method is one byte and the three that follow are padding the
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
fn setup_status_and_installer() {
    let mut h = command_device(48);

    h.feed(op::SETUP_STATUS, &[1]);
    assert_eq!(h.device().setup_done, Some(true));

    h.feed(op::SET_INSTALLER, &[0x34, 0x12]);
    assert_eq!(h.device().installer_id, Some(0x1234));
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

    // A payload that reaches no form is not the empty one. Reading it as the
    // sender resetting itself would report a destructive request built from a
    // frame nothing understood, so it is dropped and the last request stands.
    let before = h.events.len();
    h.feed(op::SYS_FACTORY_RESET, &[0x01, 0x02, 0x03]);
    assert!(
        !h.events[before..]
            .iter()
            .any(|e| matches!(e, Event::FactoryResetRequested { .. })),
        "a payload matching no form was read as a reset of the sender"
    );
}

#[test]
fn a_payload_that_grew_at_the_back_is_still_read() {
    // A protocol update appends to an opcode's payload and leaves the fields
    // ahead of the addition where they were, which is the only way a newer
    // device stays readable by an older client. A handler that requires an
    // exact length reads such a frame as neither of the forms it knows, and
    // drops or misfiles the whole thing - fields it does understand included.
    // The symptom appears only once a device is upgraded, so it is asserted
    // here rather than left to the first field that gets added.
    let mut h = command_device(52);
    let target = uid_n(64);
    let grown = |base: &[u8]| {
        let mut p = base.to_vec();
        p.extend_from_slice(&[0x5A, 0x5B, 0x5C]);
        p
    };

    let mut record = vec![0u8; 257];
    record[0] = 1; // output
    record[1] = 0xAB;
    h.feed(op::DEV_EDID, &grown(&record));
    assert!(
        h.saw(|e| matches!(e, Event::EdidReceived { edid, .. }
        if edid.output && edid.data.len() == 256 && edid.data[0] == 0xAB)),
        "an EDID record with bytes appended was not read as a record"
    );

    let mut request = target.as_bytes().to_vec();
    request.push(1);
    h.feed(op::DEV_EDID, &grown(&request));
    assert!(
        h.saw(|e| matches!(e, Event::EdidRequested { request, .. }
        if request.target == target && request.output)),
        "an EDID request with bytes appended was not read as a request"
    );

    let mut power = target.as_bytes().to_vec();
    power.push(1);
    h.feed(op::V2IP_POWER_SAVE, &grown(&power));
    assert!(
        h.saw(|e| matches!(e, Event::PowerSaveRequested { request, .. }
        if request.target == Some(target) && request.enabled)),
        "an addressed power-save request with bytes appended lost its target"
    );

    h.feed(op::SYS_FACTORY_RESET, &grown(target.as_bytes()));
    assert!(
        h.saw(|e| matches!(e, Event::FactoryResetRequested { request, .. }
        if request.target == Some(target) && !request.all)),
        "a factory reset with bytes appended lost the device it names"
    );
}

#[test]
fn a_reply_holds_as_many_edid_records_as_it_is_long() {
    // The count is not enumerated: the output flag leads every record, so a
    // reply carrying a third is three records rather than an unknown length.
    let mut h = command_device(53);
    let mut p = vec![0u8; 3 * 257];
    for (i, mark) in [0xA1u8, 0xA2, 0xA3].iter().enumerate() {
        p[i * 257] = u8::from(i == 1); // only the middle record is an output
        p[i * 257 + 1] = *mark;
    }
    h.feed(op::DEV_EDID, &p);

    let records: Vec<_> = h
        .events
        .iter()
        .filter_map(|e| match e {
            Event::EdidReceived { edid, .. } => Some(edid.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(records.len(), 3, "a three-record reply was not read");
    assert_eq!(
        records.iter().map(|r| r.data[0]).collect::<Vec<_>>(),
        [0xA1, 0xA2, 0xA3]
    );
    assert_eq!(
        records.iter().map(|r| r.output).collect::<Vec<_>>(),
        [false, true, false],
        "the flag leading each record was not read per record"
    );
}

#[test]
fn a_filter_list_is_read_to_its_last_whole_uid() {
    let mut h = command_device(54);
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

    let filtered = uid_n(65);
    let mut p = h.sender.as_bytes().to_vec();
    p.extend_from_slice(filtered.as_bytes());
    p.extend_from_slice(&[0x5A, 0x5B, 0x5C]);
    h.feed(op::BAY_FILTER_STATUS, &p);

    assert_eq!(
        h.bay(2).filtered,
        [filtered],
        "a trailing partial uid was read as a malformed frame rather than ignored"
    );
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

    // The 192 bytes a status report carries; a shorter one is not decoded.
    let mut p = vec![0u8; 192];
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
fn a_stats_request_is_not_a_report() {
    // The opcode carries a request as well as a report, and the request is far
    // short of the counters. The length test that separates them also guards
    // the slicing that reads the counter blocks, so failing it is not a missed
    // reading but a panic on a frame a device legitimately sends - and this
    // client would take itself down decoding traffic it asked for.
    let mut h = command_device(89);
    let mut request = uid_n(70).as_bytes().to_vec();
    request.push(1); // enable

    h.feed(op::V2IP_STATS, &request);
    assert!(
        h.device().v2ip_stats.is_none(),
        "a request was read as a report"
    );

    // A payload one byte short of the counters is the other side of the same
    // gate: nothing about it says request, and it must still not be sliced.
    h.feed(op::V2IP_STATS, &[0u8; 127]);
    assert!(
        h.device().v2ip_stats.is_none(),
        "a truncated report was read as a whole one"
    );
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

/// A statistics report with the decoder block appended, poisoned everywhere a
/// caller then writes nothing.
///
/// The counter blocks in front of it are left poisoned: the block is read from
/// the payload length rather than from anything in them, and a decode that
/// reached into them would read a number rather than a zero.
fn stats_with_decoder(valid: u8) -> Vec<u8> {
    let mut p = poisoned(152);
    p[128] = valid;
    p[129] = V2ipDecoderReason::FORMAT_MISMATCH.to_wire();
    p[130] = 0; // blocking
    p[131] = 0x77; // reserved, and never a colour depth
    p[132..134].copy_from_slice(&3840u16.to_le_bytes());
    p[134..136].copy_from_slice(&2160u16.to_le_bytes());
    p[136..138].copy_from_slice(&V2ipDecoderFormat::YCBCR_422.to_wire().to_le_bytes());
    p[138..140].copy_from_slice(&600u16.to_le_bytes());
    // Bit 20 is a cause this build does not name, and it is what makes the
    // word's width readable: every cause it does name fits in the low half.
    p[140..144].copy_from_slice(&((1u32 << 4) | (1u32 << 8) | (1u32 << 20)).to_le_bytes());
    p[144..148].copy_from_slice(&100_009u32.to_le_bytes());
    // The block's own tail padding, 20 bytes of fields rounded to 24. A device
    // sends it as zero, having cleared the payload buffer first, so this is
    // poison a parser must ignore rather than anything a sender puts there.
    p[148..152].copy_from_slice(&0xDEADBEEFu32.to_le_bytes());
    p
}

#[test]
fn an_idle_sink_keeps_every_cause_beneath_idle() {
    // A switched-off sink reports idle, and the word can carry causes beneath
    // it: what the decoder still observes is reported whether or not the sink
    // is on. Those bits are a reading, not a fault to escalate - idle outranks
    // them, and a caller testing a fault mask over the whole word would call a
    // sink somebody deliberately switched off broken. What this pins is that
    // the library reports the word as it arrived: suppressing the lower bits
    // here would answer that question for every caller, and answer it by
    // throwing away what the decoder saw.
    //
    // Which causes accompany idle is the sender's business and will change, so
    // this asserts the bits the fixture wrote rather than a word a device is
    // expected to produce.
    let mut h = command_device(88);
    let mut p = stats_with_decoder(1);
    p[129] = V2ipDecoderReason::IDLE.to_wire();
    p[140..144].copy_from_slice(
        &((1u32 << V2ipDecoderReason::NO_PACKETS.to_wire())
            | (1u32 << V2ipDecoderReason::NO_FORMAT.to_wire())
            | (1u32 << V2ipDecoderReason::IDLE.to_wire()))
        .to_le_bytes(),
    );
    h.feed(op::V2IP_STATS, &p);

    let d = h
        .device()
        .v2ip_stats
        .expect("no stats")
        .decoder
        .reading()
        .expect("no decoder reading");
    assert_eq!(d.reason, V2ipDecoderReason::IDLE, "idle did not win");
    for cause in [
        V2ipDecoderReason::NO_PACKETS,
        V2ipDecoderReason::NO_FORMAT,
        V2ipDecoderReason::IDLE,
    ] {
        assert!(
            d.has_cause(cause),
            "{cause} was dropped from a word that carried it"
        );
    }
    assert!(
        !d.has_cause(V2ipDecoderReason::PACKETS_DEGRADED),
        "a cause the word does not carry was reported"
    );
}

#[test]
fn the_decoder_block_is_read_at_its_own_offsets() {
    let mut h = command_device(81);
    let mut p = stats_with_decoder(1);
    // Every field distinct, and the counters in front of the block set so that
    // a decoder reading the block at the wrong base is visible as a counter
    // that moved.
    p[0..4].copy_from_slice(&11u32.to_le_bytes()); // tx totals, video
    p[40..44].copy_from_slice(&33u32.to_le_bytes()); // rx totals, video total
    p[80] = V2ipDecoderState::HEALTHY.to_wire();
    h.feed(op::V2IP_STATS, &p);

    let stats = h.device().v2ip_stats.expect("no stats");
    assert_eq!((stats.tx.video, stats.rx.video_total), (11, 33));
    assert_eq!(stats.rx.decoder_state, V2ipDecoderState::HEALTHY);

    let d = stats
        .decoder
        .reading()
        .expect("a valid decoder block read as no reading");
    assert_eq!(d.reason, V2ipDecoderReason::FORMAT_MISMATCH);
    assert!(
        !d.blocking,
        "the reserved byte beside it is set, and it is not the watchdog flag"
    );
    assert_eq!((d.width, d.height), (3840, 2160));
    assert_eq!(d.format, V2ipDecoderFormat::YCBCR_422);
    assert_eq!(d.updates, 600);
    assert_eq!(d.flags, (1u32 << 4) | (1u32 << 8) | (1u32 << 20));
    assert_eq!(d.blocked_count, 100_009);
    assert!(d.has_geometry());

    // The other direction, so that the flag above is read rather than always
    // false.
    p[130] = 1;
    h.feed(op::V2IP_STATS, &p);
    assert!(
        h.device()
            .v2ip_stats
            .expect("no stats")
            .decoder
            .reading()
            .expect("no reading")
            .blocking
    );
}

#[test]
fn a_report_that_stops_after_the_counters_carries_no_decoder_block() {
    let mut h = command_device(82);

    // What a sender predating the block sends. The bytes that would hold it are
    // absent rather than zero, so nothing here is a reading.
    let p = poisoned(128);
    h.feed(op::V2IP_STATS, &p);
    assert_eq!(
        h.device().v2ip_stats.expect("no stats").decoder,
        V2ipDecoderDetail::Absent
    );

    // And a longer report than this build knows still yields the block, since
    // a version adds to the tail.
    let mut long = stats_with_decoder(1);
    long.extend_from_slice(&poisoned(16));
    h.feed(op::V2IP_STATS, &long);
    let d = h
        .device()
        .v2ip_stats
        .expect("no stats")
        .decoder
        .reading()
        .expect("a tail this build does not know cost it the block it does");
    assert_eq!((d.width, d.height), (3840, 2160));
}

#[test]
fn a_decoder_that_has_never_answered_offers_no_reading() {
    let mut h = command_device(83);

    // Everything behind `valid` still carries what a real reading would, which
    // is what a reader keying on any of it would report as a 4K picture.
    let p = stats_with_decoder(0);
    h.feed(op::V2IP_STATS, &p);
    assert_eq!(
        h.device().v2ip_stats.expect("no stats").decoder,
        V2ipDecoderDetail::NeverAnswered
    );
}

#[test]
fn geometry_says_there_is_no_signal_and_format_never_does() {
    let mut h = command_device(84);

    // A sink with nothing arriving: zero geometry beside a format of zero,
    // which is RGB and is exactly what a real RGB source reads as.
    let mut p = stats_with_decoder(1);
    p[129] = V2ipDecoderReason::NO_PACKETS.to_wire();
    p[132..136].copy_from_slice(&[0; 4]);
    p[136..138].copy_from_slice(&V2ipDecoderFormat::RGB.to_wire().to_le_bytes());
    h.feed(op::V2IP_STATS, &p);

    let d = h
        .device()
        .v2ip_stats
        .expect("no stats")
        .decoder
        .reading()
        .expect("a sink with no stream reports, and this dropped the reading");
    assert_eq!(d.reason, V2ipDecoderReason::NO_PACKETS);
    assert_eq!(d.format, V2ipDecoderFormat::RGB);
    assert!(
        !d.has_geometry(),
        "nothing was recovered, and only the geometry says so"
    );

    // The other direction: a working stream carrying that same format. Both
    // halves pin the format to RGB rather than to each other, so the pair keeps
    // testing what it says it does even if one half is edited later. The reason
    // moves with the geometry because the wire ties them - an idle sink reports
    // no geometry - and the format is what stays put across both.
    let mut p = stats_with_decoder(1);
    p[136..138].copy_from_slice(&V2ipDecoderFormat::RGB.to_wire().to_le_bytes());
    h.feed(op::V2IP_STATS, &p);
    let d = h
        .device()
        .v2ip_stats
        .expect("no stats")
        .decoder
        .reading()
        .expect("no reading");
    assert_eq!(d.format, V2ipDecoderFormat::RGB);
    assert!(
        d.has_geometry(),
        "a working RGB stream read as no signal, which is what format 0 invites"
    );
}

#[test]
fn an_unnamed_format_is_not_an_unknown_colour_space() {
    let mut h = command_device(85);
    let mut p = stats_with_decoder(1);
    p[136..138].copy_from_slice(&255u16.to_le_bytes());
    p[129] = 200; // a cause no build of this library names
    h.feed(op::V2IP_STATS, &p);

    let d = h
        .device()
        .v2ip_stats
        .expect("no stats")
        .decoder
        .reading()
        .expect("no reading");
    assert_eq!(d.format, V2ipDecoderFormat::UNNAMED);
    assert_eq!(d.format.to_wire(), 255);
    assert_ne!(
        d.format.to_wire(),
        0xF,
        "the decoder's unnamed format is not a signal report's unknown colour space"
    );
    assert_eq!(d.reason, V2ipDecoderReason::from_wire(200));
    assert_eq!(d.reason.to_string(), "reason 200");

    // A format wider than a byte, which every value named here is not. Firmware
    // adds formats, and the field is two bytes whether or not one has used them.
    let mut p = stats_with_decoder(1);
    p[136..138].copy_from_slice(&0x0102u16.to_le_bytes());
    h.feed(op::V2IP_STATS, &p);
    assert_eq!(
        h.device()
            .v2ip_stats
            .expect("no stats")
            .decoder
            .reading()
            .expect("no reading")
            .format,
        V2ipDecoderFormat::from_wire(0x0102)
    );
}

#[test]
fn the_flags_word_names_every_cause_but_never_the_first() {
    let mut h = command_device(86);
    let mut p = stats_with_decoder(1);
    p[140..144].copy_from_slice(
        &((1u32 << V2ipDecoderReason::PTP_UNLOCKED.to_wire())
            | (1u32 << V2ipDecoderReason::DECODER_BLOCKED.to_wire()))
        .to_le_bytes(),
    );
    h.feed(op::V2IP_STATS, &p);

    let d = h
        .device()
        .v2ip_stats
        .expect("no stats")
        .decoder
        .reading()
        .expect("no reading");
    assert!(d.has_cause(V2ipDecoderReason::PTP_UNLOCKED));
    assert!(d.has_cause(V2ipDecoderReason::DECODER_BLOCKED));
    assert!(!d.has_cause(V2ipDecoderReason::NO_PACKETS));
    assert!(
        !d.has_cause(V2ipDecoderReason::OK),
        "bit 0 is unused, so ok is never among the causes"
    );
    assert!(
        !d.has_cause(V2ipDecoderReason::from_wire(200)),
        "a cause past the word's width is not in it"
    );
}

#[test]
fn an_idle_sink_reports_whatever_geometry_it_still_detects() {
    // Geometry is read before any cause is decided, so it answers what the
    // decoder currently detects and never whether the sink is on. A parser
    // that zeroed geometry under idle - or that inferred idle from a zero one
    // - would be reading a rank order into a field that has none, and both
    // directions are wrong. A doc comment saying so has no failing state,
    // which is why this is a test: the claim it defends was itself a
    // correction, and nothing else in the suite contradicts a parser that
    // reintroduces it.
    let mut h = command_device(91);
    let mut p = stats_with_decoder(1);
    p[129] = V2ipDecoderReason::IDLE.to_wire();
    p[140..144].copy_from_slice(&(1u32 << V2ipDecoderReason::IDLE.to_wire()).to_le_bytes());
    h.feed(op::V2IP_STATS, &p);

    let d = h
        .device()
        .v2ip_stats
        .expect("no stats")
        .decoder
        .reading()
        .expect("no decoder reading");
    assert_eq!(d.reason, V2ipDecoderReason::IDLE);
    assert_eq!(
        (d.width, d.height),
        (3840, 2160),
        "a switched-off sink had the geometry it still detects taken away"
    );
    assert!(
        d.has_geometry(),
        "idle was read as an answer about signal rather than about the sink"
    );

    // And the other direction: no geometry does not make a sink idle.
    let mut dark = stats_with_decoder(1);
    dark[129] = V2ipDecoderReason::NO_PACKETS.to_wire();
    dark[132..136].copy_from_slice(&[0, 0, 0, 0]);
    h.feed(op::V2IP_STATS, &dark);
    let d = h
        .device()
        .v2ip_stats
        .expect("no stats")
        .decoder
        .reading()
        .expect("no decoder reading");
    assert!(!d.has_geometry());
    assert_eq!(
        d.reason,
        V2ipDecoderReason::NO_PACKETS,
        "a dark decoder was reported as a switched-off sink"
    );
}

#[test]
fn a_video_wall_command_is_read_at_any_stamp() {
    // Deliberately no version floor. The module that owns this opcode never
    // reads the stamp - its gates are the target uid and the length - and its
    // answer to a MatrixOS too old to carry the opcode is to fail registration
    // at load, not to check a version per frame. A floor here would rest on
    // nothing that module could corroborate, and it fails in the direction that
    // costs: too low is free, too high drops every frame from a sender nobody
    // is testing against.
    let mut h = command_device(92);
    let mut p = poisoned(32);
    p[0..16].copy_from_slice(uid_n(92).as_bytes());

    for stamp in [0x00, 0x27, 0x28] {
        h.events.clear();
        h.feed_proto(op::V2IP_VIDEO_WALL, stamp, &p);
        assert!(
            h.saw(|e| matches!(e, Event::VideoWallCommand { .. })),
            "a wall command stamped {stamp:#04x} was dropped"
        );
    }
}

#[test]
fn a_block_sized_tail_is_not_a_decoder_block_at_an_older_stamp() {
    // Length alone says a payload is long enough to hold the block. It cannot
    // say those bytes are that block: a sender stamping a version from before
    // it existed did not append one, so 24 bytes past the counters are some
    // other growth, and reading them as a decoder report invents a reading with
    // a reason, a geometry and a fault word in it. The counters ahead of the
    // tail are unaffected and still read, which is the half a stamp ceiling
    // would have thrown away.
    let mut h = command_device(90);
    let p = stats_with_decoder(1);
    h.feed_proto(op::V2IP_STATS, 0x28, &p);

    let stats = h
        .device()
        .v2ip_stats
        .expect("the counters were lost with it");
    assert_eq!(stats.tx.video, u32::from_le_bytes([0xA5, 0xA4, 0xA7, 0xA6]));
    assert_eq!(
        stats.decoder,
        V2ipDecoderDetail::Absent,
        "a tail from before the block existed was read as one"
    );
}

#[test]
fn a_report_stamped_above_this_clients_own_version_is_still_read() {
    let mut h = command_device(87);
    let p = stats_with_decoder(1);
    h.feed_proto(op::V2IP_STATS, PROTOCOL_VERSION + 1, &p);

    // The receive path takes a frame's stamp and discards it. A ceiling here
    // would drop a newer device's report whole - the counters that predate the
    // block with it - and the symptom appears only once a device is upgraded,
    // by which time nothing points at the client. The asymmetry with transmit
    // is deliberate: a frame is stamped at its opcode's own version because the
    // device has a ceiling, which is not a reason to grow one here.
    let stats = h
        .device()
        .v2ip_stats
        .expect("a stamp above this client's version cost it the whole report");
    assert_eq!(stats.tx.video, u32::from_le_bytes([0xA5, 0xA4, 0xA7, 0xA6]));
    assert!(
        stats.decoder.reading().is_some(),
        "the counters survived the stamp and the block did not"
    );
}

/// The value this fixture writes at payload offset `off`.
///
/// Distinct per offset, so a field read four bytes out returns a wrong number
/// rather than a neighbour holding the same one. Above `0xFFFF`, so a four-byte
/// read narrowed to two returns a wrong number as well. Those are separate
/// failure modes over one fixture, and small distinct values pin only the
/// first: every counter here fits four bytes, so a narrowed read of a small one
/// returns the same answer.
fn at(off: u32) -> u32 {
    0x0037_0000 + off
}

#[test]
fn every_counter_is_read_at_its_own_offset() {
    let mut h = command_device(88);

    // All four counter blocks at once. Asserting a field against the same
    // field of another block cannot catch a shift - two reads off one wrong
    // offset agree with each other - so every counter is checked against the
    // absolute value its own offset carries.
    let mut p = poisoned(152);
    for off in (0..128).step_by(4) {
        assert!(
            at(off as u32) > u32::from(u16::MAX),
            "a counter that fits in two bytes cannot catch a narrowed read"
        );
        p[off..off + 4].copy_from_slice(&at(off as u32).to_le_bytes());
    }
    p[80] = V2ipDecoderState::HEALTHY.to_wire();
    p[124] = V2ipDecoderState::BAD.to_wire();
    p[128] = 0; // the decoder block is not what this test is about
    h.feed(op::V2IP_STATS, &p);
    let s = h.device().v2ip_stats.expect("no stats");

    assert_eq!(
        (
            s.tx.video,
            s.tx.audio,
            s.tx.anc,
            s.tx.stream_down,
            s.tx.overflow
        ),
        (at(0), at(4), at(8), at(12), at(16))
    );
    assert_eq!(
        (
            s.tx_per_minute.video,
            s.tx_per_minute.audio,
            s.tx_per_minute.anc,
            s.tx_per_minute.stream_down,
            s.tx_per_minute.overflow
        ),
        (at(20), at(24), at(28), at(32), at(36))
    );
    for (block, base, state) in [
        (s.rx, 40, V2ipDecoderState::HEALTHY),
        (s.rx_per_minute, 84, V2ipDecoderState::BAD),
    ] {
        assert_eq!(
            (
                block.video_total,
                block.video_dropped,
                block.video_seq_errors,
                block.wdt_timeout,
                block.audio_total
            ),
            (
                at(base),
                at(base + 4),
                at(base + 8),
                at(base + 12),
                at(base + 16)
            ),
            "receive block at {base}"
        );
        assert_eq!(
            (
                block.audio_dropped,
                block.audio_seq_errors,
                block.anc_total,
                block.anc_dropped,
                block.anc_seq_errors
            ),
            (
                at(base + 20),
                at(base + 24),
                at(base + 28),
                at(base + 32),
                at(base + 36)
            ),
            "receive block at {base}"
        );
        assert_eq!(block.decoder_state, state, "receive block at {base}");
    }
}

#[test]
fn half_a_geometry_is_not_a_geometry() {
    let mut h = command_device(89);

    // One dimension zero is the only shape that separates "both were
    // recovered" from "either was", and no reading a sink sends has it: a
    // decoder that recovered a width recovered a height. It is here because
    // the two readings differ nowhere else.
    for (width, height) in [(0u16, 2160u16), (3840, 0)] {
        let mut p = stats_with_decoder(1);
        p[132..134].copy_from_slice(&width.to_le_bytes());
        p[134..136].copy_from_slice(&height.to_le_bytes());
        h.feed(op::V2IP_STATS, &p);
        let d = h
            .device()
            .v2ip_stats
            .expect("no stats")
            .decoder
            .reading()
            .expect("no reading");
        assert!(
            !d.has_geometry(),
            "{width}x{height} is half a geometry and was read as a whole one"
        );
    }
}

#[test]
fn the_primary_cause_is_not_derivable_from_the_flags_word() {
    let mut h = command_device(90);

    // `reason` cannot be computed from `flags`: it is the sender's fixed
    // priority order, which the numbering does not express. In the first
    // reading bit 1 is set and bit 7 is the reason; in the second bit 1 is the
    // reason while bit 3 is also set. So it is neither the lowest set bit nor
    // the highest, and a caller asking "is this cause present" has to read the
    // word rather than compare the byte.
    //
    // The first two are a teardown reported from a device - the switch, then
    // the silence after it - relayed here rather than captured, so they are
    // evidence of what a sink sends and not a frame this crate has seen. The
    // third is composed from the priority order rather than observed at all.
    let readings = [
        (
            V2ipDecoderReason::SWITCH_PENDING,
            0b1000_1010u32,
            [
                V2ipDecoderReason::NO_PACKETS,
                V2ipDecoderReason::NO_FORMAT,
                V2ipDecoderReason::SWITCH_PENDING,
            ],
        ),
        (
            V2ipDecoderReason::NO_PACKETS,
            0b0000_1010u32,
            [
                V2ipDecoderReason::NO_PACKETS,
                V2ipDecoderReason::NO_FORMAT,
                V2ipDecoderReason::NO_PACKETS,
            ],
        ),
        // The sharper case, and a standing one rather than a moment: the
        // transmitter bridge sits below every input-side cause, so a pipeline
        // rebuilding in a loop always names an input cause and carries bit 9
        // here alone. A caller reading the byte never sees the loop at all.
        (
            V2ipDecoderReason::NO_PACKETS,
            0b0010_0000_0010u32,
            [
                V2ipDecoderReason::NO_PACKETS,
                V2ipDecoderReason::TX_BRIDGE_UNLOCKED,
                V2ipDecoderReason::TX_BRIDGE_UNLOCKED,
            ],
        ),
    ];

    // The set has to contradict both derivations, or a parser implementing one
    // of them passes on the readings that happen not to catch it. Losing the
    // first reading leaves the other two agreeing with a lowest-set-bit answer,
    // so this guards the fixtures rather than the parser: it fires when the set
    // degrades, not only when the decode does.
    let bit_of = |r: V2ipDecoderReason| u32::from(r.to_wire());
    assert!(
        readings
            .iter()
            .any(|(r, flags, _)| bit_of(*r) != flags.trailing_zeros()),
        "no reading here contradicts reading the primary cause as the lowest set bit"
    );
    assert!(
        readings
            .iter()
            .any(|(r, flags, _)| bit_of(*r) != u32::BITS - 1 - flags.leading_zeros()),
        "no reading here contradicts reading the primary cause as the highest set bit"
    );

    for (reason, flags, present) in readings {
        let mut p = stats_with_decoder(1);
        p[129] = reason.to_wire();
        p[140..144].copy_from_slice(&flags.to_le_bytes());
        h.feed(op::V2IP_STATS, &p);
        let d = h
            .device()
            .v2ip_stats
            .expect("no stats")
            .decoder
            .reading()
            .expect("no reading");
        assert_eq!(d.reason, reason);
        for cause in present {
            assert!(
                d.has_cause(cause),
                "{cause} is set in {flags:#b} and unread"
            );
        }
        assert!(
            !d.has_cause(V2ipDecoderReason::DECODER_BLOCKED),
            "a cause absent from {flags:#b} was reported as applying"
        );
        assert!(
            !d.has_cause(V2ipDecoderReason::OK),
            "bit 0 is cleared by the sender, so ok is never a cause"
        );
    }
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

    // Firmware without DeviceFeature::CONFIG_INITIALISED builds this frame from
    // an uninitialised stack local and ORs its scaling flags onto whatever was
    // there, so bits 2..6 arrive as noise. Only bit 7 carries meaning.
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

/// A real `V2IP_DEVICE_CFG`, captured off a live mesh.
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
    // Flags 0xdf carries bits 2, 3, 4 and 6 as well: this unit does not
    // initialise the configuration it broadcasts, so only bits 0, 1 and 7 mean
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
