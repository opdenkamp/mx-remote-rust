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
    MultiviewerViewMode, UtpLinkSpeed, PROTOCOL_VERSION,
};

use crate::testing::{bay_config_rec, field, poisoned, uid_n};

use super::Harness;

#[test]
fn topology_parses() {
    let mut h = Harness::new(3);
    h.hello(0x27, "FF88", "TP0001", DeviceFeature::VIDEO_ROUTING);

    // Two entries, so the second shows where the first one ends.
    let (one, two) = (uid_n(8), uid_n(9));
    let mut p = poisoned(48);
    p[0..16].copy_from_slice(one.as_bytes());
    p[16..20].copy_from_slice(&0xABCDu32.to_le_bytes());
    p[24..40].copy_from_slice(two.as_bytes());
    p[40..44].copy_from_slice(&0x0102u32.to_le_bytes());
    h.feed(op::TOPOLOGY, &p);

    let topo = &h.device().topology;
    assert_eq!(topo.len(), 2);
    assert_eq!((topo[0].uid, topo[0].mask), (one, 0xABCD));
    assert_eq!((topo[1].uid, topo[1].mask), (two, 0x0102));
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
    p[176] = 2; // the small screen arrangement
                // A status names the unit it describes, and a unit reports only on
                // itself.
    p[24..40].copy_from_slice(uid_n(5).as_bytes());
    h.feed(op::V2IP_MULTIVIEWER, &p);
    assert!(
        h.device().multiviewer.is_none(),
        "a status about another unit"
    );
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
    assert_eq!(mv.screen_mode, 2);
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
    h.feed_audio_report(&p);

    let eps = h.device().audio.clone().expect("no audio endpoints");
    assert_eq!(eps.list().count(), 2);
    let ep0 = eps.get(0).expect("endpoint 0 missing");
    assert!(ep0.features.has(AudioFeatures::INPUT) && ep0.features.is_v2ip());
    let ep1 = eps.get(1).expect("endpoint 1 missing");
    assert!(ep1.features.has(AudioFeatures::OUTPUT));
}

/// A unit reports only its own audio: a report naming another unit, or too
/// short for what it counts, changes nothing.
#[test]
fn an_audio_report_is_read_only_whole_and_about_its_sender() {
    let mut h = Harness::new(7);
    h.hello(0x2F, "ONEIP", "AU0003", DeviceFeature::V2IP_SINK);
    let tree = lockable_tree(0, 0);

    let mut other = tree.clone();
    other[4..20].copy_from_slice(uid_n(8).as_bytes());
    h.feed(op::V2IP_AUDIO, &other);
    assert!(h.device().audio.is_none(), "a report about another unit");

    h.feed_audio_report(&tree[..67]);
    assert!(h.device().audio.is_none(), "a report short of its entries");

    // Links behind the entries: a count, two reserved bytes, then 20 bytes
    // per link.
    let linked = uid_n(9);
    let link = |endpoint: u8| {
        let mut l = vec![endpoint, 4, 0, 0];
        l.extend_from_slice(linked.as_bytes());
        l
    };
    let mut with_links = tree.clone();
    with_links.extend_from_slice(&[1, 0, 0, 0]);
    with_links.extend(link(0));
    h.feed_audio_report(&with_links[..with_links.len() - 1]);
    assert!(h.device().audio.is_none(), "a report short of its links");
    h.feed_audio_report(&with_links);
    let eps = h.device().audio.clone().expect("no audio endpoints");
    assert_eq!(eps.get(0).map(|e| e.linked_device), Some(linked));

    // A links report alone carries its block behind the command header.
    let mut links = poisoned(20);
    links[0..2].copy_from_slice(&5u16.to_le_bytes()); // the LINKS sub-opcode
    links.extend_from_slice(&[1, 0, 0, 0]);
    links.extend(link(1));
    h.feed_audio_report(&links);
    let eps = h.device().audio.clone().expect("no audio endpoints");
    assert_eq!(eps.get(1).map(|e| e.linked_device), Some(linked));
}

