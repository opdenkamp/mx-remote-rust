// Author: Lars Op den Kamp (lars@opdenkamp-it.nl)
// Copyright (c) 2026 Op den Kamp IT Solutions

//! Decodes that protocol 0x28 pinned down: the per-field merge rules of the
//! V2IP device configuration, the paged reports, and the signal report.

use std::net::Ipv4Addr;

use crate::types::{
    StreamKind, V2ipStreamSource, SCALING_FLAG_AUTO_SCALING, SCALING_FLAG_MODE_VALID,
    SCALING_FLAG_OPTIONS_VALID,
};
use crate::wire::{
    cstr, op, BayFeatures, BayStatus, DeviceFeature, MxrSignalType, BAY_CONFIG_SIZE, V2IP_DSCP_SET,
    V2IP_PORT_VIDEO,
};

use crate::testing::{bay_config_rec, field, hello_payload, poisoned, uid_n, Cfg};

use super::Harness;

fn dscp(value: u8) -> u8 {
    V2IP_DSCP_SET | value
}

fn v2ip_device(n: u8) -> Harness {
    let mut h = Harness::new(n);
    h.hello(0x28, "ONEIP-TX", "TX0001", DeviceFeature::V2IP_SOURCE);
    h
}

fn ip(s: &str) -> Ipv4Addr {
    s.parse().expect("test address")
}

#[test]
fn device_config_carries_dscp_and_rate() {
    let mut h = v2ip_device(20);
    let sender = h.sender;

    let mut full = Cfg::addresses(sender, "239.1.2.3");
    full.rate = 40;
    full.dscp_video = dscp(34);
    full.dscp_audio = dscp(46);
    full.dscp_anc = dscp(0);
    h.feed(op::V2IP_DEVICE_CFG, &full.bytes());

    let details = h.device().v2ip_details.expect("no details");
    assert_eq!(details.tx_rate, Some(40));
    assert!(details.dscp.is_complete());
    // DSCP 0 is a legal marking: the set bit, not the value, says present.
    assert_eq!(
        (details.dscp.video, details.dscp.audio, details.dscp.anc),
        (Some(34), Some(46), Some(0))
    );

    // An address-only write: zeroed rate and DSCP bytes, which must leave the
    // cached rate and marking alone rather than clearing them.
    h.feed(
        op::V2IP_DEVICE_CFG,
        &Cfg::addresses(sender, "239.9.9.9").bytes(),
    );

    let details = h.device().v2ip_details.expect("no details");
    assert_eq!(details.video.ip, ip("239.9.9.9"));
    assert_eq!(
        details.tx_rate,
        Some(40),
        "an address-only write cleared the rate"
    );
    assert!(details.dscp.is_complete());
    assert_eq!(details.dscp.audio, Some(46));
}

#[test]
fn a_rate_only_write_keeps_the_addresses() {
    let mut h = v2ip_device(21);
    let sender = h.sender;
    h.feed(
        op::V2IP_DEVICE_CFG,
        &Cfg::addresses(sender, "239.1.2.3").bytes(),
    );

    // A rate-only write zeroes every address block; the peer keeps the ones it
    // already had, so reporting 0.0.0.0 here would be wrong.
    let rate_only = Cfg {
        uid: sender,
        rate: 40,
        ..Cfg::default()
    };
    h.feed(op::V2IP_DEVICE_CFG, &rate_only.bytes());

    let details = h.device().v2ip_details.expect("no details");
    assert_eq!(details.video.ip, ip("239.1.2.3"));
    assert_eq!(details.anc.ip, ip("239.1.2.3"));
    assert_eq!(details.tx_rate, Some(40));
}

