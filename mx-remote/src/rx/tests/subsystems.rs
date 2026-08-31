// Author: Lars Op den Kamp (lars@opdenkamp-it.nl)
// Copyright (c) 2026 Op den Kamp IT Solutions

//! The subsystem parsers: topology, multiviewer, audio, amp, stats, network,
//! firmware and the detailed signal report.

use std::time::{Duration, Instant};

use crate::event::Event;
use crate::rx::lookup_svd;
use crate::types::{AudioFeatures, MacAddress, V2ipDecoderState};
use crate::wire::{
    op, BayFeatures, BayStatus, DeviceFeature, FirmwareType, MultiviewerPipSize, MultiviewerSource,
    MultiviewerViewMode, UtpLinkSpeed,
};

use crate::testing::{bay_config_rec, field, poisoned, uid_n};

use super::Harness;

#[test]
fn topology_parses() {
    let mut h = Harness::new(3);
    h.hello(0x27, "FF88", "TP0001", DeviceFeature::VIDEO_ROUTING);

    let other = uid_n(8);
    let mut rec = poisoned(20);
    rec[0..16].copy_from_slice(other.as_bytes());
    rec[16..20].copy_from_slice(&0xABCDu32.to_le_bytes());
    h.feed(op::TOPOLOGY, &rec);

    let topo = &h.device().topology;
    assert_eq!(topo.len(), 1);
    assert_eq!((topo[0].uid, topo[0].mask), (other, 0xABCD));
}

#[test]
fn multiviewer_status_parses() {
    let mut h = Harness::new(4);
    h.hello(
        0x27,
        "ONEIP-MV",
        "MV0001",
        DeviceFeature::V2IP_SINK | DeviceFeature::MULTIVIEWER,
    );

    // The 192 bytes a multiviewer sends: the 24-byte envelope and a settings
    // block of 168.
    let mut p = poisoned(192);
    // 0..16 target, 16 the STATUS sub-opcode, 17..24 padding.
    p[16] = 0;
    p[168] = 5; // the hardware layout: four windows
    p[169] = MultiviewerViewMode::PIP.to_wire();
    p[171] = MultiviewerPipSize::LARGE.to_wire();
    p[180] = 42; // audio volume
                 // The three index fields are written as the wire numbers them, from zero,
                 // rather than through this library's own enum: a fixture built from the
                 // enum would agree with a decoder that had the numbering wrong.
    p[182] = 0; // window 0 shows input 1
    p[183] = 3; // window 1 shows input 4, which one-based numbering cannot reach
    p[179] = 0; // audio source: input 1
    p[186] = 1; // remote control follows input 2
    p[24..40].copy_from_slice(uid_n(4).as_bytes());
    h.feed(op::V2IP_MULTIVIEWER, &p);

    assert!(h.device().is_multiviewer());
    let mv = h
        .device()
        .multiviewer
        .clone()
        .expect("no multiviewer status");
    assert_eq!(mv.view_mode, MultiviewerViewMode::PIP);
    assert_eq!(mv.pip_size, MultiviewerPipSize::LARGE);
    assert_eq!(mv.audio_volume, Some(42));
    assert_eq!(mv.video_sources[0], MultiviewerSource::INPUT_1);
    assert_eq!(mv.video_sources[1], MultiviewerSource::INPUT_4);
    assert_eq!(mv.audio_source, MultiviewerSource::INPUT_1);
    assert_eq!(mv.remote_control, MultiviewerSource::INPUT_2);
    assert_eq!(mv.window_count(), Some(4));
}