/// A V2IP receiver's tree: endpoint 0 an output, endpoint 1 its V2IP input,
/// which can lock its source. Each entry's status word follows its features,
/// and every other byte is poisoned.
fn lockable_tree(status0: u32, status1: u32) -> Vec<u8> {
    let mut p = poisoned(68);
    p[0..2].copy_from_slice(&0u16.to_le_bytes()); // the FEATURES sub-opcode
    p[28..30].copy_from_slice(&2u16.to_le_bytes()); // endpoint count
    p[36] = 0;
    p[37] = 1;
    p[44..48].copy_from_slice(&AudioFeatures::OUTPUT.bits().to_le_bytes());
    p[48..52].copy_from_slice(&status0.to_le_bytes());
    p[52] = 1;
    p[53] = 1;
    let input = AudioFeatures::INPUT.bits()
        | AudioFeatures::V2IP_RX.bits()
        | AudioFeatures::AUDIO_LOCK.bits();
    p[60..64].copy_from_slice(&input.to_le_bytes());
    p[64..68].copy_from_slice(&status1.to_le_bytes());
    p
}

/// Each endpoint's status word is read behind its features, and a report
/// whose only change is a status is still a change.
#[test]
fn an_audio_endpoint_status_is_read_and_its_change_reported() {
    let mut h = Harness::new(6);
    h.hello(0x2B, "ONEIP", "AU0002", DeviceFeature::V2IP_SINK);
    // The module's MXR_AUDIO_FEATURE_AUDIO_LOCK, written out so a wrong
    // constant here cannot agree with itself.
    let lock = 1u32 << 15;
    let muted = AudioFeatures::MUTE.bits();
    h.feed_audio_report(&lockable_tree(muted, 0));

    let eps = h.device().audio.clone().expect("no audio endpoints");
    assert_eq!(eps.status(0), Some(AudioFeatures::MUTE));
    assert_eq!(eps.status(1), Some(AudioFeatures::from_bits(0)));
    assert_eq!(eps.status(2), None, "an endpoint the device did not report");

    let changes = |h: &Harness| {
        h.events
            .iter()
            .filter(|e| matches!(e, Event::AudioEndpointsChanged { .. }))
            .count()
    };
    assert_eq!(changes(&h), 1);
    h.feed_audio_report(&lockable_tree(muted, 0));
    assert_eq!(changes(&h), 1, "an unchanged report raised an event");
    h.feed_audio_report(&lockable_tree(muted, lock));
    assert_eq!(changes(&h), 2, "locking the source raised no event");
    let eps = h.device().audio.clone().expect("no audio endpoints");
    assert_eq!(eps.status(1), Some(AudioFeatures::AUDIO_LOCK));
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
    h.feed_audio_report(&amp_output_tree());
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
            0,
            0,
            "Output 1",
            "Zone 1",
            BayStatus::NONE,
            BayFeatures::AUDIO_ANA_IN,
        ),
    );
    h.feed_audio_report(&amp_output_tree());
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
    h.feed_audio_report(&amp_output_tree());

    assert_eq!(h.bay(3).audio_endpoint, Some(10));
}