#[test]
fn stream_source_validity() {
    let cases = [
        (
            "multicast with port",
            ip("239.1.2.3"),
            V2IP_PORT_VIDEO,
            true,
        ),
        ("unicast", ip("10.8.8.9"), V2IP_PORT_VIDEO, false),
        ("multicast, port 0", ip("239.1.2.3"), 0, false),
        ("zero", Ipv4Addr::UNSPECIFIED, 0, false),
    ];
    for (name, addr, port, want) in cases {
        let source = V2ipStreamSource {
            kind: StreamKind::Video,
            ip: addr,
            port,
        };
        assert_eq!(source.is_valid(), want, "{name}");
    }
}

#[test]
fn device_config_rejects_unusable_addresses() {
    let mut h = v2ip_device(22);
    let sender = h.sender;
    h.feed(
        op::V2IP_DEVICE_CFG,
        &Cfg::addresses(sender, "239.1.2.3").bytes(),
    );

    // A unicast video address fails the firmware's stream check, so the whole
    // source block is dropped rather than half-applied.
    let mut unicast = Cfg::addresses(sender, "239.4.5.6");
    unicast.video_ip = "10.8.8.9";
    h.feed(op::V2IP_DEVICE_CFG, &unicast.bytes());
    assert_eq!(
        h.device().v2ip_details.expect("no details").anc.ip,
        ip("239.1.2.3")
    );

    // So does a multicast address with no port.
    let mut no_port = Cfg::addresses(sender, "239.4.5.6");
    no_port.anc_port = 0;
    h.feed(op::V2IP_DEVICE_CFG, &no_port.bytes());
    assert_eq!(
        h.device().v2ip_details.expect("no details").video.ip,
        ip("239.1.2.3")
    );
}

#[test]
fn scaling_merges_per_field() {
    let mut h = v2ip_device(23);
    let sender = h.sender;

    let mut both = Cfg::addresses(sender, "239.1.2.3");
    both.mode = 16;
    both.refresh = 60;
    both.flags = SCALING_FLAG_MODE_VALID | SCALING_FLAG_OPTIONS_VALID | SCALING_FLAG_AUTO_SCALING;
    h.feed(op::V2IP_DEVICE_CFG, &both.bytes());

    // An options-only write carries no mode, so the peer keeps its resolution.
    let options_only = Cfg {
        uid: sender,
        flags: SCALING_FLAG_OPTIONS_VALID,
        ..Cfg::default()
    };
    h.feed(op::V2IP_DEVICE_CFG, &options_only.bytes());

    let scaling = h.device().v2ip_details.expect("no details").scaling;
    assert_eq!(scaling.mode.svd(), 16);
    assert_eq!(scaling.refresh, 60);
    // The options branch replaces the whole options bit, so this does clear
    // auto-scaling.
    assert_eq!(scaling.flags & SCALING_FLAG_AUTO_SCALING, 0);

    // A mode-only write leaves the options alone.
    let mode_only = Cfg {
        uid: sender,
        mode: 31,
        refresh: 50,
        flags: SCALING_FLAG_MODE_VALID,
        ..Cfg::default()
    };
    h.feed(op::V2IP_DEVICE_CFG, &mode_only.bytes());

    let scaling = h.device().v2ip_details.expect("no details").scaling;
    assert_eq!(scaling.mode.svd(), 31);
    assert_eq!(scaling.refresh, 50);
    assert_ne!(scaling.flags & SCALING_FLAG_OPTIONS_VALID, 0);
}

#[test]
fn undefined_scaling_flag_bits_are_dropped() {
    let mut h = v2ip_device(28);
    let sender = h.sender;

    // Only three bits of this byte are defined. Firmware that does not
    // initialise the frame builds it over an uninitialised stack local, so the
    // rest is noise and must not reach the cache even on a first report.
    let mut noisy = Cfg::addresses(sender, "239.1.2.3");
    noisy.flags = 0xFF;
    h.feed(op::V2IP_DEVICE_CFG, &noisy.bytes());

    let flags = h.device().v2ip_details.expect("no details").scaling.flags;
    assert_eq!(
        flags & !(SCALING_FLAG_MODE_VALID | SCALING_FLAG_OPTIONS_VALID | SCALING_FLAG_AUTO_SCALING),
        0,
        "undefined flag bits reached the cache: {flags:#04x}"
    );
}

