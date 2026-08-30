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

use crate::testing::{bay_config_rec, field, poisoned, Cfg};

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