/// A status report too short to carry a settings block leaves the cached one
/// alone.
///
/// Every field is read with a fallback, so a short report decodes rather than
/// fails - and would replace a good status with one saying the device reported
/// nothing.
#[test]
fn a_truncated_multiviewer_status_does_not_replace_the_one_cached() {
    let mut h = Harness::new(4);
    h.hello(
        0x27,
        "ONEIP-MV",
        "MV0001",
        DeviceFeature::V2IP_SINK | DeviceFeature::MULTIVIEWER,
    );

    let mut whole = poisoned(192);
    whole[16] = 0;
    whole[169] = MultiviewerViewMode::PIP.to_wire();
    whole[24..40].copy_from_slice(uid_n(4).as_bytes());
    h.feed(op::V2IP_MULTIVIEWER, &whole);

    let mut short = poisoned(191);
    short[16] = 0;
    short[169] = MultiviewerViewMode::SINGLE.to_wire();
    short[24..40].copy_from_slice(uid_n(4).as_bytes());
    h.feed(op::V2IP_MULTIVIEWER, &short);

    let mv = h
        .device()
        .multiviewer
        .clone()
        .expect("no multiviewer status");
    assert_eq!(
        mv.view_mode,
        MultiviewerViewMode::PIP,
        "a short report was decoded over the cached one"
    );
}

#[test]
fn audio_features_parse() {
    let mut h = Harness::new(5);
    h.hello(
        0x27,
        "ONEIP",
        "AU0001",
        DeviceFeature::V2IP_SOURCE | DeviceFeature::V2IP_SINK,
    );

    let mut p = poisoned(68);
    p[0..2].copy_from_slice(&0u16.to_le_bytes()); // the FEATURES sub-opcode
    p[28..30].copy_from_slice(&2u16.to_le_bytes()); // endpoint count
                                                    // Entry 0 at 36: id 0, an endpoint declaration, input and V2IP transmit.
    p[36] = 0;
    p[37] = 1;
    p[44..48].copy_from_slice(
        &(AudioFeatures::INPUT.bits() | AudioFeatures::V2IP_TX.bits()).to_le_bytes(),
    );
    // Entry 1 at 52: id 1, an endpoint declaration, output.
    p[52] = 1;
    p[53] = 1;
    p[60..64].copy_from_slice(&AudioFeatures::OUTPUT.bits().to_le_bytes());
    h.feed(op::V2IP_AUDIO, &p);

    let eps = h.device().audio.clone().expect("no audio endpoints");
    assert_eq!(eps.list().count(), 2);
    let ep0 = eps.get(0).expect("endpoint 0 missing");
    assert!(ep0.features.has(AudioFeatures::INPUT) && ep0.features.is_v2ip());
    let ep1 = eps.get(1).expect("endpoint 1 missing");
    assert!(ep1.features.has(AudioFeatures::OUTPUT));
}

/// An amplifier tree with one output endpoint, numbered as amplifiers number
/// them: inputs below ten, outputs from ten, so id 10 is `Output` bay 0.
fn amp_output_tree() -> Vec<u8> {
    let mut p = poisoned(52);
    p[0..2].copy_from_slice(&0u16.to_le_bytes()); // the FEATURES sub-opcode
    p[28..30].copy_from_slice(&1u16.to_le_bytes()); // endpoint count
    p[36] = 10;
    p[37] = 1; // an endpoint declaration
    p[44..48].copy_from_slice(&AudioFeatures::OUTPUT.bits().to_le_bytes());
    p
}

fn amp_harness() -> Harness {
    let mut h = Harness::new(9);
    h.hello(
        0x27,
        "PROAMP8",
        "AM0002",
        DeviceFeature::VOLUME_CONTROL | DeviceFeature::AUDIO_ROUTING,
    );
    h
}

#[test]
fn an_audio_endpoint_reaches_a_bay_that_arrives_after_the_tree() {
    let mut h = amp_harness();
    h.feed(op::V2IP_AUDIO, &amp_output_tree());
    h.feed(
        op::SYS_BAY_CONFIG,
        &bay_config_rec(
            3,
            1,
            0,
            "Output 1",
            "Zone 1",
            BayStatus::NONE,
            BayFeatures::AUDIO_AMP_OUT,
        ),
    );

    assert_eq!(h.bay(3).audio_endpoint, Some(10));
}