#[test]
fn a_partial_dscp_is_no_marking() {
    let mut h = v2ip_device(24);
    let sender = h.sender;

    // Firmware stores all three bytes behind the video byte's set bit, but
    // applies a marking only when all three carry one.
    let mut c = Cfg::addresses(sender, "239.1.2.3");
    c.dscp_video = dscp(34);
    c.dscp_audio = dscp(46);
    h.feed(op::V2IP_DEVICE_CFG, &c.bytes());

    let dscp = h.device().v2ip_details.expect("no details").dscp;
    assert!(!dscp.is_complete());
    assert_eq!(dscp.anc, None);
}

#[test]
fn bay_config_pages_merge() {
    let mut h = Harness::new(22);
    h.hello(0x28, "FF88", "PG0001", DeviceFeature::VIDEO_ROUTING);

    // A device splits its bays across frames sized against the payload it can
    // send; the second page must not displace the first.
    let mut page = bay_config_rec(
        1,
        0,
        0,
        "Input 1",
        "Apple TV",
        BayStatus::NONE,
        BayFeatures::HDMI_IN,
    );
    page.extend(bay_config_rec(
        2,
        0,
        1,
        "Input 2",
        "Blu-ray",
        BayStatus::NONE,
        BayFeatures::HDMI_IN,
    ));
    h.feed(op::SYS_BAY_CONFIG, &page);
    h.feed(
        op::SYS_BAY_CONFIG,
        &bay_config_rec(
            3,
            1,
            0,
            "Output 1",
            "TV",
            BayStatus::NONE,
            BayFeatures::HDMI_OUT,
        ),
    );

    for port in [1, 2, 3] {
        assert!(
            h.device().bay(port).is_some(),
            "bay on port {port} missing after a paged config"
        );
    }
}

#[test]
fn link_pages_merge() {
    let mut h = Harness::new(23);
    h.hello(0x28, "FF88", "PG0002", DeviceFeature::VIDEO_ROUTING);
    let mut cfg = bay_config_rec(
        1,
        0,
        0,
        "Input 1",
        "Apple TV",
        BayStatus::NONE,
        BayFeatures::HDMI_IN,
    );
    for (port, name) in [(2u8, "Output 1"), (3, "Output 2")] {
        cfg.extend(bay_config_rec(
            port,
            1,
            0,
            name,
            "TV",
            BayStatus::NONE,
            BayFeatures::HDMI_OUT,
        ));
    }
    h.feed(op::SYS_BAY_CONFIG, &cfg);

    let link_rec = |port: u8, serial: &str, bay: &str| {
        let mut rec = poisoned(38);
        rec[0] = port;
        field(&mut rec, 2, 16, serial);
        field(&mut rec, 18, 16, bay);
        rec
    };
    // The first page carries two records, so a stride read at anything but the
    // record width shifts the second one's fields; the second page is what
    // proves the first is not replaced.
    let mut page = link_rec(1, "AMP00001", "Zone 1");
    page.extend(link_rec(2, "AMP00001", "Zone 2"));
    h.feed(op::SYS_LINKS, &page);
    h.feed(op::SYS_LINKS, &link_rec(3, "AMP00001", "Zone 3"));

    let sender = h.sender;
    for (port, want) in [(1u16, "Zone 1"), (2, "Zone 2"), (3, "Zone 3")] {
        let key = h
            .state
            .link_key(crate::wire::BayUid::new(sender, port))
            .expect("no link key");
        let link = h
            .state
            .links
            .get(key)
            .expect("link lost after a second page");
        assert_eq!(link.linked_bay, want, "port {port}");
    }
}

