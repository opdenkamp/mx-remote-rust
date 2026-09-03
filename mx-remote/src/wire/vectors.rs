// Author: Lars Op den Kamp (lars@opdenkamp-it.nl)
// Copyright (c) 2026 Op den Kamp IT Solutions

//! Byte-exact transmit vectors.
//!
//! The expected bytes were generated from the reference Python library, so they
//! pin this port against something outside both implementations. A builder and
//! a decoder that are wrong together look correct to every round trip; only a
//! vector from elsewhere, or the firmware struct, catches that.
//!
//! Every field in a fixture carries a distinct value wherever the layout allows
//! it. A fixture hides a shift whenever the neighbouring bytes hold the same
//! value, so equal neighbours here would pass for an offset that is off by one.

use std::net::Ipv4Addr;

use crate::types::{AmpZoneSettings, V2ipAudioFormat, VolumeMuteStatus};

use super::bayconfig::{parse_bay_config, BAY_CONFIG_SIZE};
use super::enums::{
    BayFeatures, BayStatus, DeviceFeature, EdidProfile, MultiviewerViewMode, RcAction,
};
use super::frame::build_frame;
use super::opcode::{audio_sub, op, protocol_for};
use super::payload::*;
use super::uid::DeviceUid;

/// A UID whose every byte differs, so a misread offset lands on a wrong value.
const TEST_UID: DeviceUid =
    DeviceUid::from_array([0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15]);

fn hex_of(b: &[u8]) -> String {
    use core::fmt::Write as _;
    b.iter()
        .fold(String::with_capacity(b.len() * 2), |mut out, x| {
            let _ = write!(out, "{x:02x}");
            out
        })
}

/// Sub-opcodes of `V2IP_MULTIVIEWER`, from the multiviewer module.
const MV_OP_VIEW_MODE: u8 = 1;
const MV_OP_AUDIO_VOLUME: u8 = 4;

#[test]
fn uid_round_trips_through_its_dotted_hex_form() {
    assert_eq!(TEST_UID.to_string(), "03020100.07060504.0b0a0908.0f0e0d0c");
    assert_eq!(
        TEST_UID.to_string().parse::<DeviceUid>().unwrap(),
        TEST_UID,
        "dotted-hex form did not survive a round trip"
    );
}

#[test]
fn hello_frame() {
    // The version string and the announced protocol are fixed data rather than
    // the crate's own VERSION and PROTOCOL_VERSION: this vector pins the
    // payload layout, which a number that moves cannot do. That this client
    // announces PROTOCOL_VERSION is asserted where a peer decodes its hello,
    // in runtime::tests.
    let payload = build_hello(
        0x28,
        "TestApp",
        "P9SN00000000",
        "2.1.3",
        DeviceFeature::MANAGER.bits(),
    );
    let got = build_frame(
        TEST_UID,
        op::SYS_HELLO,
        protocol_for(op::SYS_HELLO),
        &payload,
    );
    assert_eq!(
        hex_of(&got),
        "50380100000102030405060708090a0b0c0d0e0f000036002800546573744170700000000000000000005039534e303030303030303000000000322e312e33000000000000000000000000000800"
    );
}

#[test]
fn discover_frame() {
    let got = build_frame(
        TEST_UID,
        op::SYS_DISCOVER,
        protocol_for(op::SYS_DISCOVER),
        &[],
    );
    assert_eq!(
        hex_of(&got),
        "50380100000102030405060708090a0b0c0d0e0f01000000"
    );
}

#[test]
fn set_bay_name_frame() {
    // Six bytes longer than the reference Python library sends, which stops at
    // the last name byte. `mxr_bay_name_data` is ALIGN(8) and the addressed
    // device measures the payload against the whole struct, so the shorter form
    // is dropped on the length check. The bytes up to the padding are the
    // reference's.
    let payload = build_set_bay_name(TEST_UID, 5, "Living Room");
    let got = build_frame(
        TEST_UID,
        op::CHANGE_BAY_NAME,
        protocol_for(op::CHANGE_BAY_NAME),
        &payload,
    );
    assert_eq!(
        hex_of(&got),
        "50380600000102030405060708090a0b0c0d0e0f22002800000102030405060708090a0b0c0d0e0f05004c6976696e6720526f6f6d0000000000000000000000"
    );
}