#[test]
fn a_repeated_tree_reattaches_a_bay_that_was_reconfigured_under_it() {
    let mut h = amp_harness();
    // The bay is an input when the tree arrives, so no output endpoint names
    // it and it attaches to nothing.
    h.feed(
        op::SYS_BAY_CONFIG,
        &bay_config_rec(
            3,
            1,
            0,
            "Output 1",
            "Zone 1",
            BayStatus::NONE,
            BayFeatures::AUDIO_ANA_IN,
        ),
    );
    h.feed(op::V2IP_AUDIO, &amp_output_tree());
    assert_eq!(h.bay(3).audio_endpoint, None);

    // Rewiring the bay changes which endpoint names it without changing the
    // tree, and the device re-sends the same tree.
    h.feed(
        op::SYS_BAY_CONFIG,
        &bay_config_rec(
            3,
            1,
            0,
            "Output 1",
            "Zone 1",
            BayStatus::NONE,
            BayFeatures::AUDIO_AMP_OUT,
        ),
    );
    h.feed(op::V2IP_AUDIO, &amp_output_tree());

    assert_eq!(h.bay(3).audio_endpoint, Some(10));
}

#[test]
fn a_repeated_tree_is_not_announced_again() {
    let mut h = amp_harness();
    h.feed(op::V2IP_AUDIO, &amp_output_tree());
    h.events.clear();
    h.feed(op::V2IP_AUDIO, &amp_output_tree());

    assert!(!h.saw(|e| matches!(e, Event::AudioEndpointsChanged { .. })));
}

#[test]
fn amp_stats_and_network() {
    let mut h = Harness::new(7);
    // An amp routes audio and controls volume, and does not route video.
    h.hello(
        0x27,
        "PROAMP8",
        "AM0001",
        DeviceFeature::VOLUME_CONTROL | DeviceFeature::AUDIO_ROUTING,
    );
    h.feed(
        op::SYS_BAY_CONFIG,
        &bay_config_rec(
            3,
            1,
            0,
            "Output 1",
            "Zone 1",
            BayStatus::NONE,
            BayFeatures::AUDIO_AMP_OUT,
        ),
    );
    assert!(h.device().is_amp());
    let sender = h.sender;

    let mut az = poisoned(56); // the full struct, padding included
    az[0..16].copy_from_slice(sender.as_bytes());
    az[16..18].copy_from_slice(&3u16.to_le_bytes()); // zone: port 3
    az[18] = 10;
    az[19] = 11;
    az[40..44].copy_from_slice(&300u32.to_le_bytes());
    h.feed(op::AMP_ZONE_SETTINGS, &az);
    let s = h.bay(3).amp_settings.expect("no amp settings");
    assert_eq!((s.gain_left, s.gain_right, s.power_timeout), (10, 11, 300));
    assert!(h.saw(|e| matches!(e, Event::AmpZoneSettingsChanged { .. })));

    let mut dolby = poisoned(18);
    dolby[0..16].copy_from_slice(sender.as_bytes());
    dolby[16] = 2;
    dolby[17] = 0x1 | 0x2;
    h.feed(op::AMP_DOLBY_STATE, &dolby);
    let d = h.device().dolby_settings.expect("no dolby settings");
    assert_eq!(d.mode, 2);
    assert!(d.pcm_upmix && d.dolby_detected && !d.pcm_upmix_active);
    assert!(h.saw(|e| matches!(e, Event::AmpDolbySettingsChanged { .. })));

    let mut stats = poisoned(128);
    stats[0..4].copy_from_slice(&1000u32.to_le_bytes()); // transmit video
    stats[40..44].copy_from_slice(&5000u32.to_le_bytes()); // receive video total
    stats[80] = V2ipDecoderState::HEALTHY.to_wire(); // decoder state, 40 into the receive block
    h.feed(op::V2IP_STATS, &stats);
    let st = h.device().v2ip_stats.expect("no stats");
    assert_eq!((st.tx.video, st.rx.video_total), (1000, 5000));
    assert_eq!(st.rx.decoder_state, V2ipDecoderState::HEALTHY);

    let mut net = poisoned(88);
    net[0..2].copy_from_slice(&1u16.to_le_bytes()); // port 1
    net[2] = (1 << 0) | (1 << 6); // status support and uplink
                                  // The feature word is a u16 here, and its zero high byte beside the port's
                                  // is what tells the later 0x22 layout from the earlier one.
    net[3] = 0;
    field(&mut net, 4, 17, "eth0");
    net[21..27].copy_from_slice(&[0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF]);
    net[28..32].copy_from_slice(&[10, 8, 8, 1]);
    net[38] = 0x4 | (1 << 3); // 1Gbit/s, full duplex
    h.feed_proto(op::NET_LINK_STATUS, 0x22, &net);

    let port = h
        .device()
        .network
        .get(&1)
        .expect("no network port 1")
        .clone();
    assert_eq!(port.name, "eth0");
    assert_eq!(port.ip, Some(std::net::Ipv4Addr::new(10, 8, 8, 1)));
    assert!(port.link_full_duplex);
    assert_eq!(port.link_speed, UtpLinkSpeed::SPEED_1G);
    assert_eq!(
        port.mac_address,
        Some(MacAddress([0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF]))
    );
}