#[test]
fn a_name_filling_its_field_stops_at_the_field_edge() {
    let mut h = Harness::new(24);

    // A 16-character name fills the field with no room for a terminator;
    // reading it must stop at the edge, not run into the serial that follows.
    let full = "0123456789ABCDEF";
    h.hello(0x28, full, "SN0001", DeviceFeature::VIDEO_ROUTING);

    assert_eq!(h.device().name(), full);
    assert_eq!(h.device().serial(), "SN0001");
}

#[test]
fn a_non_ascii_byte_costs_one_character() {
    assert_eq!(cstr(b"Zone \xff1"), "Zone \u{fffd}1");
    assert_eq!(
        cstr(&[0u8; 16]),
        "",
        "an all-NUL field should decode to the empty string"
    );
}

#[test]
fn signal_status_decodes_the_video_and_bay_blocks() {
    let mut h = Harness::new(25);
    h.hello(0x28, "ONEIP-RX", "SS0001", DeviceFeature::VIDEO_ROUTING);
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

    let mut p = poisoned(112);
    p[2] = 1 << 1; // support flags: the stream block is valid
    p[40] = 16; // video svd
    p[41] = 0; // colour space: RGB
    p[42] = 8; // colour depth
               // The frame rate is a u16 at offset 8 of the video block, so a rate above
               // 255Hz must not wrap into the low byte.
    p[48..50].copy_from_slice(&300u16.to_le_bytes());
    p[50..54].copy_from_slice(&594_000_000u32.to_le_bytes()); // TMDS clock
    p[100..102].copy_from_slice(&2u16.to_le_bytes()); // reporting port
    p[102..106].copy_from_slice(&BayStatus::SIGNAL_DETECTED.bits().to_le_bytes());
    // mxr_signal_type: svd 16, colour 1, bpp index 2 (ten bits)
    p[106] = 16;
    p[107] = 1 | (2 << 5);
    p[108..112].copy_from_slice(&148_500_000u32.to_le_bytes());
    h.feed(op::BAY_SIGNAL_STATUS, &p);

    let d = h.bay(2).signal_details.expect("no signal details");
    assert_eq!(d.frame_rate, 300.0);
    assert_eq!(d.tmds_clock, 594_000_000);
    assert_eq!(d.clock_rate, 148_500_000);
    assert!(d.status.has(BayStatus::SIGNAL_DETECTED));
    assert_eq!(d.scaling.svd(), 16);
    assert_eq!(d.scaling.colour_space(), 1);
    // The depth is an index, not a bit count: 2 stands for 10.
    assert_eq!(d.scaling.bpp(), Some(10));
    assert!(d.scaling.is_set());
}

/// The two payloads below came off a live mesh, from the input and the output
/// bay of one OneIP unit. Every other signal-report fixture here is one this
/// file made up, which can only show that the decoder agrees with its author;
/// these are checked against the published CTA-861 timings for the modes they
/// name, so agreement means the offsets are right rather than merely
/// self-consistent.
const CAPTURED_INPUT_BAY: [u8; 112] = [
    0x01, 0x00, 0xFF, 0x28, 0x00, 0x00, 0x00, 0x00, 0x02, 0x50, 0xA8, 0x00, 0x10, 0x00, 0x00, 0x34,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x01, 0x02, 0x80, 0xBB, 0x00, 0x00, 0x10, 0x01, 0x08, 0x02, 0x01, 0x00, 0x00, 0x00,
    0x3C, 0x00, 0x20, 0xEE, 0xD9, 0x08, 0x00, 0x00, 0x98, 0x08, 0x80, 0x07, 0x58, 0x00, 0x94, 0x00,
    0x2C, 0x00, 0x65, 0x04, 0x38, 0x04, 0x04, 0x00, 0x24, 0x00, 0x05, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x01, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x88, 0x01, 0x81, 0x00, 0x00, 0x00, 0x20, 0xEE, 0xD9, 0x08,
];