#[test]
fn set_volume_frame() {
    let vol = VolumeMuteStatus {
        volume_left: Some(40),
        volume_right: Some(40),
        muted_left: Some(false),
        muted_right: Some(false),
    };
    assert_eq!(hex_of(&vol.wire()), "282800");

    let payload = build_set_volume(TEST_UID, 5, vol);
    let got = build_frame(
        TEST_UID,
        op::AUDIO_SET_VOLUME,
        protocol_for(op::AUDIO_SET_VOLUME),
        &payload,
    );
    assert_eq!(
        hex_of(&got),
        "50381100000102030405060708090a0b0c0d0e0f14001800000102030405060708090a0b0c0d0e0f0500282800000000"
    );
}

#[test]
fn manual_source_switch_frame() {
    let format = V2ipAudioFormat {
        sample_rate: 48000,
        channels: 2,
    };
    let payload = build_v2ip_manual_source_switch(
        TEST_UID,
        V2ipStreams {
            video: StreamAddr {
                ip: Ipv4Addr::new(239, 1, 2, 3),
                port: 50020,
            },
            audio: StreamAddr {
                ip: Ipv4Addr::new(239, 1, 2, 4),
                port: 50022,
            },
            anc: StreamAddr::default(),
        },
        Some(format),
    );
    let got = build_frame(
        TEST_UID,
        op::V2IP_MANUAL_SRC_SWITCH,
        protocol_for(op::V2IP_MANUAL_SRC_SWITCH),
        &payload,
    );
    assert_eq!(
        hex_of(&got),
        "50380700000102030405060708090a0b0c0d0e0f24003000000102030405060708090a0b0c0d0e0fef01020364c30000ef01020466c30000000000000000000080bb000002000000"
    );
}

#[test]
fn edid_profile_frame() {
    let payload = build_edid_profile(TEST_UID, EdidProfile::HDR_SURROUND71_4K);
    let got = build_frame(
        TEST_UID,
        op::BAY_EDID_PROFILE,
        protocol_for(op::BAY_EDID_PROFILE),
        &payload,
    );
    assert_eq!(
        hex_of(&got),
        "50380800000102030405060708090a0b0c0d0e0f34001800000102030405060708090a0b0c0d0e0f0800000000000000"
    );
}

#[test]
fn reboot_frame() {
    let payload = build_target_only(TEST_UID);
    let got = build_frame(
        TEST_UID,
        op::SYS_REBOOT,
        protocol_for(op::SYS_REBOOT),
        &payload,
    );
    assert_eq!(
        hex_of(&got),
        "50380100000102030405060708090a0b0c0d0e0f28001000000102030405060708090a0b0c0d0e0f"
    );
}

#[test]
fn rc_action_frame() {
    let payload = build_rc_action(TEST_UID, 5, RcAction::POWER_ON);
    let got = build_frame(
        TEST_UID,
        op::RC_TX_ACTION,
        protocol_for(op::RC_TX_ACTION),
        &payload,
    );
    assert_eq!(
        hex_of(&got),
        "50380c00000102030405060708090a0b0c0d0e0f0e001400000102030405060708090a0b0c0d0e0f05000100"
    );
}

#[test]
fn multiviewer_frames() {
    // The one vector here whose stamped version differs from the reference
    // Python library, which sends this opcode at 0x20. The receiving module
    // dispatches this frame on its payload length and never reads the stamp,
    // so 0x20 enables nothing and is refused by every receiver capped between
    // this opcode's own 0x16 and 0x1F. The payload bytes still come from the
    // reference; only the header version is ours.
    let view = build_frame(
        TEST_UID,
        op::V2IP_MULTIVIEWER,
        protocol_for(op::V2IP_MULTIVIEWER),
        &mv_cmd_payload(
            TEST_UID,
            MV_OP_VIEW_MODE,
            &[MultiviewerViewMode::PIP.to_wire()],
        ),
    );
    assert_eq!(
        hex_of(&view),
        "50381600000102030405060708090a0b0c0d0e0f42001900000102030405060708090a0b0c0d0e0f010000000000000002"
    );

    let volume = build_frame(
        TEST_UID,
        op::V2IP_MULTIVIEWER,
        protocol_for(op::V2IP_MULTIVIEWER),
        &mv_cmd_payload(TEST_UID, MV_OP_AUDIO_VOLUME, &[42, 1]),
    );
    assert_eq!(
        hex_of(&volume),
        "50381600000102030405060708090a0b0c0d0e0f42001a00000102030405060708090a0b0c0d0e0f04000000000000002a01"
    );
}