#[test]
fn a_repeated_tree_is_not_announced_again() {
    let mut h = amp_harness();
    h.feed_audio_report(&amp_output_tree());
    h.events.clear();
    h.feed_audio_report(&amp_output_tree());

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

    // 24 rather than 18: the struct is 8-aligned and the amp measures all of it.
    let mut dolby = poisoned(24);
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

/// A device on 0x2A may announce itself only every 20 to 30 seconds, so it is
/// given a minute of silence rather than fifteen seconds.
#[test]
fn a_slow_hello_device_is_given_a_minute() {
    let now = Instant::now();
    let hello = |protocol| {
        super::hello_payload(
            protocol,
            "ONEIP",
            "LV0002",
            "4.7.9",
            DeviceFeature::V2IP_SINK,
        )
    };

    let mut h = Harness::new(12);
    h.feed_at(op::SYS_HELLO, &hello(0x2A), now - Duration::from_secs(45));
    assert!(
        h.device().is_online(now),
        "a 0x2A device went offline inside its minute"
    );

    let mut h = Harness::new(13);
    h.feed_at(op::SYS_HELLO, &hello(0x2A), now - Duration::from_secs(61));
    assert!(
        !h.device().is_online(now),
        "a 0x2A device outlived its minute"
    );

    let mut h = Harness::new(14);
    h.feed_at(op::SYS_HELLO, &hello(0x29), now - Duration::from_secs(20));
    assert!(
        !h.device().is_online(now),
        "a device below 0x2A was given the longer window"
    );

    // This client announces its own version, and a peer that sees every device
    // on 0x2A slows its hello to match - so a peer on this client's version has
    // to outlast the slowest of those intervals.
    let mut h = Harness::new(15);
    h.feed_at(
        op::SYS_HELLO,
        &hello(PROTOCOL_VERSION),
        now - Duration::from_secs(30),
    );
    assert!(
        h.device().is_online(now),
        "a peer on this client's version went offline between two of its hellos"
    );

    // Below 0x20 a unit allows three minutes.
    let mut h = Harness::new(16);
    h.feed_at(op::SYS_HELLO, &hello(0x1F), now - Duration::from_secs(170));
    assert!(
        h.device().is_online(now),
        "a device below 0x20 went offline inside its three minutes"
    );
    let mut h = Harness::new(17);
    h.feed_at(op::SYS_HELLO, &hello(0x1F), now - Duration::from_secs(181));
    assert!(
        !h.device().is_online(now),
        "a device below 0x20 outlived its three minutes"
    );
}

/// A device is online while it has been heard from recently, and any frame
/// keeps it so - not only a hello. Once it has been silent long enough to
/// count as offline, only a hello brings it back, as a unit has it.
#[test]
fn liveness_is_refreshed_by_any_frame_until_it_lapses() {
    let mut h = Harness::new(11);
    let now = Instant::now();

    // A V2IP device announces itself often enough that fifteen seconds of
    // silence is meaningful.
    let hello = super::hello_payload(0x27, "ONEIP", "LV0001", "4.7.9", DeviceFeature::V2IP_SINK);
    h.feed_at(op::SYS_HELLO, &hello, now - Duration::from_secs(20));
    h.feed_at(op::SYS_MONITORING_PULSE, &[], now - Duration::from_secs(10));
    assert!(
        h.device().is_online(now),
        "a frame inside the window, of any kind, kept the device alive"
    );

    h.feed_at(op::SYS_MONITORING_PULSE, &[], now + Duration::from_secs(10));
    assert!(
        !h.device().is_online(now + Duration::from_secs(10)),
        "a frame after the window lapsed brought the device back"
    );
    // The probe loop is what notices a device has gone.
    let sender = h.sender;
    let device = h.state.device_mut(sender).expect("registered");
    device.check_online(now + Duration::from_secs(10), &mut Vec::new());

    h.events.clear();
    h.feed_at(op::SYS_HELLO, &hello, now + Duration::from_secs(10));
    assert!(h.device().is_online(now + Duration::from_secs(10)));
    assert!(
        h.saw(|e| matches!(e, Event::DeviceOnlineChanged { online: true, .. })),
        "the hello brought it back without saying so"
    );
}

/// A OneIP's FPGA report carries the RTOS and loader hashes, then the unit's
/// own firmware and bootloader, behind its 144 bytes.
#[test]
fn a_firmware_report_carries_the_other_images_behind_it() {
    let mut h = Harness::new(10);
    h.hello(0x2F, "ONEIP", "FW0002", DeviceFeature::V2IP_SINK);

    let mut fw = poisoned(200);
    fw[0] = FirmwareType::FPGA.to_wire();
    fw[4..8].copy_from_slice(&0x1111u32.to_le_bytes());
    fw[8..12].copy_from_slice(&1_700_000_000u32.to_le_bytes());
    field(&mut fw, 12, 128, "oneip-fpga");
    fw[144..148].copy_from_slice(&0x2222u32.to_le_bytes());
    fw[148..152].copy_from_slice(&0x3333u32.to_le_bytes());
    fw[152..156].copy_from_slice(&0x4444u32.to_le_bytes());
    fw[156..160].copy_from_slice(&1_700_000_001u32.to_le_bytes());
    fw[160..164].copy_from_slice(&0x5555u32.to_le_bytes());
    fw[164..168].copy_from_slice(&1_700_000_002u32.to_le_bytes());
    field(&mut fw, 168, 16, "4.9.1.123");
    field(&mut fw, 184, 16, "2.0.4");
    h.feed(op::FIRMWARE_VERSION, &fw);

    let got = |t| {
        let v = h.device().firmware.get(&t).cloned().expect("missing");
        (v.hash, v.timestamp, v.version)
    };
    let fpga = |hash| (hash, 1_700_000_000, "oneip-fpga".to_owned());
    assert_eq!(got(FirmwareType::FPGA), fpga(0x1111));
    assert_eq!(got(FirmwareType::RTOS), fpga(0x2222));
    assert_eq!(got(FirmwareType::LOADER), fpga(0x3333));
    assert_eq!(
        got(FirmwareType::MCU),
        (0x4444, 1_700_000_001, "4.9.1.123".to_owned())
    );
    assert_eq!(
        got(FirmwareType::BOOTLOADER),
        (0x5555, 1_700_000_002, "2.0.4".to_owned())
    );

    // Without the second block the first is still read, and nothing is made
    // up for the unit's own firmware.
    let mut h = Harness::new(11);
    h.hello(0x2F, "ONEIP", "FW0003", DeviceFeature::V2IP_SINK);
    h.feed(op::FIRMWARE_VERSION, &fw[..199]);
    assert!(h.device().firmware.contains_key(&FirmwareType::LOADER));
    assert!(!h.device().firmware.contains_key(&FirmwareType::MCU));
    let mut h = Harness::new(12);
    h.hello(0x2F, "ONEIP", "FW0004", DeviceFeature::V2IP_SINK);
    h.feed(op::FIRMWARE_VERSION, &fw[..151]);
    assert!(!h.device().firmware.contains_key(&FirmwareType::RTOS));

    // Only an FPGA report carries the blocks.
    let mut h = Harness::new(13);
    h.hello(0x2F, "ONEIP", "FW0005", DeviceFeature::V2IP_SINK);
    fw[0] = FirmwareType::MCU.to_wire();
    h.feed(op::FIRMWARE_VERSION, &fw);
    assert_eq!(h.device().firmware.len(), 1);
    assert_eq!(h.device().firmware[&FirmwareType::MCU].hash, 0x1111);
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

    // A unit reports only its own status, naming itself first.
    let mut ss = poisoned(18);
    ss[0..16].copy_from_slice(uid_n(11).as_bytes());
    ss[16..18].copy_from_slice(&7u16.to_le_bytes());
    ss.extend_from_slice(b"overheating");
    h.feed(op::SYS_STATUS, &ss);
    assert_eq!(h.device().sys_status, None, "a status naming another unit");
    ss[0..16].copy_from_slice(h.sender.as_bytes());
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
    p[2] = 0b11; // support flags: a stream is detected and valid
    p[3] = 0; // stream flags: none
    p[40] = 16; // svd 16 (1920x1080)
    p[41] = 0; // RGB
    p[42] = 8; // 8 bits per component
    p[48..50].copy_from_slice(&60u16.to_le_bytes()); // frame rate
    p[100..102].copy_from_slice(&2u16.to_le_bytes()); // port number
    let status = BayStatus::SIGNAL_DETECTED | BayStatus::HPD_DETECTED;
    p[102..106].copy_from_slice(&status.bits().to_le_bytes());
    h.feed(op::BAY_SIGNAL_STATUS, &p);

    // The bay status word is applied as a bay configuration's is.
    let bay = h.bay(2);
    assert_eq!(bay.signal_detected, Some(true));
    assert_eq!(bay.hpd_detected, Some(true));
    let described = bay.signal_type.clone().expect("no signal description");
    assert_eq!(described, "1920x1080 / RGB / 8bpp / 60Hz");

    // A mode the CTA table does not have is described by its timing.
    p[40] = 0;
    p[58..60].copy_from_slice(&1440u16.to_le_bytes()); // active width
    p[68..70].copy_from_slice(&900u16.to_le_bytes()); // active height
    h.feed(op::BAY_SIGNAL_STATUS, &p);
    let described = h.bay(2).signal_type.clone().expect("no description");
    assert_eq!(described, "1440x900 / RGB / 8bpp / 60Hz");
}

/// An audio module on released firmware up to 4.4.2 sends 24-byte entries
/// and no links; the length tells it from the current layout, links and all.
#[test]
fn an_audio_report_is_read_at_either_entry_width() {
    // Two endpoints at the older width: an output and an input.
    let mut h = Harness::new(18);
    h.hello(0x1E, "ONEIP", "AU0018", DeviceFeature::V2IP_SINK);
    let mut p = poisoned(36 + 2 * 24);
    p[0..2].copy_from_slice(&0u16.to_le_bytes());
    p[28..30].copy_from_slice(&2u16.to_le_bytes());
    for (i, (id, features)) in [(0u8, AudioFeatures::OUTPUT), (1, AudioFeatures::INPUT)]
        .into_iter()
        .enumerate()
    {
        let at = 36 + i * 24;
        p[at] = id;
        p[at + 1] = 1; // an endpoint
        p[at + 8..at + 12].copy_from_slice(&features.bits().to_le_bytes());
    }
    h.feed_audio_report(&p);
    let eps = h.device().audio.clone().expect("no audio endpoints");
    assert!(eps
        .get(0)
        .is_some_and(|e| e.features.has(AudioFeatures::OUTPUT)));
    assert!(eps
        .get(1)
        .is_some_and(|e| e.features.has(AudioFeatures::INPUT)));
    assert_eq!(h.device().audio_legacy, Some(true));

    // Three current entries and one link come to the length of three older
    // entries; the link count says which it is.
    let mut h = Harness::new(19);
    h.hello(0x2F, "ONEIP", "AU0019", DeviceFeature::V2IP_SINK);
    let mut p = poisoned(36 + 3 * 16);
    p[0..2].copy_from_slice(&0u16.to_le_bytes());
    p[28..30].copy_from_slice(&3u16.to_le_bytes());
    for i in 0..3u8 {
        let at = 36 + usize::from(i) * 16;
        p[at] = i;
        p[at + 1] = 1;
        p[at + 8..at + 12].copy_from_slice(&AudioFeatures::OUTPUT.bits().to_le_bytes());
    }
    p.extend_from_slice(&[1, 0, 0, 0]);
    p.extend_from_slice(&[2, 4, 0, 0]);
    p.extend_from_slice(uid_n(20).as_bytes());
    assert_eq!(p.len(), 36 + 3 * 24);
    h.feed_audio_report(&p);
    let eps = h.device().audio.clone().expect("no audio endpoints");
    assert_eq!(eps.list().count(), 3);
    assert_eq!(eps.get(2).map(|e| e.linked_device), Some(uid_n(20)));
    assert_eq!(h.device().audio_legacy, Some(false));
}