const CAPTURED_OUTPUT_BAY: [u8; 112] = [
    0x01, 0x00, 0xFF, 0x28, 0x00, 0x00, 0x00, 0x00, 0x02, 0x72, 0xA8, 0x00, 0x61, 0x00, 0x00, 0xA4,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x01, 0x02, 0x80, 0xBB, 0x00, 0x00, 0x61, 0x03, 0x08, 0x02, 0x01, 0x00, 0x00, 0x00,
    0x3C, 0x00, 0x80, 0xB8, 0x67, 0x23, 0x00, 0x00, 0x30, 0x11, 0x00, 0x0F, 0xB0, 0x00, 0x28, 0x01,
    0x58, 0x00, 0xCA, 0x08, 0x70, 0x08, 0x08, 0x00, 0x48, 0x00, 0x0A, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x01, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x10, 0x00, 0x98, 0x01, 0x81, 0x00, 0x00, 0xA0, 0x40, 0xDC, 0xB3, 0x11,
];

/// A unit with the two bays the captured reports name: an input on port 0 and
/// an output on port 16.
fn captured_unit() -> Harness {
    let mut h = Harness::new(27);
    h.hello(0x28, "ONEIP", "CAP0001", DeviceFeature::V2IP_SOURCE);
    h.feed(
        op::SYS_BAY_CONFIG,
        &bay_config_rec(
            0,
            0,
            0,
            "Input 1",
            "Apple TV",
            BayStatus::NONE,
            BayFeatures::HDMI_IN,
        ),
    );
    h.feed(
        op::SYS_BAY_CONFIG,
        &bay_config_rec(
            16,
            1,
            0,
            "Output 1",
            "TV",
            BayStatus::NONE,
            BayFeatures::HDMI_OUT,
        ),
    );
    h
}

#[test]
fn a_captured_input_report_decodes_to_the_mode_it_names() {
    let mut h = captured_unit();
    h.feed(op::BAY_SIGNAL_STATUS, &CAPTURED_INPUT_BAY);

    let d = h.bay(0).signal_details.expect("no signal details");
    // CTA-861 mode 16 is 1920x1080p60, whose pixel clock is 148.5MHz. The
    // clock is read from the video block and the mode from the bay block, so
    // the pair agreeing is what pins both.
    assert_eq!(d.tmds_clock, 148_500_000);
    // Sent as 60 with the non-integer-clock flag set, never as a fraction.
    assert_eq!(d.frame_rate, 59.94);
    assert_eq!(d.status, BayStatus::from_bits(0x0081_0188));
    assert!(d.status.has(BayStatus::SIGNAL_DETECTED));
    // Hot-plug detect is asserted by a display, so an input bay never reports
    // it. Read as an ordinal rather than a bitmask, 0x00810188 names nothing.
    assert!(!d.status.has(BayStatus::HPD_DETECTED));

    let audio = d.audio.expect("no audio block");
    assert_eq!(audio.format, 1); // L-PCM
    assert_eq!(audio.channels, 2);
    assert_eq!(audio.sample_rate, 48_000);
    assert_eq!(audio.coding, Some(0));

    // An input bay scales nothing, and says so with a plain zero.
    assert!(!d.scaling.is_set());
}

#[test]
fn a_captured_output_report_decodes_to_the_mode_it_names() {
    let mut h = captured_unit();
    h.feed(op::BAY_SIGNAL_STATUS, &CAPTURED_OUTPUT_BAY);

    let d = h.bay(16).signal_details.expect("no signal details");
    // Mode 97 is 3840x2160p60 at 594MHz.
    assert_eq!(d.tmds_clock, 594_000_000);
    assert_eq!(d.frame_rate, 59.94);
    assert_eq!(d.clock_rate, 297_000_000);
    assert_eq!(d.status, BayStatus::from_bits(0x0081_0198));
    // A display plugged in and being driven: the pair that separates "nothing
    // plugged in" from "no picture".
    assert!(d.status.has(BayStatus::HPD_DETECTED));
    assert!(d.status.has(BayStatus::SIGNAL_DETECTED));

    let audio = d.audio.expect("no audio block");
    assert_eq!(audio.format, 1);
    assert_eq!(audio.channels, 2);
    assert_eq!(audio.sample_rate, 48_000);

    // Nothing configured, said the other way: the word is zeroed and stamped
    // with the bpp index that names no depth.
    assert_eq!(d.scaling.bpp_index(), 5);
    assert!(!d.scaling.is_set());
    // The bay the report is filed under comes from the bay block, not from the
    // frame: the input bay this unit also has must not have moved.
    assert!(h.bay(0).signal_details.is_none());
}