#[test]
fn audio_command_frames() {
    let mut mute = audio_cmd_header(audio_sub::MUTE, TEST_UID);
    mute.extend_from_slice(&audio_param(3, 1));
    let got = build_frame(
        TEST_UID,
        op::V2IP_AUDIO,
        protocol_for(op::V2IP_AUDIO),
        &mute,
    );
    assert_eq!(
        hex_of(&got),
        "50381a00000102030405060708090a0b0c0d0e0f43001c0001000000000102030405060708090a0b0c0d0e0f0300000001000000"
    );

    let mut volume = audio_cmd_header(audio_sub::VOLUME, TEST_UID);
    volume.extend_from_slice(&audio_param(5, 80));
    let got = build_frame(
        TEST_UID,
        op::V2IP_AUDIO,
        protocol_for(op::V2IP_AUDIO),
        &volume,
    );
    assert_eq!(
        hex_of(&got),
        "50381a00000102030405060708090a0b0c0d0e0f43001c0004000000000102030405060708090a0b0c0d0e0f0500000050000000"
    );
}

/// The expected bytes here are laid out from the C declaration field by field,
/// not produced by `build_amp_zone_settings` - a vector taken from the builder
/// only proves the builder agrees with itself, which is how the delays sat at
/// the wrong offset in two libraries at once.
#[test]
fn amp_zone_settings_frame() {
    let settings = AmpZoneSettings {
        gain_left: 10,
        gain_right: 11,
        volume_min: 0,
        volume_max: 100,
        delay_left: 5,
        delay_right: 6,
        bass: 3,
        treble: 4,
        bridged: 1,
        power_mode: 2,
        power_level: 7,
        power_timeout: 300,
        eq_left: [1, 2, 3, 4, 5],
        eq_right: [6, 7, 8, 9, 10],
    };
    let payload = build_amp_zone_settings(TEST_UID, 5, &settings);
    let got = build_frame(
        TEST_UID,
        op::AMP_ZONE_SETTINGS,
        protocol_for(op::AMP_ZONE_SETTINGS),
        &payload,
    );
    assert_eq!(
        hex_of(&got),
        "50381c00000102030405060708090a0b0c0d0e0f3d003800000102030405060708090a0b0c0d0e0f05000a0b00640000050000000600000003040102070000002c0100000102030405060708090a0000"
    );
}

#[test]
fn stats_request_frame() {
    let payload = build_stats_request(TEST_UID, true);
    let got = build_frame(
        TEST_UID,
        op::V2IP_STATS,
        protocol_for(op::V2IP_STATS),
        &payload,
    );
    assert_eq!(
        hex_of(&got),
        "50381300000102030405060708090a0b0c0d0e0f3f001100000102030405060708090a0b0c0d0e0f01"
    );
}

#[test]
fn bay_config_record_fields() {
    let mut p = vec![0u8; BAY_CONFIG_SIZE];
    p[0] = 7; // port
    p[1] = 1; // output
    p[2] = 2; // bay
    p[3] = 3; // video source
    p[4] = 4; // audio source
    p[5..13].copy_from_slice(b"Output 3");
    p[21..28].copy_from_slice(b"Kitchen");
    p[37..44].copy_from_slice(b"1080p60");
    p[53] = 1 << 3; // status: signal detected
    p[57] = 1 << 0; // features: HDMI out

    let cfg = parse_bay_config(&p).expect("a full-width record must parse");
    assert_eq!((cfg.port, cfg.modenum, cfg.bay), (7, 1, 2));
    assert_eq!(cfg.bay_name, "Output 3");
    assert_eq!(cfg.user_name, "Kitchen");
    assert_eq!(cfg.signal_type, "1080p60");
    assert!(cfg.status.has(BayStatus::SIGNAL_DETECTED));
    assert!(cfg.features.has(BayFeatures::HDMI_OUT));
}

#[test]
fn a_record_shorter_than_the_declared_width_is_dropped() {
    assert!(parse_bay_config(&[0u8; BAY_CONFIG_SIZE - 1]).is_none());
}