/// A device is online while it has been heard from recently, and any frame
/// counts - not only a hello.
#[test]
fn liveness_is_refreshed_by_any_frame() {
    let mut h = Harness::new(11);
    let now = Instant::now();

    // A V2IP device announces itself often enough that fifteen seconds of
    // silence is meaningful, so a hello received twenty seconds ago reads as
    // offline.
    let hello = super::hello_payload(0x27, "ONEIP", "LV0001", "4.7.9", DeviceFeature::V2IP_SINK);
    let twenty_ago = now - Duration::from_secs(20);
    h.feed_at(op::SYS_HELLO, &hello, twenty_ago);
    assert!(
        !h.device().is_online(now),
        "twenty seconds of silence should read as offline"
    );

    // Any later frame refreshes liveness, including one this library has no
    // handler for.
    h.feed_at(op::SYS_MONITORING_PULSE, &[], now);
    assert!(
        h.device().is_online(now),
        "a fresh frame should bring the device back online"
    );
    assert!(
        h.saw(|e| matches!(e, Event::DeviceOnlineChanged { online: true, .. })),
        "the online transition was not announced"
    );
}

#[test]
fn firmware_and_system_status() {
    let mut h = Harness::new(10);
    h.hello(0x27, "ONEIP", "FW0001", DeviceFeature::V2IP_SINK);

    let mut fw = poisoned(28);
    fw[0] = FirmwareType::FPGA.to_wire();
    fw[4..8].copy_from_slice(&0xCAFEu32.to_le_bytes());
    fw[8..12].copy_from_slice(&1_700_000_000u32.to_le_bytes());
    field(&mut fw, 12, 16, "1.2.3");
    h.feed(op::FIRMWARE_VERSION, &fw);

    let v = h
        .device()
        .firmware
        .get(&FirmwareType::FPGA)
        .expect("no FPGA version")
        .clone();
    assert_eq!(v.version, "1.2.3");
    assert_eq!((v.hash, v.timestamp), (0xCAFE, 1_700_000_000));

    let mut ss = poisoned(18);
    ss[16..18].copy_from_slice(&7u16.to_le_bytes());
    ss.extend_from_slice(b"overheating");
    h.feed(op::SYS_STATUS, &ss);
    assert_eq!(
        h.device().sys_status.clone(),
        Some((7, "overheating".to_owned()))
    );
}

#[test]
fn svd_lookup_and_signal_status() {
    let svd = lookup_svd(1).expect("svd 1 missing");
    assert_eq!((svd.horizontal_active, svd.vertical_active), (640, 480));

    let mut h = Harness::new(6);
    h.hello(0x27, "FF88", "SG0001", DeviceFeature::VIDEO_ROUTING);
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
    p[40] = 16; // svd 16 (1920x1080)
    p[41] = 0; // RGB
    p[42] = 8; // 8 bits per component
    p[48] = 60; // frame rate
    p[100..102].copy_from_slice(&2u16.to_le_bytes()); // port number
    h.feed(op::BAY_SIGNAL_STATUS, &p);

    let bay = h.bay(2);
    assert_eq!(bay.signal_detected, Some(true));
    let described = bay.signal_type.clone().expect("no signal description");
    assert_ne!(described, "no signal");
    assert!(
        described.starts_with("1920x1080 / RGB / 8bpp"),
        "description = {described}"
    );
}