/// A bay with nothing on it is described the way firmware describes one.
///
/// A device sends its own signal description in its bay configuration and this
/// library builds one from a signal report; both land in the same field, so a
/// second spelling would show a caller two states where there is one.
#[test]
fn a_bay_with_no_signal_is_described_as_firmware_describes_it() {
    let mut h = captured_unit();
    let mut p = CAPTURED_INPUT_BAY;
    p[2] &= !(1 << 1); // the stream block no longer holds a real signal
    h.feed(op::BAY_SIGNAL_STATUS, &p);

    assert_eq!(h.bay(0).signal_type.as_deref(), Some("no signal"));
    assert_eq!(h.bay(0).signal_detected, Some(false));
}

#[test]
fn a_report_without_the_audio_bit_carries_no_audio() {
    let mut h = captured_unit();
    let mut p = CAPTURED_INPUT_BAY;
    // Support flags with the audio block and the infoframe both unclaimed.
    p[2] &= !((1 << 4) | (1 << 5));
    h.feed(op::BAY_SIGNAL_STATUS, &p);

    let d = h.bay(0).signal_details.expect("no signal details");
    assert!(d.audio.is_none(), "the block was not claimed");
}

#[test]
fn a_source_sending_no_infoframe_claims_no_coding() {
    let mut h = captured_unit();
    let mut p = CAPTURED_INPUT_BAY;
    // The audio block stays claimed; only the infoframe bit goes.
    p[2] &= !(1 << 4);
    h.feed(op::BAY_SIGNAL_STATUS, &p);

    let audio = h
        .bay(0)
        .signal_details
        .expect("details")
        .audio
        .expect("audio");
    // The rest of the block is still read.
    assert_eq!(audio.sample_rate, 48_000);
    // Zero is a coding type a source can claim, so "did not say" is not it.
    assert_eq!(audio.coding, None);
}

#[test]
fn a_short_signal_report_is_dropped() {
    let mut h = Harness::new(26);
    h.hello(0x28, "ONEIP-RX", "SS0002", DeviceFeature::VIDEO_ROUTING);
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

    // The bay block naming the reporting bay is not there, so the report
    // cannot be attributed to a bay at all.
    h.feed(op::BAY_SIGNAL_STATUS, &[0u8; 68]);
    assert!(
        h.bay(2).signal_details.is_none(),
        "a short report was decoded"
    );
}

#[test]
fn the_unset_depth_sentinel_is_not_a_depth() {
    let unset = MxrSignalType::from_wire(5 << 13);
    assert!(!unset.is_set());
    assert_eq!(unset.to_string(), "unset");
    for (index, want) in [(1u16, 8u8), (2, 10), (3, 12), (4, 16)] {
        let signal = MxrSignalType::from_wire(index << 13);
        assert_eq!(signal.bpp(), Some(want), "depth index {index}");
    }
    // Index 0 is the firmware's "no depth", which is not a depth of zero.
    assert_eq!(MxrSignalType::from_wire(0).bpp(), None);
}

#[test]
fn every_amp_opcode_stamps_under_the_amp_cap() {
    // A ProAmp8 caps at 0x22 and drops any frame stamped above its own cap, so
    // the opcodes it handles must stay under it.
    for opcode in [
        op::SYS_HELLO,
        op::SYS_DISCOVER,
        op::RC_SETTINGS,
        op::V2IP_DEVICE_CFG,
        op::V2IP_MANUAL_SRC_SWITCH,
        op::AUDIO_SET_VOLUME,
        op::AMP_ZONE_SETTINGS,
        op::AMP_DOLBY_STATE,
    ] {
        let stamped = crate::wire::protocol_for(opcode);
        assert!(
            stamped <= 0x22,
            "opcode {:#04x} stamps {stamped:#04x}",
            opcode.0
        );
    }
    assert_eq!(crate::wire::protocol_for(op::V2IP_VIDEO_WALL), 0x28);
}

#[test]
fn a_per_record_loop_is_bounded_by_the_declared_length() {
    // A datagram carrying more than its header declares must not yield phantom
    // records to a per-record loop.
    let mut h = Harness::new(27);
    h.hello(0x28, "FF88", "PB0001", DeviceFeature::VIDEO_ROUTING);

    let mut data = super::datagram(
        h.sender,
        op::SYS_BAY_CONFIG,
        0x01,
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
    // A second record beyond what the header declares.
    data.extend(bay_config_rec(
        2,
        0,
        1,
        "Input 2",
        "Blu-ray",
        BayStatus::NONE,
        BayFeatures::HDMI_IN,
    ));
    let events = crate::rx::process_frame(&mut h.state, &data, None, std::time::Instant::now());
    h.events.extend(events);

    assert!(
        h.device().bay(1).is_some(),
        "the declared record should have registered"
    );
    assert!(
        h.device().bay(2).is_none(),
        "a record beyond the declared length became a bay"
    );
    assert_eq!(BAY_CONFIG_SIZE, 61);
}

/// A configuration a management client writes for another device belongs to
/// that device.
///
/// The payload names its subject in the first sixteen bytes, and every receiver
/// on the network resolves it from there: equal to the sender it is a device
/// describing itself, and different it is a write for the device it names. A
/// receiver that read the sender instead would file a controller's write to a
/// transceiver against the controller, leaving the transceiver's record as it
/// was and the controller's describing hardware it is not.
#[test]
fn a_managed_write_lands_on_the_device_it_names() {
    let mut h = v2ip_device(30);
    let sink = h.sender;
    let controller = uid_n(31);
    h.feed_as(
        controller,
        op::SYS_HELLO,
        &hello_payload(0x28, "Ctrl", "CTRL0001", "4.8.0", DeviceFeature::MANAGER),
    );

    let mut cfg = Cfg::addresses(sink, "239.1.2.3");
    cfg.rate = 40;
    h.feed_as(controller, op::V2IP_DEVICE_CFG, &cfg.bytes());

    let named = h
        .state
        .device(sink)
        .expect("the device the write names is not registered")
        .v2ip_details
        .expect("the write did not reach the device it names");
    assert_eq!(named.tx_rate, Some(40));
    assert_eq!(named.video.ip, ip("239.1.2.3"));

    assert!(
        h.state
            .device(controller)
            .expect("the controller is not registered")
            .v2ip_details
            .is_none(),
        "the write was filed against the sender rather than its subject"
    );
}

/// A device describing itself is still read as its own.
///
/// The subject and the sender are the same uid on every report a device sends
/// about itself, which is the case the network is mostly made of. Resolving by
/// subject has to leave it alone.
#[test]
fn a_device_describing_itself_is_read_as_its_own() {
    let mut h = v2ip_device(32);
    let sender = h.sender;

    let mut cfg = Cfg::addresses(sender, "239.4.5.6");
    cfg.rate = 30;
    h.feed(op::V2IP_DEVICE_CFG, &cfg.bytes());

    let details = h.device().v2ip_details.expect("no details");
    assert_eq!(details.tx_rate, Some(30));
    assert_eq!(details.video.ip, ip("239.4.5.6"));
}

/// A device that is not management cannot write another device's configuration.
///
/// A receiver takes a write for a third party only from a sender it treats as
/// management, so a record moved on any other sender's say-so is a record this
/// client holds and the network does not.
#[test]
fn a_write_for_another_device_from_an_ordinary_peer_is_dropped() {
    let mut h = v2ip_device(33);
    let sink = h.sender;
    let peer = uid_n(34);
    h.feed_as(
        peer,
        op::SYS_HELLO,
        &hello_payload(0x28, "OneIP", "TX0002", "4.8.0", DeviceFeature::V2IP_SOURCE),
    );

    let mut cfg = Cfg::addresses(sink, "239.7.8.9");
    cfg.rate = 40;
    h.feed_as(peer, op::V2IP_DEVICE_CFG, &cfg.bytes());

    assert!(
        h.state
            .device(sink)
            .expect("the named device is not registered")
            .v2ip_details
            .is_none(),
        "a peer with no management standing moved another device's record"
    );
    assert!(
        h.state
            .device(peer)
            .expect("the peer is not registered")
            .v2ip_details
            .is_none(),
        "the write was filed against the sender"
    );
}

/// A controller's write is trusted through the bit a controller actually sets.
///
/// The two kinds of writer announce themselves differently and neither implies
/// the other: an external application sets the manager bit, and a device
/// controlling its mesh sets the master bit instead. Testing only the first
/// would refuse every write that comes from a controller, which is the ordinary
/// case rather than an unusual one.
#[test]
fn a_mesh_controllers_write_lands_on_the_device_it_names() {
    let mut h = v2ip_device(35);
    let sink = h.sender;
    let controller = uid_n(36);
    h.feed_as(
        controller,
        op::SYS_HELLO,
        &hello_payload(
            0x28,
            "OneIP",
            "CTRL0002",
            "4.8.0",
            DeviceFeature::MESH_MASTER | DeviceFeature::VIDEO_ROUTING,
        ),
    );

    let mut cfg = Cfg::addresses(sink, "239.2.3.4");
    cfg.rate = 60;
    h.feed_as(controller, op::V2IP_DEVICE_CFG, &cfg.bytes());

    let named = h
        .state
        .device(sink)
        .expect("the named device is not registered")
        .v2ip_details
        .expect("a controller's write did not reach the device it names");
    assert_eq!(named.tx_rate, Some(60));
}

/// A controller a device names is trusted for that device even while it
/// announces neither bit.
///
/// A device sets the master bit only while it is both controlling its mesh and
/// has bays mapped, so one promoted before it has any announces nothing while
/// still being the controller its mesh obeys. What closes that window is the
/// controller uid the devices in the mesh report.
#[test]
fn a_controller_a_device_names_is_trusted_for_that_device() {
    let mut h = v2ip_device(37);
    let sink = h.sender;
    let controller = uid_n(38);
    h.feed_as(
        controller,
        op::SYS_HELLO,
        &hello_payload(
            0x28,
            "OneIP",
            "CTRL0003",
            "4.8.0",
            DeviceFeature::VIDEO_ROUTING,
        ),
    );

    // Before the sink says who it follows, a sender with neither bit is a
    // stranger and its write for the sink is dropped.
    let mut cfg = Cfg::addresses(sink, "239.3.4.5");
    cfg.rate = 60;
    h.feed_as(controller, op::V2IP_DEVICE_CFG, &cfg.bytes());
    assert!(
        h.state
            .device(sink)
            .expect("the named device is not registered")
            .v2ip_details
            .is_none(),
        "a sender nothing has vouched for moved another device's record"
    );

    // The sub-opcode at 0, the controller uid at 4.
    let mut mesh = poisoned(40);
    mesh[0] = 0xFF;
    mesh[4..20].copy_from_slice(controller.as_bytes());
    h.feed(op::MESH_OPERATION, &mesh);

    h.feed_as(controller, op::V2IP_DEVICE_CFG, &cfg.bytes());
    let named = h
        .state
        .device(sink)
        .expect("the named device is not registered")
        .v2ip_details
        .expect("the controller the device names was not trusted for it");
    assert_eq!(named.tx_rate, Some(60));
}
