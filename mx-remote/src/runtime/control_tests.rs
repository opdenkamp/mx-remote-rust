// Author: Lars Op den Kamp (lars@opdenkamp-it.nl)
// Copyright (c) 2026 Op den Kamp IT Solutions

//! The transmit surface: the protocol gate, and what the frames mean.
//!
//! A client here is built without a socket, so a send that clears the gate
//! fails afterwards with [`SendError::NotConnected`]. That is what most of the
//! tests want: it separates "refused" from "sent", and the tap on the transmit
//! path still hands back the frame that would have gone out. Where a test needs
//! the difference between the two to be visible in the registry rather than in
//! a return value, [`Fixture::connect`] gives the client a socket that a send
//! can actually succeed on.

use std::net::Ipv4Addr;
use std::sync::{Arc, Mutex};

use crate::event::EventHandler;
use crate::testing::{bay_config_rec, datagram, hello_payload, stream_rec, uid_n, vlan_block, Cfg};
use crate::types::{
    ActionTransmitRequest, AmpZoneSettings, AudioChangeSource, AudioFeatures, BayNameChange,
    EdidProfileChange, EdidRequest, KeyTransmitRequest, UnitNetConfig, V2ipAudioFormat,
    V2ipDownmix, V2ipOutputMode, V2ipPtpMessages, V2ipRoute, V2ipRouteTarget, V2ipVlan,
    SCALING_FLAG_AUTO_SCALING, SCALING_FLAG_MODE_VALID, SCALING_FLAG_OPTIONS_VALID,
    V2IP_MINUTES_PER_DAY, V2IP_PTP_DOMAIN_MAX, V2IP_VLAN_ID_MAX, VOLUME_UNCHANGED,
};
use crate::wire::{
    build_amp_zone_settings, build_v2ip_manual_source_switch, build_video_wall, op, protocol_for,
    Addressee, BayFeatures, BayStatus, BayUid, Conn, DeviceFeature, DeviceUid, EdidProfile,
    MultiviewerAspectRatio, MultiviewerEdidTemplate, MultiviewerHdcpMode, MultiviewerItcMode,
    MultiviewerOutputMode, MultiviewerPipPosition, MultiviewerPipSize, MultiviewerSource,
    MultiviewerViewMode, Opcode, RcAction, RcKey, SendError, StreamAddr, UnitFlag, V2ipColourSpace,
    V2ipDeviceSetting, V2ipDownmixLevel, V2ipDownmixMode, V2ipFpgaFeature, V2ipPowerSaveLevel,
    V2ipPtpMode, V2ipStreams, V2ipTestPattern, V2ipToneMode, V2ipVlanFlag, HEADER_LEN,
    PROTOCOL_VERSION, V2IP_AUDIO_DEFAULT_CHANNELS, V2IP_AUDIO_DEFAULT_SAMPLE_RATE, V2IP_DSCP_SET,
    V2IP_IR_PROFILE_MAX, V2IP_IR_PROFILE_NOT_SET, V2IP_PORT_ANC, V2IP_PORT_AUDIO, V2IP_PORT_VIDEO,
    V2IP_SOURCE_RATE_MAX, V2IP_SOURCE_RATE_MIN,
};

use super::control::ControlError;
use super::tests::{client_with, Tap};
use super::*;

/// The address every fixture frame appears to come from.
const FROM: Ipv4Addr = Ipv4Addr::new(10, 8, 8, 9);

/// The client's own identifier, which no peer below uses: a frame carrying it
/// would be dropped as this client's own echo.
const CLIENT: u8 = 0xFE;

/// Where [`Fixture::connect`] points its socket. Deliberately not the MX Remote
/// port, so nothing here can reach a live AV network.
const SCRATCH_PORT: u16 = 18818;

/// Whether the gate refused this call, as opposed to anything else failing.
///
/// The distinction is the whole point: a client with no socket fails every
/// send, so "returned an error" would pass with the gate deleted.
fn refused(result: &Result<(), ControlError>) -> bool {
    matches!(
        result,
        Err(ControlError::Send(SendError::ProtocolTooOld { .. }))
    )
}

/// A route whose three groups differ, so a pair swapped between slots shows.
fn route(n: u8) -> V2ipRoute {
    let at = |last| V2ipRouteTarget::new(Ipv4Addr::new(239, 11, n, last));
    V2ipRoute {
        video: at(1),
        audio: at(2),
        anc: at(3),
    }
}

/// A client that has heard from peers, and the tap on what it sends.
struct Fixture {
    remote: Remote,
    tap: Arc<Tap>,
}

impl Fixture {
    fn new() -> Self {
        Self::with_handler(Arc::new(()))
    }

    fn with_handler(handler: Arc<dyn EventHandler>) -> Self {
        let (remote, tap) = client_with(CLIENT, handler);
        Self { remote, tap }
    }

    fn feed(&self, sender: DeviceUid, opcode: Opcode, payload: &[u8]) {
        self.feed_proto(sender, opcode, PROTOCOL_VERSION, payload);
    }

    fn feed_proto(&self, sender: DeviceUid, opcode: Opcode, protocol: u16, payload: &[u8]) {
        self.remote
            .shared
            .process_datagram(&datagram(sender, opcode, protocol, payload), FROM);
    }

    /// Announces a ProAmp8 with one amplifier zone on port 1.
    fn amplifier(&self, uid: DeviceUid, protocol: u16, serial: &str) {
        self.feed(
            uid,
            op::SYS_HELLO,
            &hello_payload(
                protocol,
                "ProAmp8",
                serial,
                "4.8.0",
                DeviceFeature::AUDIO_AMPLIFIER,
            ),
        );
        self.feed(
            uid,
            op::SYS_BAY_CONFIG,
            &bay_config_rec(
                1,
                1,
                0,
                "Zone 1",
                "Hall",
                BayStatus::NONE,
                BayFeatures::AUDIO_AMP_OUT,
            ),
        );
    }

    /// Feeds a status report putting `uid`'s multiviewer in the layout whose
    /// hardware view mode is `hw_view_mode`, which is what bounds the window
    /// index a caller may address.
    fn multiviewer_showing(&self, uid: DeviceUid, hw_view_mode: u8) {
        let mut p = vec![0u8; 192];
        p[..16].copy_from_slice(uid.as_bytes());
        p[16] = 0;
        p[24..40].copy_from_slice(uid.as_bytes());
        p[168] = hw_view_mode;
        self.feed(uid, op::V2IP_MULTIVIEWER, &p);
    }

    /// Announces a unit that is every kind of thing a guarded send addresses:
    /// a OneIP source on port 1, a OneIP sink and amplifier zone on port 2, a
    /// multiviewer, and stream addresses for the source.
    ///
    /// Every guard has to be reached to be tested, so each method's own
    /// preconditions have to pass before the gate is the thing that refuses it.
    fn everything(&self, uid: DeviceUid, protocol: u16, serial: &str) {
        self.feed(
            uid,
            op::SYS_HELLO,
            &hello_payload(
                protocol,
                "OneIP",
                serial,
                "4.8.0",
                DeviceFeature::V2IP_SINK
                    | DeviceFeature::V2IP_SOURCE
                    | DeviceFeature::MULTIVIEWER
                    | DeviceFeature::AUDIO_AMPLIFIER
                    | DeviceFeature::VIDEO_ROUTING
                    | DeviceFeature::VOLUME_CONTROL
                    | DeviceFeature::AUDIO_ROUTING
                    | DeviceFeature::VIDEO_WALL,
            ),
        );
        let mut bays = bay_config_rec(
            1,
            0,
            0,
            "Input 1",
            "Apple TV",
            BayStatus::NONE,
            BayFeatures::HDMI_IN | BayFeatures::V2IP_SOURCE_LOCAL,
        );
        bays.extend_from_slice(&bay_config_rec(
            2,
            1,
            0,
            "Output 1",
            "TV",
            BayStatus::NONE,
            BayFeatures::HDMI_OUT | BayFeatures::AUDIO_AMP_OUT | BayFeatures::V2IP_SINK_LOCAL,
        ));
        self.feed(uid, op::SYS_BAY_CONFIG, &bays);
        // A group of its own per device. A route is resolved by searching
        // every device for the one advertising the address, so two devices
        // sharing a group would make the answer whichever was reached first.
        let n = uid.as_bytes()[0];
        self.feed(
            uid,
            op::SYS_BAY_V2IP_SOURCES,
            &stream_rec(
                uid,
                &format!("239.7.{n}.1"),
                &format!("239.7.{n}.2"),
                &format!("239.7.{n}.3"),
                V2IP_PORT_VIDEO,
            ),
        );
    }

    /// Sends, then replays the captured frame as though `sender` had sent it.
    ///
    /// A frame this client builds carries this client's own uid, and the
    /// receive path drops those as echoes, so the sender has to be rewritten
    /// for the library to decode what it just built. The send itself fails for
    /// want of a socket, which is not what a round trip is asking about.
    ///
    /// The sender is what this client is to a unit, management: units take
    /// most of these frames from no one else.
    ///
    /// Exactly one frame must be waiting, so the tap is cleared afterwards and
    /// the first call in a test clears it beforehand.
    fn round_trip(&self, what: &str, sender: DeviceUid, call: Result<(), ControlError>) {
        assert!(
            matches!(call, Err(ControlError::Send(SendError::NotConnected))),
            "{what} failed for a reason other than the missing socket: {call:?}"
        );
        let frames = self.tap.frames();
        assert_eq!(frames.len(), 1, "{what} captured {} frames", frames.len());
        let mut frame = frames.into_iter().next().expect("one frame");
        frame[4..20].copy_from_slice(sender.as_bytes());
        self.remote.shared.mutate(|state, _| {
            if let Some(d) = state.device_mut(sender) {
                d.hello.features |= DeviceFeature::MANAGER;
            }
        });
        self.remote.shared.process_datagram(&frame, FROM);
        self.tap.clear();
    }

    /// Gives the client a socket, so a send that clears the gate succeeds.
    ///
    /// It is pointed at the loopback address rather than at a multicast group:
    /// that needs no interface beyond `lo`, so it cannot be skipped for want of
    /// one, and the datagram reaches nothing but this socket's own buffer,
    /// which nothing reads.
    fn connect(&self) {
        let conn = Conn::open(
            Ipv4Addr::LOCALHOST,
            SCRATCH_PORT,
            Some(Ipv4Addr::LOCALHOST),
            None,
        )
        .expect("a loopback socket could not be opened");
        lock(&self.remote.shared.tx).set_conn(Some(conn));
    }
}

#[test]
fn a_send_is_refused_below_the_opcode_floor() {
    let f = Fixture::new();

    // A ProAmp8 on 4.1.1 reports 0x11, below the 0x1C floor of
    // AMP_ZONE_SETTINGS and the 0x13 of V2IP_STATS.
    let old = uid_n(150);
    f.amplifier(old, 0x11, "AMP4111");
    let zone = BayUid::new(old, 1);

    assert!(refused(
        &f.remote
            .set_amp_zone_settings(zone, AmpZoneSettings::default())
    ));
    assert!(refused(&f.remote.subscribe_v2ip_stats(old, true)));
    // SYS_REBOOT floors at 0x01 and stays allowed.
    assert!(!refused(&f.remote.reboot(old)));

    // A device that reported no version at all is let through: not knowing is
    // not the same as knowing it is too old.
    let unknown = uid_n(152);
    f.amplifier(unknown, 0, "UNK0001");
    assert!(!refused(&f.remote.set_amp_zone_settings(
        BayUid::new(unknown, 1),
        AmpZoneSettings::default()
    )));

    let current = uid_n(151);
    f.amplifier(current, 0x28, "AMP0480");
    assert!(!refused(&f.remote.set_amp_zone_settings(
        BayUid::new(current, 1),
        AmpZoneSettings::default()
    )));
}

/// Every guarded send, called against one device that is below every floor
/// here and one that is above all of them.
///
/// Asserting only the refusal would pass for a guard hardcoded to refuse, or
/// for a command failing for some unrelated reason; the pair is what makes a
/// refusal mean "because of the protocol floor".
type Guarded = (
    &'static str,
    fn(&Remote, DeviceUid) -> Result<(), ControlError>,
);

const GUARDED_SENDS: &[Guarded] = &[
    ("select_video_source", |r, d| {
        r.select_video_source(BayUid::new(d, 2), 1)
    }),
    ("select_audio_source", |r, d| {
        r.select_audio_source(BayUid::new(d, 2), 1)
    }),
    ("select_video_source_by_name", |r, d| {
        r.select_video_source_by_name(BayUid::new(d, 2), "Apple TV")
    }),
    ("select_audio_source_by_name", |r, d| {
        r.select_audio_source_by_name(BayUid::new(d, 2), "Apple TV", None)
    }),
    ("select_audio_source_by_name/format", |r, d| {
        r.select_audio_source_by_name(
            BayUid::new(d, 2),
            "Apple TV",
            Some(V2ipAudioFormat {
                sample_rate: 48000,
                channels: 2,
            }),
        )
    }),
    ("select_audio_source_addr", |r, d| {
        r.select_audio_source_addr(BayUid::new(d, 2), Ipv4Addr::new(239, 1, 1, 1), None, None)
    }),
    ("select_source_addr", |r, d| {
        r.select_source_addr(BayUid::new(d, 2), route(1), None)
    }),
    ("set_bay_name", |r, d| {
        r.set_bay_name(BayUid::new(d, 2), "Kitchen")
    }),
    ("set_bay_hidden", |r, d| {
        r.set_bay_hidden(BayUid::new(d, 2), true)
    }),
    ("select_edid_profile", |r, d| {
        r.select_edid_profile(BayUid::new(d, 1), EdidProfile::UHD_4K)
    }),
    ("send_action", |r, d| {
        r.send_action(BayUid::new(d, 2), RcAction::POWER_ON)
    }),
    ("send_key", |r, d| {
        r.send_key(BayUid::new(d, 2), RcKey::PLAY)
    }),
    ("request_signal_status", |r, d| {
        r.request_signal_status(Some(d))
    }),
    ("power_on", |r, d| r.power_on(BayUid::new(d, 2))),
    ("set_volume", |r, d| {
        r.set_volume(BayUid::new(d, 2), 40, None)
    }),
    ("set_amp_zone_settings", |r, d| {
        r.set_amp_zone_settings(BayUid::new(d, 2), AmpZoneSettings::default())
    }),
    ("subscribe_v2ip_stats", |r, d| {
        r.subscribe_v2ip_stats(d, true)
    }),
    ("select_audio_endpoint_input", |r, d| {
        r.select_audio_endpoint_input(d, 1, d, 2)
    }),
    ("set_multiviewer_view_mode", |r, d| {
        r.set_multiviewer_view_mode(d, MultiviewerViewMode::PIP)
    }),
    ("set_multiviewer_video_source", |r, d| {
        r.set_multiviewer_video_source(d, 0, MultiviewerSource::INPUT_2)
    }),
    ("multiviewer_auto_route", |r, d| r.multiviewer_auto_route(d)),
    ("store_video_wall", |r, d| {
        r.store_video_wall(d, VIDEO_WALL_CLEARED)
    }),
    ("revert_video_wall", |r, d| r.revert_video_wall(d)),
];

#[test]
fn every_guarded_send_checks_the_protocol_floor() {
    let f = Fixture::new();

    // 0x01 is below the floor of every opcode in the table. A device at 0x11
    // would leave most of these guards untested, since their floors are at or
    // below it.
    let old = uid_n(161);
    f.everything(old, 0x01, "OLD0001");
    for (name, call) in GUARDED_SENDS {
        let got = call(&f.remote, old);
        assert!(refused(&got), "0x01 device, {name}: {got:?}");
    }
    // SYS_REBOOT floors at 0x01, so even this device must not be refused it.
    //
    // Its guard is therefore unreachable: nothing can report below 0x01, and a
    // device reporting nothing is let through deliberately. What is pinned
    // here is that reboot stays allowed; the guard is kept because it reads the
    // floor from the table, so it starts working on its own if SYS_REBOOT ever
    // gains one.
    assert!(!refused(&f.remote.reboot(old)));
    // DEV_EDID floors at 0x01 too, and is out of the table for the same
    // reason: asking the oldest device for its EDID must not be refused.
    assert!(!refused(&f.remote.request_edid(old, true)));

    let current = uid_n(162);
    f.everything(current, 0x28, "NEW0001");
    for (name, call) in GUARDED_SENDS {
        assert!(
            !refused(&call(&f.remote, current)),
            "0x28 device, {name}: refused, but it is above every floor here"
        );
    }
}

#[test]
fn a_device_exactly_on_a_floor_is_allowed() {
    let f = Fixture::new();
    let uid = uid_n(164);
    // 0x11 is exactly AUDIO_SET_VOLUME's floor and below V2IP_STATS' 0x13.
    f.everything(uid, 0x11, "EDGE001");

    assert!(!refused(&f.remote.set_volume(
        BayUid::new(uid, 2),
        40,
        None
    )));
    assert!(refused(&f.remote.subscribe_v2ip_stats(uid, true)));
}

#[test]
fn the_gate_sits_on_the_transmit_path_itself() {
    let f = Fixture::new();
    let (old, current) = (uid_n(171), uid_n(172));
    f.amplifier(old, 0x01, "OLD1");
    f.amplifier(current, 0x28, "NEW1");

    let addressee = |uid: DeviceUid| {
        f.remote
            .shared
            .read(|state| Addressee::device(state.device(uid).expect("device not registered")))
    };
    let send = |to: Addressee| {
        f.remote
            .shared
            .send(&to, op::AMP_ZONE_SETTINGS, &[0u8; 56])
            .map(|_| ())
            .map_err(ControlError::Send)
    };

    assert!(refused(&send(addressee(old))));
    // A current target and an untargeted broadcast both get past the gate and
    // fail after it for want of a socket, which is a different error.
    assert!(!refused(&send(addressee(current))));
    assert!(!refused(&send(Addressee::Broadcast)));
}

/// Round-tripping this library's own builders through its own decoders is the
/// only instrument here that tests *meaning* rather than position.
///
/// A field written at the right offset but attributed to the wrong thing -
/// source read as target, left delay as right - passes every positional check,
/// because nothing about the byte layout is wrong. A round trip catches it when
/// the builder and the decoder disagree about what a field means, which is what
/// an orientation bug looks like: two sides of one library, written at
/// different times, disagreeing.
///
/// Where builder and decoder are wrong *together* a round trip is clean. That
/// case needs the byte-exact vectors or the firmware struct, and no round trip
/// can substitute for either.
#[test]
fn a_built_amp_zone_frame_decodes_to_what_was_built() {
    let f = Fixture::new();
    let sender = uid_n(181);
    f.everything(sender, 0x28, "RT0001");

    // Every field distinct, and the two delays deliberately unequal so that
    // swapping them is visible.
    let want = AmpZoneSettings {
        gain_left: 190,
        gain_right: 191,
        volume_min: 12,
        volume_max: 220,
        delay_left: 96000,
        delay_right: 144000,
        bass: 130,
        treble: 131,
        bridged: 1,
        power_mode: 2,
        power_level: 33,
        power_timeout: 900,
        eq_left: [120, 121, 122, 123, 124],
        eq_right: [140, 141, 142, 143, 144],
    };
    f.feed(
        sender,
        op::AMP_ZONE_SETTINGS,
        &build_amp_zone_settings(sender, 2, &want),
    );

    let got = f
        .remote
        .bay(BayUid::new(sender, 2))
        .expect("no bay")
        .amp_settings
        .expect("what the builder produced did not decode at all");
    assert_eq!(got, want);
}

#[test]
fn a_built_manual_switch_frame_decodes_to_what_was_built() {
    let f = Fixture::new();
    let sender = uid_n(183);
    f.everything(sender, 0x28, "RT0002");

    // Three distinct addresses and ports, so any pair being swapped shows.
    let payload = build_v2ip_manual_source_switch(
        sender,
        V2ipStreams {
            video: StreamAddr {
                ip: Ipv4Addr::new(239, 10, 0, 1),
                port: V2IP_PORT_VIDEO,
            },
            audio: StreamAddr {
                ip: Ipv4Addr::new(239, 10, 0, 2),
                port: V2IP_PORT_AUDIO,
            },
            anc: StreamAddr {
                ip: Ipv4Addr::new(239, 10, 0, 3),
                port: V2IP_PORT_ANC,
            },
        },
        Some(V2ipAudioFormat {
            sample_rate: 96000,
            channels: 6,
        }),
    );
    let controller = uid_n(184);
    f.feed(
        controller,
        op::SYS_HELLO,
        &hello_payload(0x28, "Ctrl", "RT0005", "4.8.0", DeviceFeature::MANAGER),
    );
    f.feed(controller, op::V2IP_MANUAL_SRC_SWITCH, &payload);

    let sink = f
        .remote
        .v2ip_sink(sender)
        .expect("what the builder produced did not decode at all");
    for (what, got, ip, port) in [
        (
            "video",
            sink.addresses.video,
            Ipv4Addr::new(239, 10, 0, 1),
            V2IP_PORT_VIDEO,
        ),
        (
            "audio",
            sink.addresses.audio,
            Ipv4Addr::new(239, 10, 0, 2),
            V2IP_PORT_AUDIO,
        ),
        (
            "anc",
            sink.addresses.anc,
            Ipv4Addr::new(239, 10, 0, 3),
            V2IP_PORT_ANC,
        ),
    ] {
        assert_eq!((what, got.ip, got.port), (what, ip, port));
    }
    let format = sink.audio_fmt.expect("no audio format");
    assert_eq!((format.sample_rate, format.channels), (96000, 6));
}

/// What the control methods themselves put on the wire, not just the payload
/// builders that happen to be separate functions.
///
/// Attempting this is an audit in itself, and it surfaced two things about this
/// library that no other test states. A captured frame carries this client's
/// own uid as its sender and the receive path drops those as echoes, so the
/// harness has to rewrite the sender to a peer: the library genuinely cannot
/// decode its own sends. And once it does, the handlers split - some act on the
/// device named in the payload, others on whoever sent the frame - so a round
/// trip only proves something if it asserts against the bay the request names
/// rather than whichever one the handler happens to update.
#[derive(Default)]
struct Requests {
    names: Mutex<Vec<BayNameChange>>,
    profiles: Mutex<Vec<EdidProfileChange>>,
    actions: Mutex<Vec<ActionTransmitRequest>>,
}

impl EventHandler for Requests {
    fn on_bay_name_change_requested(&self, _device: DeviceUid, change: BayNameChange) {
        self.names.lock().expect("test handler").push(change);
    }

    fn on_edid_profile_change_requested(&self, _device: DeviceUid, change: EdidProfileChange) {
        self.profiles.lock().expect("test handler").push(change);
    }

    fn on_action_transmit_requested(&self, _device: DeviceUid, request: ActionTransmitRequest) {
        self.actions.lock().expect("test handler").push(request);
    }
}

#[test]
fn a_control_method_decodes_back_to_what_it_asked_for() {
    let seen = Arc::new(Requests::default());
    let f = Fixture::with_handler(Arc::clone(&seen) as Arc<dyn EventHandler>);
    let (target, peer) = (uid_n(191), uid_n(192));
    f.everything(target, 0x28, "RT0003");
    f.everything(peer, 0x28, "RT0004");

    let round_trip = |what: &str, call| f.round_trip(what, peer, call);

    let out = BayUid::new(target, 2);
    let input = BayUid::new(target, 1);

    // Addressed by the uid in the payload.
    f.tap.clear();
    round_trip("set_bay_name", f.remote.set_bay_name(out, "Kitchen Amp"));
    let name = seen
        .names
        .lock()
        .expect("test handler")
        .pop()
        .expect("none");
    assert_eq!(name.target, target);
    assert_eq!(name.port, 2);
    assert_eq!(name.name, "Kitchen Amp");

    round_trip(
        "select_edid_profile",
        f.remote
            .select_edid_profile(input, EdidProfile::HDR_SURROUND71_4K),
    );
    let profile = seen
        .profiles
        .lock()
        .expect("test handler")
        .pop()
        .expect("none");
    assert_eq!(profile.target, target);
    assert_eq!(profile.profile, EdidProfile::HDR_SURROUND71_4K);

    round_trip("send_action", f.remote.send_action(out, RcAction::POWER_ON));
    let action = seen
        .actions
        .lock()
        .expect("test handler")
        .pop()
        .expect("none");
    assert_eq!(action.target, target);
    assert_eq!(action.local_bay, 2);
    assert_eq!(action.action, RcAction::POWER_ON);

    round_trip("set_bay_hidden", f.remote.set_bay_hidden(out, true));
    assert_eq!(
        f.remote.bay(out).expect("no bay").hidden,
        Some(true),
        "hidden did not round trip onto the addressed bay"
    );

    round_trip("set_volume", f.remote.set_volume(out, 37, Some(false)));
    assert_eq!(
        f.remote
            .bay(out)
            .expect("no bay")
            .volume
            .and_then(|v| v.volume_left),
        Some(37),
        "volume did not round trip onto the addressed bay"
    );
    // The sender is a different device here, and owns a bay with the same port
    // number, so a handler reading the sender lands on this one instead of
    // failing to land anywhere.
    assert_ne!(
        f.remote
            .bay(BayUid::new(peer, 2))
            .expect("no peer bay")
            .volume
            .and_then(|v| v.volume_left),
        Some(37),
        "volume landed on the sender's bay rather than the one it named"
    );
}

/// A video-wall frame is 32 bytes, which is three more than its fields.
///
/// The struct is 4-aligned, so the op byte at 28 is followed by padding, and
/// the sink's length check is against the whole struct. A payload built by
/// summing field widths is 29 bytes and is dropped without a word, which is
/// the one mistake on this opcode that no amount of correct geometry saves.
/// Collects the video-wall commands a round trip decodes back.
#[derive(Default)]
struct Walls(Mutex<Vec<VideoWallCommand>>);

impl EventHandler for Walls {
    fn on_video_wall_command(&self, _device: DeviceUid, command: VideoWallCommand) {
        self.0.lock().expect("test handler").push(command);
    }
}

impl Walls {
    fn latest(&self) -> VideoWallCommand {
        *self
            .0
            .lock()
            .expect("test handler")
            .last()
            .expect("no wall command")
    }
}

/// A unit on released firmware up to 4.4.2 is sent the 8-byte form naming
/// two of its own endpoints, and anything newer the form naming both units;
/// its own features report says which it runs, and its protocol until then.
#[test]
fn an_endpoint_selection_is_sent_in_the_units_own_layout() {
    let f = Fixture::new();
    f.connect();
    let (old, new, other) = (uid_n(205), uid_n(206), uid_n(207));
    f.everything(old, 0x1E, "AS0001");
    f.everything(new, 0x2F, "AS0002");
    f.everything(other, 0x2F, "AS0003");

    f.tap.clear();
    f.remote
        .select_audio_endpoint_input(old, 4, old, 9)
        .expect("sent");
    let frame = f.tap.frames().pop().expect("nothing reached the gate");
    let p = &frame[HEADER_LEN..];
    assert_eq!(p.len(), 28);
    assert_eq!(&p[20..28], &[4, 0, 0, 0, 9, 0, 0, 0]);
    assert!(matches!(
        f.remote.select_audio_endpoint_input(old, 4, other, 9),
        Err(ControlError::Unsupported(_))
    ));

    f.tap.clear();
    f.remote
        .select_audio_endpoint_input(new, 4, other, 9)
        .expect("sent");
    let frame = f.tap.frames().pop().expect("nothing reached the gate");
    assert_eq!(frame.len() - HEADER_LEN, 56);

    // A report in the current layout from the older-numbered unit decides it.
    let mut tree = vec![0u8; 36 + 16];
    tree[4..20].copy_from_slice(old.as_bytes());
    tree[28..30].copy_from_slice(&1u16.to_le_bytes());
    tree[37] = 1;
    f.feed(old, op::V2IP_AUDIO, &tree);
    f.tap.clear();
    f.remote
        .select_audio_endpoint_input(old, 4, other, 9)
        .expect("sent");
    let frame = f.tap.frames().pop().expect("nothing reached the gate");
    assert_eq!(frame.len() - HEADER_LEN, 56);
}

/// No unit acts on an endpoint volume, and a released one reads its
/// sub-opcode as selecting an output, so it is never sent.
#[test]
fn an_endpoint_volume_is_never_sent() {
    let f = Fixture::new();
    let uid = uid_n(208);
    f.everything(uid, 0x2F, "AV0001");
    f.connect();
    f.tap.clear();
    assert!(matches!(
        f.remote.set_audio_endpoint_volume(uid, 1, 40),
        Err(ControlError::Unsupported(_))
    ));
    assert!(f.tap.frames().is_empty());
}

/// A unit without the video wall module drops the frame unanswered, so it is
/// refused for one that does not report the feature.
#[test]
fn a_video_wall_goes_only_to_a_unit_with_the_module() {
    let f = Fixture::new();
    let sink = uid_n(209);
    f.feed(
        sink,
        op::SYS_HELLO,
        &hello_payload(0x2F, "OneIP", "VW0000", "4.8.0", DeviceFeature::V2IP_SINK),
    );
    f.connect();
    f.tap.clear();
    assert!(matches!(
        f.remote.revert_video_wall(sink),
        Err(ControlError::Unsupported(_))
    ));
    assert!(f.tap.frames().is_empty());
}

#[test]
fn a_video_wall_frame_carries_its_trailing_padding() {
    let f = Fixture::new();
    let sink = uid_n(210);
    f.everything(sink, 0x28, "VW0001");
    f.connect();

    let window = VideoWallWindow {
        pos_x: 1920,
        pos_y: 1080,
        width: 1920,
        height: 1080,
        raster_w: 3840,
        raster_h: 2160,
    };

    f.tap.clear();
    f.remote
        .store_video_wall(sink, window)
        .expect("the socket is open and the device is above the floor");
    let frame = f.tap.frames().pop().expect("nothing reached the gate");
    assert_eq!(
        frame.len() - HEADER_LEN,
        32,
        "the payload is not the size the sink measures against"
    );
    // The declared length has to agree, or the frame is dropped on the
    // envelope check before any handler sees it.
    assert_eq!(u16::from_le_bytes([frame[22], frame[23]]), 32);
    assert_eq!(frame[HEADER_LEN + 28], VideoWallOp::STORE.to_wire());
}

/// A rename is 40 bytes, which is six more than its fields.
///
/// `mxr_bay_name_data` is `ALIGN(8)`, so the 16-byte name at 18 is followed by
/// six bytes of padding, and the addressed device measures the payload against
/// the whole struct before it reads a field. A payload built by summing field
/// widths is 34 bytes and is discarded on the length check, which looks
/// exactly like a rename that was accepted and ignored.
#[test]
fn a_rename_carries_its_trailing_padding() {
    let f = Fixture::new();
    let device = uid_n(211);
    f.everything(device, 0x28, "BN0001");
    f.connect();

    f.tap.clear();
    f.remote
        .set_bay_name(BayUid::new(device, 2), "Kitchen")
        .expect("the socket is open and the device is above the floor");
    let frame = f.tap.frames().pop().expect("nothing reached the gate");
    assert_eq!(
        frame.len() - HEADER_LEN,
        40,
        "the payload is not the size the device measures against"
    );
    assert_eq!(u16::from_le_bytes([frame[22], frame[23]]), 40);
    // The padding is behind the name, not in front of it: a frame the right
    // length with the name in the wrong place passes the check and renames
    // nothing recognisable.
    assert_eq!(&frame[HEADER_LEN + 18..HEADER_LEN + 25], b"Kitchen");
}

/// A unit keeps 15 bytes of a name, so that is what is sent and remembered,
/// cut before a character that would not fit whole.
#[test]
fn a_rename_is_cut_to_what_a_unit_keeps() {
    let f = Fixture::new();
    let device = uid_n(212);
    f.everything(device, 0x28, "BN0002");
    f.connect();
    let bay = BayUid::new(device, 2);
    for (asked, kept) in [
        ("Living Room Left", "Living Room Lef"),
        ("Living Room Lé", "Living Room Lé"),
        ("Living Room Lée", "Living Room Lé"),
        ("Living Room Lfé", "Living Room Lf"),
    ] {
        f.tap.clear();
        f.remote.set_bay_name(bay, asked).expect("sent");
        let frame = f.tap.frames().pop().expect("nothing reached the gate");
        let field = &frame[HEADER_LEN + 18..HEADER_LEN + 34];
        let sent = &field[..field.iter().position(|&b| b == 0).unwrap_or(16)];
        assert_eq!(sent, kept.as_bytes(), "{asked}");
        assert_eq!(f.remote.bay(bay).expect("bay").user_name, kept, "{asked}");
    }
}

/// Each of the three operations decodes back to what it asked for.
#[test]
fn a_video_wall_command_decodes_back_to_what_was_asked() {
    let seen = Arc::new(Walls::default());
    let f = Fixture::with_handler(Arc::clone(&seen) as Arc<dyn EventHandler>);
    let (sink, peer) = (uid_n(211), uid_n(212));
    f.everything(sink, 0x28, "VW0002");
    f.everything(peer, 0x28, "VW0003");

    // Every field a distinct value, so a pair read into each other's offsets
    // cannot pass.
    let window = VideoWallWindow {
        pos_x: 64,
        pos_y: 100,
        width: 128,
        height: 200,
        raster_w: 1920,
        raster_h: 1080,
    };

    f.tap.clear();
    f.round_trip(
        "store_video_wall",
        peer,
        f.remote.store_video_wall(sink, window),
    );
    let got = seen.latest();
    assert_eq!(got.target, sink);
    assert_eq!((got.pos_x, got.pos_y), (64, 100));
    assert_eq!((got.width, got.height), (128, 200));
    assert_eq!((got.raster_w, got.raster_h), (1920, 1080));
    assert_eq!(got.op, VideoWallOp::STORE);
    assert!(got.has_window() && !got.is_cleared());

    // A preview names the same window under a different operation, which is
    // the only thing separating "show this" from "keep this".
    f.round_trip(
        "preview_video_wall",
        peer,
        f.remote.preview_video_wall(sink, window),
    );
    assert_eq!(seen.latest().op, VideoWallOp::PREVIEW);

    // A clear is a window of zero size rather than an operation of its own.
    f.round_trip(
        "clear",
        peer,
        f.remote.store_video_wall(sink, VIDEO_WALL_CLEARED),
    );
    let got = seen.latest();
    assert!(got.is_cleared());
    assert_eq!(got.op, VideoWallOp::STORE);

    // A revert carries no window, and must not carry the last one sent: a
    // reader of the frame could not otherwise tell it from a placement.
    f.round_trip("revert_video_wall", peer, f.remote.revert_video_wall(sink));
    let got = seen.latest();
    assert_eq!(got.op, VideoWallOp::REVERT);
    assert!(!got.has_window());
    assert_eq!((got.pos_x, got.pos_y, got.width, got.height), (0, 0, 0, 0));

    // That the frame above was empty proves nothing on its own, because the
    // method hands the builder a cleared window to begin with. The guarantee
    // lives in the builder, so reaching it is the only way to test it: a
    // future caller with a window in hand must not be able to put one on a
    // revert. The six geometry fields sit between the uid and the op byte.
    let carried = build_video_wall(sink, window, VideoWallOp::REVERT);
    assert_eq!(
        &carried[16..28],
        &[0u8; 12],
        "a revert carried the geometry it was handed"
    );
    assert_eq!(carried[28], VideoWallOp::REVERT.to_wire());
}

/// A window the sink might store and never recover from is refused here.
///
/// Every case is one value away from the accepted window, and that window is
/// asserted to pass, so a guard that refused everything would fail this too.
#[test]
fn an_out_of_spec_video_wall_window_is_refused_before_it_is_sent() {
    let f = Fixture::new();
    let sink = uid_n(213);
    f.everything(sink, 0x28, "VW0004");
    f.connect();

    let good = VideoWallWindow {
        pos_x: 64,
        pos_y: 100,
        width: 128,
        height: 200,
        raster_w: 1920,
        raster_h: 1080,
    };
    assert!(
        f.remote.store_video_wall(sink, good).is_ok(),
        "the window every case below is derived from was itself refused"
    );

    let refused = |what: &str, window: VideoWallWindow| {
        f.tap.clear();
        let got = f.remote.store_video_wall(sink, window);
        assert!(
            matches!(got, Err(ControlError::InvalidRequest(_))),
            "{what}: {got:?}"
        );
        assert!(
            f.tap.frames().is_empty(),
            "{what} reached the wire despite being refused"
        );
    };

    refused(
        "origin off the 64-pixel grid",
        VideoWallWindow { pos_x: 32, ..good },
    );
    refused(
        "width not a multiple of 4",
        VideoWallWindow { width: 130, ..good },
    );
    refused(
        "width below the scaler minimum",
        VideoWallWindow { width: 60, ..good },
    );
    refused(
        "height below the scaler minimum",
        VideoWallWindow { height: 63, ..good },
    );
    refused(
        "window running off the raster horizontally",
        VideoWallWindow {
            pos_x: 1920,
            ..good
        },
    );
    refused(
        "window running off the raster vertically",
        VideoWallWindow {
            pos_y: 1000,
            ..good
        },
    );
    // A sum that wraps reads as containment on 16 bits.
    refused(
        "origin and width wrapping past the raster",
        VideoWallWindow {
            pos_x: 65472,
            width: 128,
            ..good
        },
    );

    // The vertical origin and the height carry no alignment rule, so a client
    // generalising the two horizontal ones would refuse windows a sink takes.
    for window in [
        VideoWallWindow { pos_y: 101, ..good },
        VideoWallWindow {
            height: 201,
            ..good
        },
    ] {
        assert!(
            f.remote.store_video_wall(sink, window).is_ok(),
            "an unaligned vertical value was refused: {window}"
        );
    }

    // A window naming no raster is contained in nothing, and the sink skips the
    // containment check rather than refusing it - so refusing it here would
    // withhold a window the sink takes. Every other rule still applies to it,
    // which is what separates this from a cleared window.
    for window in [
        VideoWallWindow {
            raster_w: 0,
            ..good
        },
        VideoWallWindow {
            raster_h: 0,
            ..good
        },
        VideoWallWindow {
            raster_w: 0,
            raster_h: 0,
            ..good
        },
    ] {
        assert!(
            f.remote.store_video_wall(sink, window).is_ok(),
            "a window naming no raster was refused: {window}"
        );
    }
    refused(
        "an unaligned origin is still refused without a raster",
        VideoWallWindow {
            pos_x: 32,
            raster_w: 0,
            raster_h: 0,
            ..good
        },
    );

    // A revert is not geometry and is never refused for it.
    assert!(f.remote.revert_video_wall(sink).is_ok());
}

/// A volume command that names no mute state says so on the wire.
///
/// Zero is what a device is told to unmute by, so sending it for a caller who
/// asked only for a volume would unmute a bay they never mentioned. The byte
/// is read off the frame rather than from the type that built it: a builder
/// and a decoder that are wrong together agree with each other, and only the
/// wire says which of them is right.
#[test]
fn a_volume_with_no_mute_state_sends_the_unchanged_value() {
    let f = Fixture::new();
    let uid = uid_n(209);
    f.everything(uid, 0x28, "VU0001");
    f.connect();
    let bay = BayUid::new(uid, 2);

    // The mute byte sits after the uid, the u16 port and the two volumes.
    let mute_byte = |frame: &[u8]| frame[HEADER_LEN + 20];

    f.tap.clear();
    f.remote
        .set_volume(bay, 40, None)
        .expect("the socket is open and the device is above the floor");
    let frame = f.tap.frames().pop().expect("nothing reached the gate");
    assert_eq!(
        mute_byte(&frame),
        VOLUME_UNCHANGED,
        "a command naming no mute state told the device to unmute"
    );

    // Both named states, so the assertion above cannot be passing for an
    // encoder that writes the same byte whatever it is given.
    for (muted, want) in [(false, 0u8), (true, 3)] {
        f.tap.clear();
        f.remote
            .set_volume(bay, 40, Some(muted))
            .expect("the socket is open and the device is above the floor");
        let frame = f.tap.frames().pop().expect("nothing reached the gate");
        assert_eq!(mute_byte(&frame), want, "muted={muted}");
    }
}

/// A manual route names all three streams, and always carries a format.
///
/// The three groups are read back out of the registry rather than off the
/// frame: a group written into the wrong slot is a well-formed address at a
/// well-formed offset, which only the decode can tell apart. The format is the
/// separate half - firmware hands whatever this frame carries to the FPGA
/// unexamined, so a frame with no format leaves a zero sample rate there and
/// the switch dies with it.
#[test]
fn a_manual_route_names_three_streams_and_a_format() {
    let f = Fixture::new();
    let (sink, peer) = (uid_n(201), uid_n(202));
    f.everything(sink, 0x28, "MR0001");
    f.everything(peer, 0x28, "MR0002");

    f.tap.clear();
    f.round_trip(
        "select_source_addr",
        peer,
        f.remote
            .select_source_addr(BayUid::new(sink, 2), route(4), None),
    );

    // Filed under the device the payload names, not under whoever sent the
    // frame: this handler is one of the addressed-uid ones.
    let got = f
        .remote
        .v2ip_sink(sink)
        .expect("the route did not decode at all");
    for (what, addr, last, port) in [
        ("video", got.addresses.video, 1, V2IP_PORT_VIDEO),
        ("audio", got.addresses.audio, 2, V2IP_PORT_AUDIO),
        ("anc", got.addresses.anc, 3, V2IP_PORT_ANC),
    ] {
        assert_eq!(
            (what, addr.ip, addr.port),
            (what, Ipv4Addr::new(239, 11, 4, last), port),
            "an unset port must become the stream's standard one"
        );
    }
    let format = got
        .audio_fmt
        .expect("no format on the frame; the FPGA is handed zeroes");
    assert_eq!(
        (format.sample_rate, format.channels),
        (V2IP_AUDIO_DEFAULT_SAMPLE_RATE, V2IP_AUDIO_DEFAULT_CHANNELS)
    );
}

/// A route slot with no address carries no port either.
///
/// Firmware reads the pair together and takes 0.0.0.0 as "leave this stream
/// where it is", so a port beside it describes nothing. Sending one would also
/// hide the slot being empty from anything reading the frame back.
#[test]
fn an_unset_route_slot_carries_neither_address_nor_port() {
    let f = Fixture::new();
    let sink = uid_n(203);
    f.everything(sink, 0x28, "MR0003");

    f.tap.clear();
    let mut route = route(5);
    route.anc = V2ipRouteTarget::default();
    let _ = f
        .remote
        .select_source_addr(BayUid::new(sink, 2), route, None);
    let frame = f.tap.frames().pop().expect("nothing reached the gate");

    // The three stream slots start 16 bytes into the payload, eight bytes
    // apart: address, port, two pad.
    let anc = &frame[HEADER_LEN + 32..HEADER_LEN + 40];
    assert_eq!(anc, &[0; 8], "an empty slot must be empty entire");
}

/// A signal-status request addresses one unit or the whole network, and the
/// payload is the only thing that says which.
#[test]
fn a_signal_status_request_says_who_it_is_for() {
    let f = Fixture::new();
    let one = uid_n(204);
    f.everything(one, 0x28, "SS0003");
    f.connect();

    f.tap.clear();
    f.remote
        .request_signal_status(Some(one))
        .expect("the socket is open and the device is above the floor");
    let frame = f.tap.frames().pop().expect("nothing reached the gate");
    assert_eq!(
        &frame[HEADER_LEN..],
        one.as_bytes(),
        "a request for one unit carries its uid and nothing else"
    );

    f.tap.clear();
    f.remote
        .request_signal_status(None)
        .expect("a broadcast names no device to be refused by");
    let frame = f.tap.frames().pop().expect("nothing reached the gate");
    assert_eq!(
        frame.len(),
        HEADER_LEN,
        "an empty payload is what makes it a broadcast request"
    );
}

/// What the two request methods and the key send ask for, decoded back.
#[derive(Default)]
struct Asks {
    edid: Mutex<Vec<EdidRequest>>,
    keys: Mutex<Vec<KeyTransmitRequest>>,
}

impl EventHandler for Asks {
    fn on_edid_requested(&self, _device: DeviceUid, request: EdidRequest) {
        self.edid.lock().expect("test handler").push(request);
    }

    fn on_key_transmit_requested(&self, _device: DeviceUid, request: KeyTransmitRequest) {
        self.keys.lock().expect("test handler").push(request);
    }
}

#[test]
fn an_edid_request_and_a_key_decode_back_to_what_was_asked() {
    let seen = Arc::new(Asks::default());
    let f = Fixture::with_handler(Arc::clone(&seen) as Arc<dyn EventHandler>);
    let (target, peer) = (uid_n(205), uid_n(206));
    f.everything(target, 0x28, "AK0001");
    f.everything(peer, 0x28, "AK0002");

    f.tap.clear();
    f.round_trip("request_edid", peer, f.remote.request_edid(target, true));
    let ask = seen.edid.lock().expect("test handler").pop().expect("none");
    assert_eq!(ask.target, target);
    assert!(ask.output, "the flag is what picks display over source");

    // And the other way, so the assertion above is not passing on a constant.
    f.round_trip("request_edid", peer, f.remote.request_edid(target, false));
    let ask = seen.edid.lock().expect("test handler").pop().expect("none");
    assert!(!ask.output);

    f.round_trip(
        "send_key",
        peer,
        f.remote.send_key(BayUid::new(target, 2), RcKey::PLAY),
    );
    let key = seen.keys.lock().expect("test handler").pop().expect("none");
    assert_eq!(key.target, target);
    assert_eq!(key.local_bay, 2);
    assert_eq!(key.key, RcKey::PLAY);
}

/// An EDID a device reports is kept, so a caller reads it back rather than
/// having to hold on to one from a callback.
#[test]
fn a_reported_edid_is_kept_per_direction() {
    let f = Fixture::new();
    let unit = uid_n(207);
    f.everything(unit, 0x28, "ED0001");
    assert_eq!(f.remote.edid(unit, true), None);

    // One record per direction, each a flag byte and 256 bytes of EDID. The
    // two differ in every byte, so a reply filed under the wrong direction
    // cannot read as the right one.
    let mut reply = Vec::new();
    for (output, fill) in [(false, 0x11u8), (true, 0x22)] {
        reply.push(u8::from(output));
        reply.extend(std::iter::repeat(fill).take(256));
    }
    f.feed(unit, op::DEV_EDID, &reply);

    assert_eq!(f.remote.edid(unit, false), Some(vec![0x11; 256]));
    assert_eq!(f.remote.edid(unit, true), Some(vec![0x22; 256]));
}

/// The frame counter separates a quiet mesh from an interface nothing is on.
///
/// It counts frames from other senders only. This client's own multicast is
/// looped back by the host whichever interface was chosen, so counting that
/// would answer "did anything reach this interface" with yes on every one of
/// them - which is the single question the counter exists to answer.
#[test]
fn frames_from_peers_are_counted_and_this_clients_own_are_not() {
    let f = Fixture::new();
    assert_eq!(f.remote.frames_received(), 0);

    let peer = uid_n(208);
    f.everything(peer, 0x28, "FC0001");
    let after_peer = f.remote.frames_received();
    assert!(after_peer > 0, "nothing was counted for a peer's frames");

    // An opcode no handler claims still arrived, so it still counts.
    f.feed(peer, Opcode(0xFFFF), &[]);
    assert_eq!(
        f.remote.frames_received(),
        after_peer + 1,
        "a frame nothing decoded was not counted"
    );

    // This client's own uid, which is what its own loopback carries.
    f.feed(
        uid_n(CLIENT),
        op::SYS_HELLO,
        &hello_payload(
            0x28,
            "MXR Rust",
            "P9SN00000000",
            "4.8.0",
            DeviceFeature::MANAGER,
        ),
    );
    assert_eq!(
        f.remote.frames_received(),
        after_peer + 1,
        "this client's own frame was counted"
    );
}

/// A multiviewer frame is stamped at its own opcode's version, and reaches a
/// device reporting exactly that.
///
/// The module that receives this opcode dispatches on payload length and never
/// reads the stamp, so a stamp above the table entry enables nothing and costs
/// every receiver capped between 0x16 and 0x1F. Both halves are asserted: the
/// number on the wire, and the device that number decides the fate of. A test
/// on the stamp alone would pass just as well for a value that reaches nobody.
#[test]
fn a_multiviewer_frame_is_stamped_at_its_opcodes_own_version() {
    let f = Fixture::new();
    let uid = uid_n(196);
    f.everything(uid, 0x28, "MV0001");

    f.tap.clear();
    let _ = f
        .remote
        .set_multiviewer_view_mode(uid, MultiviewerViewMode::PIP);
    let frame = f.tap.frames().pop().expect("nothing reached the gate");

    let stamped = u16::from_le_bytes([frame[2], frame[3]]);
    assert_eq!(stamped, protocol_for(op::V2IP_MULTIVIEWER));
    assert_eq!(stamped, 0x16);
    assert_eq!(frame.len(), HEADER_LEN + 25);

    // A device reporting exactly the opcode's version is the one a raised
    // stamp would have shut out, so it is the input that makes this mean
    // something.
    let on_the_floor = uid_n(197);
    f.everything(on_the_floor, 0x16, "MV0002");
    assert!(!refused(&f.remote.set_multiviewer_view_mode(
        on_the_floor,
        MultiviewerViewMode::PIP
    )));

    // And the paired direction, so the allowance above is not one the gate
    // would give anything.
    let below = uid_n(198);
    f.everything(below, 0x15, "MV0003");
    assert!(refused(
        &f.remote
            .set_multiviewer_view_mode(below, MultiviewerViewMode::PIP)
    ));
}

/// A command's local write-back must wait until its frame is away.
///
/// Nothing acknowledges a command, so the write-back is this client's only
/// record that it asked for anything - and a refused frame asked for nothing.
/// Applying it up front would leave the registry describing a device that was
/// never told, until some later report from that device contradicted it.
#[test]
fn a_write_back_waits_for_the_send() {
    let f = Fixture::new();
    let (old, current) = (uid_n(198), uid_n(199));
    f.everything(old, 0x01, "WB0001");
    f.everything(current, 0x28, "WB0002");

    // CHANGE_BAY_NAME and BAY_HIDE both floor at 0x06, so this device is
    // refused both and its bay must read exactly as its config left it.
    let refused_bay = BayUid::new(old, 2);
    assert!(refused(&f.remote.set_bay_name(refused_bay, "Kitchen")));
    assert!(refused(&f.remote.set_bay_hidden(refused_bay, true)));
    let bay = f.remote.bay(refused_bay).expect("no bay");
    assert_eq!(
        bay.user_name, "TV",
        "a refused rename still renamed the bay"
    );
    assert_eq!(bay.hidden, Some(false), "a refused hide still hid the bay");

    // The same two calls where the frame does leave. Without this half the
    // assertions above would hold just as well for write-backs that never run
    // at all, which is a different library and not a correct one.
    f.connect();
    let sent_bay = BayUid::new(current, 2);
    f.remote
        .set_bay_name(sent_bay, "Kitchen")
        .expect("the socket is open and the device is above the floor");
    f.remote
        .set_bay_hidden(sent_bay, true)
        .expect("the socket is open and the device is above the floor");
    let bay = f.remote.bay(sent_bay).expect("no bay");
    assert_eq!(bay.user_name, "Kitchen", "the rename was not written back");
    assert_eq!(bay.hidden, Some(true), "the hide was not written back");
}

/// Which stream a source switch names, and which it leaves alone.
///
/// The frame carries a video group and an audio group in adjacent slots, and
/// each method fills one and zeroes the other - zero being how a sink is told
/// to keep the stream it has. Filling the wrong slot puts a well-formed address
/// at a well-formed offset, so only reading the route back out of the registry
/// says which stream was actually named.
///
/// A sink apiece, because the audio source a bay reports falls back to its
/// video source until it is given one of its own: on a bay that has been given
/// both, neither slot can be told from the other.
/// Routing video leaves the audio address empty so the audio follows, except
/// for a unit below protocol 0x27, which is told the source's audio stream
/// because some of those route no audio for an empty one.
#[test]
fn routing_video_names_the_audio_only_for_older_units() {
    let f = Fixture::new();
    f.connect();
    for (protocol, uid, serial, audio) in [
        (0x2F, uid_n(196), "SW0004", [0, 0, 0, 0]),
        (0x1E, uid_n(197), "SW0005", [239, 7, 197, 2]),
    ] {
        f.everything(uid, protocol, serial);
        f.tap.clear();
        f.remote
            .select_video_source(BayUid::new(uid, 2), 1)
            .expect("sent");
        let frame = f.tap.frames().pop().expect("nothing reached the gate");
        let p = &frame[HEADER_LEN..];
        assert_eq!(&p[16..20], &[239, 7, 197 - u8::from(protocol == 0x2F), 1]);
        assert_eq!(&p[20..24], &audio, "protocol {protocol:#04x}");
    }
}

#[test]
fn a_source_switch_names_one_stream_and_zeroes_the_other() {
    let f = Fixture::new();
    let (video, audio, peer) = (uid_n(193), uid_n(194), uid_n(195));
    f.everything(video, 0x28, "SW0001");
    f.everything(audio, 0x28, "SW0002");
    f.everything(peer, 0x28, "SW0003");

    f.tap.clear();
    f.round_trip(
        "select_video_source",
        peer,
        f.remote.select_video_source(BayUid::new(video, 2), 1),
    );
    assert_eq!(
        f.remote
            .bay(BayUid::new(video, 2))
            .expect("no bay")
            .video_source,
        Some(BayUid::new(video, 1)),
        "the video group did not reach the video slot"
    );

    f.round_trip(
        "select_audio_source",
        peer,
        f.remote.select_audio_source(BayUid::new(audio, 2), 1),
    );
    let bay = f.remote.bay(BayUid::new(audio, 2)).expect("no bay");
    assert_eq!(
        bay.audio_source,
        Some(BayUid::new(audio, 1)),
        "the audio group did not resolve back to the bay advertising it"
    );
    assert_eq!(
        bay.video_source, None,
        "the audio group reached the video slot as well"
    );
}

/// Records the endpoint routes the receive path decodes.
#[derive(Default)]
struct AudioSelects(Mutex<Vec<AudioChangeSource>>);

impl EventHandler for AudioSelects {
    fn on_audio_select_input(&self, _device: DeviceUid, change: AudioChangeSource) {
        self.0.lock().expect("test handler").push(change);
    }
}

/// Which uid in a `SELECT_INPUT` frame is the sink and which is the source.
///
/// The sink is named twice - as the command header's target and again at the
/// head of the body - so taking the body's second uid for the sink would make
/// one device both ends of the route, with every byte at a well-formed offset.
/// A round trip is what catches it, and only with two distinct uids and two
/// distinct endpoint ids: equal ones would agree whichever way round they were
/// read.
#[test]
fn an_endpoint_selection_tells_its_source_from_its_sink() {
    let seen = Arc::new(AudioSelects::default());
    let f = Fixture::with_handler(Arc::clone(&seen) as Arc<dyn EventHandler>);
    let (sink, source, peer) = (uid_n(186), uid_n(187), uid_n(188));
    f.everything(sink, 0x28, "EP0001");
    f.everything(source, 0x28, "EP0002");
    // Management, as a unit takes an endpoint selection from no one else.
    f.feed(
        peer,
        op::SYS_HELLO,
        &hello_payload(0x28, "Ctrl", "EP0003", "4.8.0", DeviceFeature::MANAGER),
    );

    f.tap.clear();
    f.round_trip(
        "select_audio_endpoint_input",
        peer,
        f.remote.select_audio_endpoint_input(sink, 5, source, 9),
    );

    let change = seen
        .0
        .lock()
        .expect("test handler")
        .pop()
        .expect("what the builder produced did not decode at all");
    assert_eq!(change.target_uid, sink, "the sink is not the listening end");
    assert_eq!(
        change.source_uid, source,
        "the source is not the end being listened to"
    );
    assert_eq!((change.target_id, change.source_id), (5, 9));
}

/// The sub-opcode byte and the parameters behind it, for one command of each
/// shape the multiviewer has.
///
/// Fifteen of the sixteen sub-commands are requests that nothing decodes, so
/// what they change comes back only on the next status report. That leaves the
/// frame itself as the only statement of their layout, and a sub-opcode off by
/// one turns a command into its neighbour with every byte still plausible.
#[test]
fn a_multiviewer_command_carries_its_sub_opcode_and_parameters() {
    let f = Fixture::new();
    let uid = uid_n(197);
    f.everything(uid, 0x28, "MV0002");
    // Four windows, so window 1 is one this multiviewer is showing.
    f.multiviewer_showing(uid, 5);

    let mapped = uid_n(60);
    let mut config_source = mapped.as_bytes().to_vec();
    config_source.push(3);
    config_source.extend_from_slice(&[0; 7]);

    // The sub-opcodes are written out rather than read from `mv_sub`, which is
    // the table under test: comparing it against itself would hold for any
    // value in it.
    //
    // Every byte a window or a source is named by is written here as the
    // number the wire carries, so this holds the encoder to the firmware's
    // numbering rather than to this library's. The wire counts both from zero,
    // while `MultiviewerSource` counts inputs from one; input 2 is therefore
    // the byte 1, and window 1 is the second window.
    type Call = fn(&Remote, DeviceUid) -> Result<(), ControlError>;
    let calls: Vec<(&str, u8, Vec<u8>, Call)> = vec![
        (
            "view_mode",
            1,
            vec![MultiviewerViewMode::PIP.to_wire()],
            |r, d| r.set_multiviewer_view_mode(d, MultiviewerViewMode::PIP),
        ),
        ("video_source", 2, vec![1, 3], |r, d| {
            // Input 4 is the one a one-based encoding cannot reach: the
            // firmware's validator stops at 3.
            r.set_multiviewer_video_source(d, 1, MultiviewerSource::INPUT_4)
        }),
        ("audio_source", 3, vec![1], |r, d| {
            r.set_multiviewer_audio_source(d, MultiviewerSource::INPUT_2)
        }),
        ("audio_volume", 4, vec![70, 1], |r, d| {
            r.set_multiviewer_audio_volume(d, 70, true)
        }),
        ("remote_control", 6, vec![1], |r, d| {
            r.set_multiviewer_remote_control(d, MultiviewerSource::INPUT_2)
        }),
        ("input_source", 14, config_source, |r, d| {
            r.set_multiviewer_input_source(d, 3, uid_n(60))
        }),
        ("auto_route", 15, vec![], |r, d| r.multiviewer_auto_route(d)),
    ];

    for (name, sub, args, call) in calls {
        f.tap.clear();
        let _ = call(&f.remote, uid);
        let frame = f
            .tap
            .frames()
            .pop()
            .unwrap_or_else(|| panic!("{name} reached no frame"));
        let payload = frame
            .get(HEADER_LEN..)
            .expect("a frame shorter than a header");
        assert_eq!(&payload[..16], &uid.as_bytes()[..], "{name}: wrong target");
        assert_eq!(payload[16], sub, "{name}: wrong sub-opcode");
        assert_eq!(
            &payload[17..24],
            &[0; 7],
            "{name}: the padding carries data"
        );
        assert_eq!(&payload[24..], &args[..], "{name}: wrong parameters");
    }
}

/// A OneIP output whose volume control the mesh put on an amplifier zone.
///
/// The link is the only thing tying the two together: the output carries
/// `HDMI_OUT` and no audio feature, so nothing about the bay itself says where
/// its volume lives.
fn linked_to_amplifier() -> (Fixture, BayUid, BayUid) {
    let f = Fixture::new();
    let amp = uid_n(0x40);
    let oneip = uid_n(0x41);
    f.amplifier(amp, PROTOCOL_VERSION, "AMP00001");
    f.feed(
        oneip,
        op::SYS_HELLO,
        &hello_payload(
            PROTOCOL_VERSION,
            "OneIP",
            "ONE00001",
            "4.8.0",
            DeviceFeature::VIDEO_ROUTING,
        ),
    );
    f.feed(
        oneip,
        op::SYS_BAY_CONFIG,
        &bay_config_rec(
            1,
            1,
            0,
            "Output 1",
            "Living room",
            BayStatus::NONE,
            BayFeatures::HDMI_OUT,
        ),
    );
    let mut record = vec![0u8; 38];
    record[0] = 1;
    record[2..10].copy_from_slice(b"AMP00001");
    record[18..24].copy_from_slice(b"Zone 1");
    f.feed(oneip, op::SYS_LINKS, &record);
    f.tap.clear();
    (f, BayUid::new(oneip, 1), BayUid::new(amp, 1))
}

#[test]
fn a_linked_output_reads_the_volume_of_the_zone_it_is_wired_to() {
    let (f, output, zone) = linked_to_amplifier();

    // Only the amplifier reports a volume: the mesh gives the output no
    // volume of its own, which is the whole reason the link is configured.
    f.feed(zone.device, op::AUDIO_VOLUME_MUTE, &[1, 51, 51, 0]);

    let info = f.remote.bay(output).expect("the output vanished");
    assert_eq!(
        info.linked_bay,
        Some(zone),
        "the output does not name the zone it is linked to"
    );
    assert_eq!(
        info.volume.map(|v| v.volume()),
        Some(51),
        "the output reads no volume, so the link was not followed"
    );
    assert_eq!(
        f.remote
            .bay(zone)
            .and_then(|b| b.volume)
            .map(|v| v.volume()),
        Some(51),
        "the zone lost its own volume"
    );
}

#[test]
fn setting_a_linked_output_addresses_the_zone_it_is_wired_to() {
    let (f, output, zone) = linked_to_amplifier();
    f.feed(zone.device, op::AUDIO_VOLUME_MUTE, &[1, 51, 51, 0]);
    f.tap.clear();

    let call = f.remote.set_volume(output, 40, Some(false));
    assert!(
        matches!(call, Err(ControlError::Send(SendError::NotConnected))),
        "setting the linked output was refused rather than sent: {call:?}"
    );

    let frame = f.tap.frames().pop().expect("no frame was built");
    let payload = frame
        .get(HEADER_LEN..)
        .expect("a frame shorter than a header");
    assert_eq!(
        &payload[..16],
        &zone.device.as_bytes()[..],
        "the frame names the output's device, not the amplifier"
    );
    assert_eq!(
        u16::from_le_bytes([payload[16], payload[17]]),
        zone.port,
        "the frame names the output's port, not the zone's"
    );
}

#[test]
fn stepping_a_linked_output_starts_from_the_zone_volume() {
    let (f, output, zone) = linked_to_amplifier();
    f.feed(zone.device, op::AUDIO_VOLUME_MUTE, &[1, 51, 51, 0]);
    f.tap.clear();

    // Without the link the step has no volume to start from and fails before
    // building anything, so the frame is what proves the zone was read.
    let _ = f.remote.volume_up(output);
    let frame = f.tap.frames().pop().expect("no frame was built");
    let payload = frame
        .get(HEADER_LEN..)
        .expect("a frame shorter than a header");
    assert_eq!(payload[18], 52, "the step did not start from the zone's 51");
    // It keeps the mute state the zone reported rather than leaving it to
    // "unchanged", which an older matrix reads as mute.
    assert_eq!(payload[20], 0, "the step did not carry the zone's mute");
    f.feed(zone.device, op::AUDIO_VOLUME_MUTE, &[1, 51, 51, 3]);
    f.tap.clear();
    let _ = f.remote.volume_down(output);
    let frame = f.tap.frames().pop().expect("no frame was built");
    assert_eq!(frame[HEADER_LEN + 20], 3);
}

/// A OneIP output takes a volume: it carries a digital audio output.
#[test]
fn a_oneip_output_takes_a_volume() {
    let f = Fixture::new();
    let uid = uid_n(214);
    f.feed(
        uid,
        op::SYS_HELLO,
        &hello_payload(0x2F, "OneIP", "VO0001", "4.8.0", DeviceFeature::V2IP_SINK),
    );
    f.feed(
        uid,
        op::SYS_BAY_CONFIG,
        &bay_config_rec(
            127,
            1,
            0,
            "Output 1",
            "TV",
            BayStatus::NONE,
            BayFeatures::from_bits(0x10605),
        ),
    );
    assert!(matches!(
        f.remote.set_volume(BayUid::new(uid, 127), 40, None),
        Err(ControlError::Send(SendError::NotConnected))
    ));
}

#[test]
fn a_bay_with_its_own_volume_control_is_not_redirected() {
    let (f, _, zone) = linked_to_amplifier();
    f.feed(zone.device, op::AUDIO_VOLUME_MUTE, &[1, 51, 51, 0]);
    f.tap.clear();

    // The amplifier zone names the output back, so a link exists in both
    // directions. A bay that has volume control must still keep its own.
    let call = f.remote.set_volume(zone, 40, Some(false));
    assert!(
        matches!(call, Err(ControlError::Send(SendError::NotConnected))),
        "setting the zone was refused: {call:?}"
    );
    let frame = f.tap.frames().pop().expect("no frame was built");
    let payload = frame
        .get(HEADER_LEN..)
        .expect("a frame shorter than a header");
    assert_eq!(
        &payload[..16],
        &zone.device.as_bytes()[..],
        "the zone's own volume was redirected somewhere else"
    );
}

/// The Dolby input a bay's feature word names, in bits 24-31.
///
/// `MXR_BAY_FEATURE_DOLBY` means the output is one of a Dolby group's zones
/// and those bits say which input drives it, so a bay carrying the bit without
/// them would not be what a device reports.
fn dolby_output(input_bay: u8) -> BayFeatures {
    BayFeatures::from_bits(
        BayFeatures::AUDIO_AMP_OUT.bits()
            | BayFeatures::DOLBY.bits()
            | (u32::from(input_bay) << 24),
    )
}

/// A ProAmp8 in `mode`, whose outputs are four Dolby zones, one plain zone,
/// and a ninth output that carries the Dolby bit from outside the group.
///
/// That last one is the whole point: the feature bit does not describe the
/// group, so a mirror driven off the bit alone would reach a bay the
/// amplifier does not drive with the others.
fn dolby_amplifier(mode: u8) -> (Fixture, DeviceUid) {
    let f = Fixture::new();
    let amp = uid_n(0x50);
    f.feed(
        amp,
        op::SYS_HELLO,
        &hello_payload(
            PROTOCOL_VERSION,
            "ProAmp8",
            "AMP00002",
            "4.8.0",
            DeviceFeature::AUDIO_AMPLIFIER
                | DeviceFeature::VOLUME_CONTROL
                | DeviceFeature::AUDIO_ROUTING,
        ),
    );
    let mut bays = Vec::new();
    for zone in 0..4u8 {
        bays.extend(bay_config_rec(
            9 + zone,
            1,
            zone,
            &format!("Output {}", zone + 1),
            "Dolby zone",
            BayStatus::NONE,
            dolby_output(8),
        ));
    }
    bays.extend(bay_config_rec(
        13,
        1,
        4,
        "Output 5",
        "Kitchen",
        BayStatus::NONE,
        BayFeatures::AUDIO_AMP_OUT,
    ));
    bays.extend(bay_config_rec(
        17,
        1,
        8,
        "Output 9",
        "Line out",
        BayStatus::NONE,
        dolby_output(8),
    ));
    f.feed(amp, op::SYS_BAY_CONFIG, &bays);

    let mut dolby = vec![0u8; 24];
    dolby[..16].copy_from_slice(amp.as_bytes());
    dolby[16] = mode;
    f.feed(amp, op::AMP_DOLBY_STATE, &dolby);
    f.tap.clear();
    (f, amp)
}

/// The volume each output reads, by port, after the amplifier reported one
/// against the group's first zone.
fn zone_volumes(f: &Fixture, amp: DeviceUid, ports: &[u16]) -> Vec<Option<u8>> {
    ports
        .iter()
        .map(|port| {
            f.remote
                .bay(BayUid::new(amp, *port))
                .and_then(|b| b.volume)
                .map(|v| v.volume())
        })
        .collect()
}

#[test]
fn a_four_zone_dolby_group_all_read_the_volume_reported_for_its_first_zone() {
    let (f, amp) = dolby_amplifier(2);
    f.feed(amp, op::AUDIO_VOLUME_MUTE, &[9, 51, 51, 0]);

    assert_eq!(
        zone_volumes(&f, amp, &[9, 10, 11, 12]),
        vec![Some(51); 4],
        "the four zones do not share the volume the amplifier reported"
    );
    assert_eq!(
        zone_volumes(&f, amp, &[13, 17]),
        vec![None, None],
        "the volume reached an output outside the group"
    );
}

#[test]
fn a_three_zone_dolby_group_stops_at_its_third_zone() {
    let (f, amp) = dolby_amplifier(1);
    f.feed(amp, op::AUDIO_VOLUME_MUTE, &[9, 51, 51, 0]);

    assert_eq!(
        zone_volumes(&f, amp, &[9, 10, 11]),
        vec![Some(51); 3],
        "the three zones do not share the volume the amplifier reported"
    );
    assert_eq!(
        zone_volumes(&f, amp, &[12, 13, 17]),
        vec![None, None, None],
        "the fourth output is not in a three-zone group"
    );
}

#[test]
fn an_amplifier_out_of_dolby_mode_groups_nothing() {
    let (f, amp) = dolby_amplifier(0);
    f.feed(amp, op::AUDIO_VOLUME_MUTE, &[9, 51, 51, 0]);

    assert_eq!(
        zone_volumes(&f, amp, &[9, 10, 11, 12]),
        vec![Some(51), None, None, None],
        "a standard-mode amplifier drives its outputs separately"
    );
}

#[test]
fn a_volume_against_a_later_zone_is_not_spread_over_the_group() {
    let (f, amp) = dolby_amplifier(2);
    // Only the group's first zone stands for the group. A report against any
    // other is that output's own, so spreading it would overwrite the rest
    // with a value the amplifier said nothing about.
    f.feed(amp, op::AUDIO_VOLUME_MUTE, &[11, 42, 42, 0]);

    assert_eq!(
        zone_volumes(&f, amp, &[9, 10, 11, 12]),
        vec![None, None, Some(42), None],
        "a report against a later zone was spread over the group"
    );
}

/// A window index the multiviewer is not showing never reaches the wire.
///
/// Firmware accepts an index one past the last window and then indexes its
/// window array with it, so the frame carrying one corrupts state on the
/// receiving device. That makes this the one multiviewer frame this library
/// must refuse to build rather than merely discourage.
#[test]
fn a_window_the_multiviewer_is_not_showing_is_refused() {
    let f = Fixture::new();
    let uid = uid_n(150);
    f.everything(uid, 0x28, "MV0010");
    f.connect();

    let sent = |screen: u8| {
        f.tap.clear();
        let got = f
            .remote
            .set_multiviewer_video_source(uid, screen, MultiviewerSource::INPUT_1);
        assert!(
            !matches!(got, Err(ControlError::InvalidRequest(_))),
            "window {screen}: {got:?}"
        );
        assert_eq!(f.tap.frames().len(), 1, "window {screen} reached no frame");
    };
    let refused = |screen: u8, what: &str| {
        f.tap.clear();
        let got = f
            .remote
            .set_multiviewer_video_source(uid, screen, MultiviewerSource::INPUT_1);
        assert!(
            matches!(got, Err(ControlError::InvalidRequest(_))),
            "{what}: {got:?}"
        );
        assert!(f.tap.frames().is_empty(), "{what} reached the wire");
    };

    // Before any status report the layout is unknown, and window zero is the
    // one window every layout has.
    sent(0);
    refused(
        1,
        "a second window on a multiviewer that has reported no layout",
    );

    // Two windows: the second is addressable and the third is not. The index
    // equal to the count is the value firmware mishandles, so it is the one
    // this has to catch.
    f.multiviewer_showing(uid, 3);
    sent(1);
    refused(2, "the window one past a two-window layout");

    // Four windows, where the same index is now legitimate: the bound has to
    // follow the layout rather than being a constant that happens to fit.
    f.multiviewer_showing(uid, 5);
    sent(3);
    refused(4, "the window one past a four-window layout");
}

/// A source that names no input is refused rather than sent as input 1.
///
/// The wire counts inputs from zero, so the value a source naming nothing
/// would encode to is the first input. Passing it through would turn "I do not
/// know" into a switch the caller never asked for.
#[test]
fn a_source_naming_no_input_is_refused_rather_than_selecting_the_first() {
    let f = Fixture::new();
    let uid = uid_n(151);
    f.everything(uid, 0x28, "MV0011");
    f.connect();

    type Call = fn(&Remote, DeviceUid, MultiviewerSource) -> Result<(), ControlError>;
    let calls: [(&str, Call); 3] = [
        ("audio_source", |r, d, s| {
            r.set_multiviewer_audio_source(d, s)
        }),
        ("remote_control", |r, d, s| {
            r.set_multiviewer_remote_control(d, s)
        }),
        ("video_source", |r, d, s| {
            r.set_multiviewer_video_source(d, 0, s)
        }),
    ];

    for (name, call) in calls {
        f.tap.clear();
        assert!(
            call(&f.remote, uid, MultiviewerSource::INPUT_1).is_ok(),
            "{name} refused the input this case is measured against"
        );
        assert_eq!(f.tap.frames().len(), 1, "{name} reached no frame");

        f.tap.clear();
        let got = call(&f.remote, uid, MultiviewerSource::UNKNOWN);
        assert!(
            matches!(got, Err(ControlError::InvalidRequest(_))),
            "{name} accepted a source naming no input: {got:?}"
        );
        assert!(f.tap.frames().is_empty(), "{name} reached the wire");
    }
}

/// A setting outside the range firmware accepts is refused here.
///
/// The device drops one without answering, so a caller would otherwise see a
/// send succeed and the setting stay as it was. Each case is paired with the
/// highest value that is accepted, so a check that refused everything would
/// fail rather than look thorough.
#[test]
fn a_setting_the_multiviewer_would_drop_never_reaches_the_wire() {
    let f = Fixture::new();
    let uid = uid_n(152);
    f.everything(uid, 0x28, "MV0012");
    f.connect();

    type Call = fn(&Remote, DeviceUid, u8) -> Result<(), ControlError>;
    let calls: [(&str, u8, Call); 9] = [
        ("view_mode", 8, |r, d, v| {
            r.set_multiviewer_view_mode(d, MultiviewerViewMode::from_wire(v))
        }),
        ("edid_template", 19, |r, d, v| {
            r.set_multiviewer_edid_template(d, MultiviewerEdidTemplate::from_wire(v))
        }),
        ("pip_size", 3, |r, d, v| {
            r.set_multiviewer_pip_size(d, MultiviewerPipSize::from_wire(v))
        }),
        ("pip_position", 4, |r, d, v| {
            r.set_multiviewer_pip_position(d, MultiviewerPipPosition::from_wire(v))
        }),
        ("aspect", 2, |r, d, v| {
            r.set_multiviewer_aspect_ratio(d, MultiviewerAspectRatio::from_wire(v))
        }),
        ("output_mode", 14, |r, d, v| {
            r.set_multiviewer_output_mode(d, MultiviewerOutputMode::from_wire(v))
        }),
        ("output_itc", 2, |r, d, v| {
            r.set_multiviewer_output_itc(d, MultiviewerItcMode::from_wire(v))
        }),
        // Three is HDCP off, which a range stopping at the two HDCP versions
        // would refuse.
        ("hdcp_mode", 3, |r, d, v| {
            r.set_multiviewer_hdcp_mode(d, MultiviewerHdcpMode::from_wire(v))
        }),
        ("audio_volume", 100, |r, d, v| {
            r.set_multiviewer_audio_volume(d, v, false)
        }),
    ];

    for (name, highest, call) in calls {
        f.tap.clear();
        assert!(
            call(&f.remote, uid, highest).is_ok(),
            "{name} refused {highest}, the highest value firmware accepts"
        );
        assert_eq!(f.tap.frames().len(), 1, "{name} reached no frame");

        f.tap.clear();
        let got = call(&f.remote, uid, highest + 1);
        assert!(
            matches!(got, Err(ControlError::InvalidRequest(_))),
            "{name} accepted {}: {got:?}",
            highest + 1
        );
        assert!(f.tap.frames().is_empty(), "{name} reached the wire");
    }

    // Zero is what every one of these settings reads as "the device has
    // reported nothing", and it is not a value to send either.
    assert!(matches!(
        f.remote
            .set_multiviewer_view_mode(uid, MultiviewerViewMode::UNKNOWN),
        Err(ControlError::InvalidRequest(_))
    ));

    // The input index the mapping command carries is bounded by the array it
    // indexes rather than by a layout.
    assert!(f
        .remote
        .set_multiviewer_input_source(uid, 3, uid_n(60))
        .is_ok());
    assert!(matches!(
        f.remote.set_multiviewer_input_source(uid, 4, uid_n(60)),
        Err(ControlError::InvalidRequest(_))
    ));
}

/// A scaling write carries nothing but the scaling block.
///
/// The receiver hands this frame's source addresses to its encoder on every
/// frame it applies, not only on the ones that carry addresses, and what stops
/// a scaling write from repointing the encoder is that a zero video address is
/// not multicast. The rate beside them is dropped for being out of range. So
/// the zeros and the `0xFF` are the assertion: a builder that filled either in
/// would move a transceiver's stream or reset its rate as a side effect of
/// turning scaling off.
/// A released unit, below protocol 0x2F, drops a scaling write from
/// management, so none is sent to one.
#[test]
fn a_scaling_write_to_a_released_unit_is_refused() {
    let f = Fixture::new();
    let sink = uid_n(219);
    f.everything(sink, 0x27, "SC0000");
    f.connect();
    f.tap.clear();
    for call in [
        f.remote.set_v2ip_auto_scaling(sink, true),
        f.remote.clear_v2ip_output_mode(sink),
    ] {
        assert!(
            matches!(call, Err(ControlError::Unsupported(_))),
            "{call:?}"
        );
    }
    assert!(f.tap.frames().is_empty());
}

#[test]
fn a_scaling_write_carries_nothing_but_the_scaling_block() {
    let f = Fixture::new();
    let sink = uid_n(220);
    f.everything(sink, 0x2F, "SC0001");
    f.connect();

    f.tap.clear();
    f.remote
        .set_v2ip_auto_scaling(sink, true)
        .expect("the socket is open and the device is above the floor");
    let frame = f.tap.frames().pop().expect("nothing reached the gate");
    let p = &frame[HEADER_LEN..];

    assert_eq!(p.len(), 88, "the payload is not the size the sink requires");
    assert_eq!(u16::from_le_bytes([frame[22], frame[23]]), 88);
    assert_eq!(&p[..16], sink.as_bytes(), "the frame names another device");
    assert!(
        p[16..40].iter().all(|b| *b == 0),
        "a source address would repoint the encoder"
    );
    assert!(
        !(V2IP_SOURCE_RATE_MIN..=V2IP_SOURCE_RATE_MAX).contains(&p[40]),
        "the rate is inside the valid range, so it would replace the sink's"
    );
    assert!(
        p[41..44].iter().all(|b| *b & V2IP_DSCP_SET == 0),
        "a marking would be applied"
    );
    assert!(
        p[64..80].iter().all(|b| *b == 0),
        "a non-zero tiling uid would carry a window"
    );
    assert_eq!(
        p[60],
        SCALING_FLAG_OPTIONS_VALID | SCALING_FLAG_AUTO_SCALING
    );
    assert_eq!(
        p[60] & SCALING_FLAG_MODE_VALID,
        0,
        "an options write claimed to carry a mode"
    );
}

/// Clearing a mode is spelled one way on a write and the other way in a
/// report, and the cache has to hold the report's spelling.
///
/// A write clears a configured mode by setting the valid bit over a zero
/// descriptor; a sink with no mode configured reports the valid bit clear and
/// never sets it over a zero mode. Caching what was sent would leave a caller
/// reading back a state no device ever broadcasts - a mode configured to
/// descriptor zero.
#[test]
fn clearing_a_mode_is_written_set_and_cached_clear() {
    let f = Fixture::new();
    let sink = uid_n(221);
    f.everything(sink, 0x2F, "SC0002");
    f.connect();

    f.remote
        .set_v2ip_output_mode(
            sink,
            V2ipOutputMode {
                svd: 16,
                depth: 12,
                colour: V2ipColourSpace::YCBCR422,
                refresh: 60,
            },
        )
        .expect("the mode is one a sink accepts");
    let cached = f
        .remote
        .v2ip_details(sink)
        .expect("the write back left no configuration");
    assert_eq!(
        cached.scaling.configured_mode().map(|(m, r)| (m.svd(), r)),
        Some((16, 60)),
        "the mode that was set did not reach the cache"
    );

    f.tap.clear();
    f.remote
        .clear_v2ip_output_mode(sink)
        .expect("the socket is open and the device is above the floor");
    let frame = f.tap.frames().pop().expect("nothing reached the gate");
    let p = &frame[HEADER_LEN..];

    assert_eq!(
        p[60] & SCALING_FLAG_MODE_VALID,
        SCALING_FLAG_MODE_VALID,
        "the clear did not claim to carry a mode, so the sink ignores it"
    );
    assert_eq!(
        u16::from_le_bytes([p[56], p[57]]),
        0,
        "the descriptor is not zero, so this sets a mode rather than clearing one"
    );

    let cached = f
        .remote
        .v2ip_details(sink)
        .expect("the write back left no configuration");
    assert_eq!(
        cached.scaling.configured_mode(),
        None,
        "the cache holds a mode the sink will not report"
    );
}

/// Turning automatic scaling on leaves a configured mode where it is.
///
/// The two are separate reasons for a sink to scale and each is written behind
/// its own validity bit, so an options write that claimed the mode half would
/// overwrite the mode with whatever it happened to carry.
#[test]
fn an_auto_scaling_write_leaves_a_configured_mode_alone() {
    let f = Fixture::new();
    let sink = uid_n(222);
    f.everything(sink, 0x2F, "SC0003");
    f.connect();

    f.remote
        .set_v2ip_output_mode(
            sink,
            V2ipOutputMode {
                svd: 4,
                depth: 8,
                colour: V2ipColourSpace::RGB,
                refresh: 60,
            },
        )
        .expect("the mode is one a sink accepts");
    f.remote
        .set_v2ip_auto_scaling(sink, false)
        .expect("the socket is open and the device is above the floor");

    let cached = f
        .remote
        .v2ip_details(sink)
        .expect("the write back left no configuration");
    assert_eq!(
        cached.scaling.configured_mode().map(|(m, _)| m.svd()),
        Some(4),
        "turning automatic scaling off dropped the configured mode"
    );
    assert_eq!(cached.scaling.auto_scaling(), Some(false));

    f.remote
        .set_v2ip_auto_scaling(sink, true)
        .expect("the socket is open and the device is above the floor");
    let cached = f
        .remote
        .v2ip_details(sink)
        .expect("the write back left no configuration");
    assert_eq!(cached.scaling.auto_scaling(), Some(true));
    assert_eq!(
        cached.scaling.configured_mode().map(|(m, _)| m.svd()),
        Some(4)
    );
}

/// Has `uid` report the device settings in `valid`, all of them off, in a
/// frame long enough to hold the PTP message block. The VLAN block and PTP
/// state ahead of that block are zero, so they carry nothing.
fn report_settings(f: &Fixture, uid: DeviceUid, valid: V2ipDeviceSetting) {
    let cfg = Cfg::addresses(uid, "239.1.2.3");
    let mut p = cfg.bytes_with_power_save(valid.bits(), 0, 0, &V2ipPowerSaveSchedule::default());
    p.resize(224, 0);
    f.feed(uid, op::V2IP_DEVICE_CFG, &p);
}

/// A settings write carries nothing but the settings block.
///
/// Everything ahead of the block is a field a receiver would otherwise apply:
/// a source address repoints the encoder, a rate in range replaces the
/// device's, and a scaling validity bit rewrites its scaling. The zeros and
/// the out-of-range rate are the assertion.
#[test]
fn a_settings_write_carries_nothing_but_the_settings_block() {
    let f = Fixture::new();
    let uid = uid_n(223);
    f.everything(uid, 0x28, "ST0001");
    report_settings(&f, uid, V2ipDeviceSetting::SWITCHES);
    f.connect();

    f.tap.clear();
    f.remote
        .set_v2ip_device_setting(uid, V2ipDeviceSetting::STATUS_LED, true)
        .expect("the device has reported the setting");
    let frame = f.tap.frames().pop().expect("nothing reached the gate");
    let p = &frame[HEADER_LEN..];

    assert_eq!(p.len(), 176, "the payload stops short of the settings");
    assert_eq!(&p[..16], uid.as_bytes(), "the frame names another device");
    assert!(
        p[16..40].iter().all(|b| *b == 0),
        "a source address would repoint the encoder"
    );
    assert!(
        !(V2IP_SOURCE_RATE_MIN..=V2IP_SOURCE_RATE_MAX).contains(&p[40]),
        "the rate is inside the valid range, so it would replace the device's"
    );
    assert!(
        p[41..128].iter().all(|b| *b == 0),
        "a field between the rate and the settings block is set"
    );
    assert_eq!(
        &p[128..144],
        &[8, 0, 0, 0, 8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
        "the settings block"
    );
    assert!(
        p[144..].iter().all(|b| *b == 0),
        "the schedule and reserved bytes"
    );

    let cached = f.remote.v2ip_device_settings(uid).expect("no settings");
    assert_eq!(
        cached.get(V2ipDeviceSetting::STATUS_LED),
        Some(true),
        "reading back before the device reports shows the old value"
    );
    assert_eq!(cached.get(V2ipDeviceSetting::FAN_QUIET), Some(false));
}

/// A write the device would ignore, refused before it is sent.
#[test]
fn a_settings_write_the_device_would_ignore_is_refused() {
    let f = Fixture::new();
    let uid = uid_n(224);
    f.everything(uid, 0x28, "ST0002");
    f.connect();

    assert!(
        matches!(
            f.remote
                .set_v2ip_device_setting(uid, V2ipDeviceSetting::STATUS_LED, true),
            Err(ControlError::NotReported(_))
        ),
        "a device that has reported no settings"
    );

    report_settings(
        &f,
        uid,
        V2ipDeviceSetting::STATUS_LED | V2ipDeviceSetting::IR_PROFILE_SINK,
    );
    assert!(
        matches!(
            f.remote
                .set_v2ip_device_setting(uid, V2ipDeviceSetting::FAN_QUIET, true),
            Err(ControlError::Unsupported(_))
        ),
        "a setting the device does not have"
    );
    assert!(
        matches!(
            f.remote.set_v2ip_ir_profile(uid, 1),
            Err(ControlError::Unsupported(_))
        ),
        "a port the device does not have"
    );
    for setting in [
        V2ipDeviceSetting::NONE,
        V2ipDeviceSetting::IR_PROFILE_SINK,
        V2ipDeviceSetting::STATUS_LED | V2ipDeviceSetting::IR_PROFILES,
    ] {
        assert!(
            matches!(
                f.remote.set_v2ip_device_setting(uid, setting, true),
                Err(ControlError::InvalidRequest(_))
            ),
            "{setting} is not a set of on/off settings"
        );
    }
    for profile in [-1, V2IP_IR_PROFILE_MAX] {
        assert!(
            matches!(
                f.remote.set_v2ip_ir_profile(uid, profile),
                Err(ControlError::InvalidRequest(_))
            ),
            "global profile {profile}"
        );
    }
    for profile in [V2IP_IR_PROFILE_NOT_SET - 1, V2IP_IR_PROFILE_MAX] {
        assert!(
            matches!(
                f.remote.set_v2ip_sink_ir_profile(uid, profile),
                Err(ControlError::InvalidRequest(_))
            ),
            "output profile {profile}"
        );
    }
    f.remote
        .set_v2ip_sink_ir_profile(uid, V2IP_IR_PROFILE_NOT_SET)
        .expect("following the global profile is a profile");
    assert_eq!(
        f.remote
            .v2ip_device_settings(uid)
            .and_then(|s| s.ir_profile_sink()),
        Some(V2IP_IR_PROFILE_NOT_SET)
    );
}

/// What the builder writes, the decoder reads back as the same settings.
#[test]
fn a_built_settings_frame_decodes_to_what_was_built() {
    let f = Fixture::new();
    let uid = uid_n(225);
    f.everything(uid, 0x28, "ST0003");
    let has = V2ipDeviceSetting::SWITCHES
        | V2ipDeviceSetting::IR_PROFILE
        | V2ipDeviceSetting::IR_PROFILE_SINK;
    report_settings(&f, uid, has);
    f.tap.clear();

    f.round_trip(
        "the global profile",
        uid,
        f.remote.set_v2ip_ir_profile(uid, 7),
    );
    f.round_trip(
        "the output profile",
        uid,
        f.remote
            .set_v2ip_sink_ir_profile(uid, V2IP_IR_PROFILE_NOT_SET),
    );
    f.round_trip(
        "two switches",
        uid,
        f.remote.set_v2ip_device_setting(
            uid,
            V2ipDeviceSetting::SINK_OFF_NO_SIGNAL | V2ipDeviceSetting::CEC_COMBO_KEYS,
            true,
        ),
    );

    let s = f.remote.v2ip_device_settings(uid).expect("no settings");
    assert_eq!(s.ir_profile(), Some(7));
    assert_eq!(s.ir_profile_sink(), Some(V2IP_IR_PROFILE_NOT_SET));
    assert_eq!(s.get(V2ipDeviceSetting::SINK_OFF_NO_SIGNAL), Some(true));
    assert_eq!(s.get(V2ipDeviceSetting::CEC_COMBO_KEYS), Some(true));
    assert_eq!(s.get(V2ipDeviceSetting::SINK_CHECK_POWER), Some(false));
    assert_eq!(s.valid, has, "the round trip changed what the device has");
}

/// Every mode a sink refuses, refused here instead.
///
/// A sink drops each of these without answering, so a caller that sent one
/// would see the send succeed and the setting stay where it was.
#[test]
fn an_output_mode_a_sink_would_drop_is_refused_before_it_is_sent() {
    let f = Fixture::new();
    let sink = uid_n(223);
    f.everything(sink, 0x2F, "SC0004");
    f.connect();

    let good = V2ipOutputMode {
        svd: 16,
        depth: 10,
        colour: V2ipColourSpace::YCBCR444,
        refresh: 50,
    };
    assert!(
        f.remote.set_v2ip_output_mode(sink, good).is_ok(),
        "the mode every case below is derived from was itself refused"
    );

    let refused = |what: &str, mode: V2ipOutputMode| {
        f.tap.clear();
        let got = f.remote.set_v2ip_output_mode(sink, mode);
        assert!(
            matches!(got, Err(ControlError::InvalidRequest(_))),
            "{what}: {got:?}"
        );
        assert!(
            f.tap.frames().is_empty(),
            "{what} reached the wire despite being refused"
        );
    };

    refused(
        "descriptor zero, which clears rather than sets",
        V2ipOutputMode { svd: 0, ..good },
    );
    refused(
        "a descriptor no table names",
        V2ipOutputMode { svd: 254, ..good },
    );
    // 16bpp has a bpp index and reads back from a device, but the output stage
    // refuses it - so it decodes cleanly and is then dropped in silence.
    refused("16 bits per pixel", V2ipOutputMode { depth: 16, ..good });
    refused(
        "a depth no index names",
        V2ipOutputMode { depth: 9, ..good },
    );
    refused(
        "a colour space outside the four defined",
        V2ipOutputMode {
            colour: V2ipColourSpace::from_wire(4),
            ..good
        },
    );
    // Out of range does not refuse the write on the sink: it silently replaces
    // the rate with 50, so a caller asking for 0 gets 50Hz rather than nothing.
    refused(
        "refresh below the range",
        V2ipOutputMode { refresh: 0, ..good },
    );
    refused(
        "refresh above the range",
        V2ipOutputMode {
            refresh: 144,
            ..good
        },
    );
}

/// Scaling settings belong to a sink, and a device without one takes the frame
/// and does nothing with it.
#[test]
fn a_scaling_write_needs_a_v2ip_sink() {
    let f = Fixture::new();
    let amp = uid_n(224);
    f.amplifier(amp, 0x28, "SC0005");
    f.connect();

    f.tap.clear();
    assert!(matches!(
        f.remote.set_v2ip_auto_scaling(amp, true),
        Err(ControlError::Unsupported(_))
    ));
    assert!(matches!(
        f.remote.clear_v2ip_output_mode(amp),
        Err(ControlError::Unsupported(_))
    ));
    assert!(
        f.tap.frames().is_empty(),
        "a device with no sink was sent a scaling write"
    );
}

/// A ping names the device it asks, and goes only to a device that answers
/// one: a device below 0x2A drops the frame as stamped above its own version.
#[test]
fn a_ping_names_its_device_and_needs_a_device_that_answers() {
    let f = Fixture::new();
    let uid = uid_n(225);
    f.everything(uid, 0x2A, "PG0001");

    f.tap.clear();
    assert!(!refused(&f.remote.ping(uid)));
    let frame = f.tap.frames().pop().expect("nothing reached the gate");
    assert_eq!(&frame[20..22], &op::SYS_PING.0.to_le_bytes());
    assert_eq!(&frame[2..4], &0x2Au16.to_le_bytes(), "the stamp");
    assert_eq!(&frame[HEADER_LEN..], uid.as_bytes(), "the payload");

    let old = uid_n(226);
    f.everything(old, 0x29, "PG0002");
    assert!(refused(&f.remote.ping(old)));
}

/// The power save writes put each value at its offset in the settings block,
/// behind its own bit.
#[test]
fn a_power_save_write_carries_its_value_behind_its_bit() {
    let f = Fixture::new();
    let uid = uid_n(227);
    f.everything(uid, 0x2A, "PS0001");
    report_settings(
        &f,
        uid,
        V2ipDeviceSetting::AUTO_POWER_SAVE | V2ipDeviceSetting::POWER_SAVE_SCHEDULE,
    );
    f.connect();

    f.tap.clear();
    f.remote
        .set_v2ip_auto_power_save(uid, 0x0102)
        .expect("the device has reported the setting");
    let frame = f.tap.frames().pop().expect("nothing reached the gate");
    let block = &frame[HEADER_LEN + 128..];
    assert_eq!(&block[..4], &(1u32 << 11).to_le_bytes(), "valid");
    assert_eq!(&block[14..16], &[0x02, 0x01], "the idle minutes");

    let schedule = V2ipPowerSaveSchedule {
        start: [1, 2, 3, 4, 5, 6, 7],
        end: [8, 9, 10, 11, 12, 13, 1439],
    };
    f.tap.clear();
    f.remote
        .set_v2ip_power_save_schedule(uid, schedule)
        .expect("the device has reported the setting");
    let frame = f.tap.frames().pop().expect("nothing reached the gate");
    let block = &frame[HEADER_LEN + 128..];
    assert_eq!(&block[..4], &(1u32 << 12).to_le_bytes(), "valid");
    let times: Vec<u16> = block[16..44]
        .chunks_exact(2)
        .map(|b| u16::from_le_bytes([b[0], b[1]]))
        .collect();
    assert_eq!(times, [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 1439]);

    let cached = f.remote.v2ip_device_settings(uid).expect("no settings");
    assert_eq!(cached.auto_power_save(), Some(0x0102));
    assert_eq!(cached.power_save_schedule(), Some(schedule));
}

/// A schedule with a time that is not a time of day is refused, and so is a
/// power save write to a device that has not reported having it.
#[test]
fn a_power_save_write_the_device_would_not_take_is_refused() {
    let f = Fixture::new();
    let uid = uid_n(228);
    f.everything(uid, 0x2A, "PS0002");
    report_settings(&f, uid, V2ipDeviceSetting::POWER_SAVE_SCHEDULE);
    f.connect();

    let mut late = V2ipPowerSaveSchedule::default();
    late.end[3] = V2IP_MINUTES_PER_DAY;
    assert!(matches!(
        f.remote.set_v2ip_power_save_schedule(uid, late),
        Err(ControlError::InvalidRequest(_))
    ));
    assert!(matches!(
        f.remote.set_v2ip_auto_power_save(uid, 5),
        Err(ControlError::Unsupported(_))
    ));
}

/// Each PTP write puts its byte at its offset in the settings block, behind its
/// own bit, and a value out of range is refused before anything is sent.
#[test]
fn a_ptp_write_carries_its_value_behind_its_bit() {
    let f = Fixture::new();
    let uid = uid_n(230);
    f.everything(uid, 0x2A, "PT0001");
    report_settings(&f, uid, V2ipDeviceSetting::PTP_SETTINGS);
    f.connect();

    let block_of = |f: &Fixture| {
        let frame = f.tap.frames().pop().expect("nothing reached the gate");
        frame[HEADER_LEN + 128..].to_vec()
    };

    f.tap.clear();
    f.remote
        .set_v2ip_ptp_mode(uid, V2ipPtpMode::FOLLOWER)
        .expect("the device has reported the setting");
    let block = block_of(&f);
    assert_eq!(&block[..4], &(1u32 << 14).to_le_bytes(), "mode: valid");
    assert_eq!(block[44], 1, "the mode");

    f.tap.clear();
    f.remote
        .set_v2ip_ptp_domain(uid, 0x7F)
        .expect("the device has reported the setting");
    let block = block_of(&f);
    assert_eq!(&block[..4], &(1u32 << 15).to_le_bytes(), "domain: valid");
    assert_eq!(block[45], 0x7F, "the domain");

    f.tap.clear();
    f.remote
        .set_v2ip_ptp_priority1(uid, 0x80)
        .expect("the device has reported the setting");
    let block = block_of(&f);
    assert_eq!(&block[..4], &(1u32 << 16).to_le_bytes(), "priority1: valid");
    assert_eq!(block[46], 0x80, "the priority1");

    let cached = f.remote.v2ip_device_settings(uid).expect("no settings");
    assert_eq!(cached.ptp_mode(), Some(V2ipPtpMode::FOLLOWER));
    assert_eq!(cached.ptp_domain(), Some(0x7F));
    assert_eq!(cached.ptp_priority1(), Some(0x80));

    f.tap.clear();
    assert!(matches!(
        f.remote.set_v2ip_ptp_mode(uid, V2ipPtpMode::from_wire(3)),
        Err(ControlError::InvalidRequest(_))
    ));
    assert!(matches!(
        f.remote.set_v2ip_ptp_domain(uid, V2IP_PTP_DOMAIN_MAX + 1),
        Err(ControlError::InvalidRequest(_))
    ));
    assert!(f.tap.frames().is_empty(), "a refused write was sent");
}

/// Each unit command names its unit, then carries the command and its flags,
/// stamped 0x2C. A network change appends five addresses in network byte
/// order, each with four distinct bytes so a reversed one shows.
#[test]
fn a_unit_command_names_its_unit_and_carries_its_command() {
    let f = Fixture::new();
    let uid = uid_n(231);
    f.everything(uid, 0x2C, "UC0001");
    f.connect();

    let sent = |f: &Fixture| {
        let frame = f.tap.frames().pop().expect("nothing reached the gate");
        assert_eq!(&frame[20..22], &op::SYS_UNIT_COMMAND.0.to_le_bytes());
        assert_eq!(&frame[2..4], &0x2Cu16.to_le_bytes(), "the stamp");
        frame[HEADER_LEN..].to_vec()
    };

    f.tap.clear();
    f.remote.clear_unit_status(uid).expect("sent");
    let p = sent(&f);
    assert_eq!(p.len(), 20);
    assert_eq!(&p[..16], uid.as_bytes(), "the unit");
    assert_eq!(&p[16..20], &[0, 0, 0, 0], "clear status, no flags");

    f.tap.clear();
    f.remote.upgrade_unit_fpga(uid, true).expect("sent");
    assert_eq!(&sent(&f)[16..18], &[1, 1], "upgrade, forced");
    f.tap.clear();
    f.remote.upgrade_unit_fpga(uid, false).expect("sent");
    assert_eq!(&sent(&f)[16..18], &[1, 0], "upgrade, not forced");

    f.tap.clear();
    f.remote.confirm_unit_network(uid).expect("sent");
    assert_eq!(&sent(&f)[16..18], &[3, 0], "confirm");

    f.tap.clear();
    f.remote
        .set_unit_network(
            uid,
            UnitNetConfig {
                ip: Ipv4Addr::new(10, 1, 2, 3),
                netmask: Ipv4Addr::new(255, 255, 254, 0),
                gateway: Ipv4Addr::new(10, 1, 2, 254),
                dns1: Ipv4Addr::new(8, 8, 4, 4),
                dns2: Ipv4Addr::new(9, 9, 9, 10),
            },
        )
        .expect("sent");
    let p = sent(&f);
    assert_eq!(p.len(), 40);
    assert_eq!(&p[16..18], &[2, 0], "a static network");
    assert_eq!(
        &p[20..40],
        &[10, 1, 2, 3, 255, 255, 254, 0, 10, 1, 2, 254, 8, 8, 4, 4, 9, 9, 9, 10]
    );

    f.tap.clear();
    f.remote.set_unit_dhcp(uid).expect("sent");
    let p = sent(&f);
    assert_eq!(p.len(), 40, "a network change carries its addresses");
    assert_eq!(&p[16..18], &[2, 2], "DHCP");
    assert_eq!(&p[20..40], &[0; 20]);
}

/// A unit command goes only to a OneIP unit new enough to take it, and a
/// network a unit cannot hold is refused before anything is sent.
#[test]
fn a_unit_command_the_unit_would_not_take_is_refused() {
    let f = Fixture::new();
    let old = uid_n(232);
    f.everything(old, 0x2B, "UC0002");
    assert!(refused(&f.remote.clear_unit_status(old)));

    let matrix = uid_n(233);
    f.feed(
        matrix,
        op::SYS_HELLO,
        &hello_payload(
            0x2C,
            "FF88",
            "UC0003",
            "4.8.0",
            DeviceFeature::VIDEO_ROUTING,
        ),
    );
    assert!(matches!(
        f.remote.clear_unit_status(matrix),
        Err(ControlError::Unsupported(_))
    ));

    let unit = uid_n(234);
    f.everything(unit, 0x2C, "UC0004");
    let good = UnitNetConfig {
        ip: Ipv4Addr::new(10, 1, 2, 3),
        netmask: Ipv4Addr::new(255, 255, 255, 0),
        ..UnitNetConfig::default()
    };
    let bad = [
        UnitNetConfig {
            ip: Ipv4Addr::UNSPECIFIED,
            ..good
        },
        UnitNetConfig {
            ip: Ipv4Addr::new(239, 1, 2, 3),
            ..good
        },
        UnitNetConfig {
            ip: Ipv4Addr::BROADCAST,
            ..good
        },
        UnitNetConfig {
            netmask: Ipv4Addr::new(255, 0, 255, 0),
            ..good
        },
        UnitNetConfig {
            netmask: Ipv4Addr::UNSPECIFIED,
            ..good
        },
    ];
    f.tap.clear();
    for network in bad {
        assert!(
            matches!(
                f.remote.set_unit_network(unit, network),
                Err(ControlError::InvalidRequest(_))
            ),
            "{network:?}"
        );
    }
    assert!(f.tap.frames().is_empty(), "a refused command was sent");
}

/// The power save level goes out as the settings block's last byte, and AES67
/// is switched like any on/off setting.
#[test]
fn a_power_save_level_and_aes67_write_carry_their_bits() {
    let f = Fixture::new();
    let uid = uid_n(235);
    f.everything(uid, 0x2C, "PL0001");
    report_settings(
        &f,
        uid,
        V2ipDeviceSetting::POWER_SAVE_LEVEL | V2ipDeviceSetting::AES67,
    );
    f.connect();

    f.tap.clear();
    f.remote
        .set_v2ip_power_save_level(uid, V2ipPowerSaveLevel::OFF)
        .expect("the device has reported the setting");
    let frame = f.tap.frames().pop().expect("nothing reached the gate");
    let block = &frame[HEADER_LEN + 128..];
    assert_eq!(block.len(), 48);
    assert_eq!(&block[..4], &(1u32 << 18).to_le_bytes(), "valid");
    assert_eq!(block[47], 3, "the level");
    assert_eq!(
        f.remote
            .v2ip_device_settings(uid)
            .map(|s| s.get(V2ipDeviceSetting::POWER_SAVE_DISABLED)),
        Some(None),
        "a switch the device does not have"
    );
    assert!(matches!(
        f.remote
            .set_v2ip_power_save_level(uid, V2ipPowerSaveLevel::from_wire(4)),
        Err(ControlError::InvalidRequest(_))
    ));

    f.tap.clear();
    f.remote
        .set_v2ip_device_setting(uid, V2ipDeviceSetting::AES67, true)
        .expect("AES67 is an on/off setting");
    let frame = f.tap.frames().pop().expect("nothing reached the gate");
    let block = &frame[HEADER_LEN + 128..];
    assert_eq!(&block[..4], &(1u32 << 17).to_le_bytes(), "valid");
    assert_eq!(&block[4..8], &(1u32 << 17).to_le_bytes(), "on");

    assert!(matches!(
        f.remote.set_all_v2ip_device_settings(V2ipDeviceSettings {
            valid: V2ipDeviceSetting::POWER_SAVE_LEVEL,
            power_save_level: V2ipPowerSaveLevel::from_wire(4),
            ..V2ipDeviceSettings::default()
        }),
        Err(ControlError::InvalidRequest(_))
    ));
}

/// The SAP switches are written as any switch is, and a SAP timing alone
/// still runs the frame on to the block that carries it.
#[test]
fn a_sap_write_carries_what_it_sets() {
    let f = Fixture::new();
    let uid = uid_n(235);
    f.everything(uid, 0x2F, "SP0001");
    report_settings(
        &f,
        uid,
        V2ipDeviceSetting::SAP_LISTEN
            | V2ipDeviceSetting::SAP_ANNOUNCE
            | V2ipDeviceSetting::SAP_EXPIRE,
    );
    f.connect();

    f.tap.clear();
    f.remote
        .set_v2ip_device_setting(uid, V2ipDeviceSetting::SAP_LISTEN, true)
        .expect("a switch the device has reported");
    let frame = f.tap.frames().pop().expect("nothing reached the gate");
    let p = &frame[HEADER_LEN..];
    let listen = V2ipDeviceSetting::SAP_LISTEN.bits().to_le_bytes();
    assert_eq!((&p[128..132], &p[132..136]), (&listen[..], &listen[..]));
    f.remote
        .set_v2ip_device_setting(uid, V2ipDeviceSetting::SAP_ANNOUNCE, false)
        .expect("a switch the device has reported");

    f.tap.clear();
    f.remote
        .set_v2ip_ptp_messages(
            uid,
            V2ipDeviceSetting::SAP_EXPIRE,
            V2ipPtpMessages {
                sap_expire_min: 15,
                ..V2ipPtpMessages::default()
            },
        )
        .expect("a value the device has reported");
    let frame = f.tap.frames().pop().expect("nothing reached the gate");
    let p = &frame[HEADER_LEN..];
    assert_eq!(p.len(), 224, "the frame stopped short of its block");
    assert_eq!(p[221], 15);
}

/// A PTP message write runs the frame on to its block at 216, past a VLAN
/// block and a PTP state that carry nothing; the same write for every device
/// puts the block behind the settings.
#[test]
fn a_ptp_message_write_carries_its_block() {
    let f = Fixture::new();
    let uid = uid_n(236);
    f.everything(uid, 0x2E, "PM0001");
    report_settings(&f, uid, V2ipDeviceSetting::SETTINGS_BLOCK);
    f.connect();

    let messages = V2ipPtpMessages {
        log_announce_interval: -3,
        log_sync_interval: -2,
        log_delay_req_interval: 1,
        announce_timeout: 4,
        dscp: 46,
        sap_expire_min: 60,
        sap_interval_s: 0x0102,
        stream_ttl: 8,
        sink_rotation: 90,
        sink_rotation_step: 45,
    };
    let which =
        V2ipDeviceSetting::PTP_SYNC | V2ipDeviceSetting::PTP_DSCP | V2ipDeviceSetting::SAP_INTERVAL;
    f.tap.clear();
    f.remote
        .set_v2ip_ptp_messages(uid, which, messages)
        .expect("the device has reported the settings");
    let frame = f.tap.frames().pop().expect("nothing reached the gate");
    let p = &frame[HEADER_LEN..];
    assert_eq!(p.len(), 224);
    assert_eq!(&p[128..132], &which.bits().to_le_bytes(), "valid");
    assert!(
        p[176..216].iter().all(|b| *b == 0),
        "a VLAN block or PTP state is set"
    );
    assert_eq!(&p[216..224], &[0xFD, 0xFE, 1, 4, 46, 60, 2, 1]);

    f.remote
        .set_v2ip_ptp_messages(
            uid,
            V2ipDeviceSetting::PTP_SYNC,
            V2ipPtpMessages {
                log_announce_interval: 2,
                log_sync_interval: -5,
                log_delay_req_interval: 3,
                announce_timeout: 9,
                dscp: 7,
                sap_expire_min: 5,
                sap_interval_s: 9,
                stream_ttl: 9,
                sink_rotation: 270,
                sink_rotation_step: 15,
            },
        )
        .expect("the device has reported the settings");
    let cached = f
        .remote
        .v2ip_device_settings(uid)
        .and_then(|s| s.ptp_messages())
        .expect("no settings");
    assert_eq!(cached.log_sync_interval, -5, "the one value carried");
    assert_eq!(
        (cached.dscp, cached.sap_interval_s),
        (46, 0x0102),
        "a value the write did not carry was replaced"
    );
    assert_eq!(
        (
            cached.log_announce_interval,
            cached.log_delay_req_interval,
            cached.announce_timeout
        ),
        (0, 0, 0),
        "a value the write did not carry was replaced"
    );

    f.tap.clear();
    let _ = f.remote.set_all_v2ip_device_settings(V2ipDeviceSettings {
        valid: which,
        ptp_messages: messages,
        ..V2ipDeviceSettings::default()
    });
    let frame = f.tap.frames().pop().expect("nothing reached the gate");
    let p = &frame[HEADER_LEN..];
    assert_eq!(p.len(), 56);
    assert_eq!(&p[48..56], &[0xFD, 0xFE, 1, 4, 46, 60, 2, 1]);

    f.tap.clear();
    let refused = [
        (V2ipDeviceSetting::NONE, messages),
        (
            V2ipDeviceSetting::PTP_SYNC | V2ipDeviceSetting::STATUS_LED,
            messages,
        ),
        (
            V2ipDeviceSetting::PTP_DSCP,
            V2ipPtpMessages {
                dscp: 64,
                ..messages
            },
        ),
    ];
    for (which, messages) in refused {
        assert!(
            matches!(
                f.remote.set_v2ip_ptp_messages(uid, which, messages),
                Err(ControlError::InvalidRequest(_))
            ),
            "{which:?}"
        );
    }
    assert!(matches!(
        f.remote.set_all_v2ip_device_settings(V2ipDeviceSettings {
            valid: V2ipDeviceSetting::PTP_DSCP,
            ptp_messages: V2ipPtpMessages {
                dscp: 64,
                ..messages
            },
            ..V2ipDeviceSettings::default()
        }),
        Err(ControlError::InvalidRequest(_))
    ));
    assert!(f.tap.frames().is_empty(), "a refused write was sent");
}

/// A session description goes out in 1 KB pages, each naming the output and
/// saying where it sits in the whole; the commands without one are the header
/// alone. All are stamped 0x2E.
#[test]
fn a_session_description_goes_out_in_pages() {
    let f = Fixture::new();
    let uid = uid_n(237);
    f.everything(uid, 0x2E, "SD0001");
    f.connect();

    let sdp: String = (0..2000).map(|i| (b'a' + (i % 26) as u8) as char).collect();
    f.tap.clear();
    f.remote.play_v2ip_sdp(uid, &sdp).expect("sent");
    let frames = f.tap.frames();
    assert_eq!(frames.len(), 2);
    let mut joined = Vec::new();
    for (i, frame) in frames.iter().enumerate() {
        assert_eq!(&frame[20..22], &op::V2IP_SINK_SDP.0.to_le_bytes());
        assert_eq!(&frame[2..4], &0x2Eu16.to_le_bytes(), "the stamp");
        let p = &frame[HEADER_LEN..];
        assert_eq!(&p[..16], uid.as_bytes(), "the output");
        assert_eq!(&p[16..20], &[1, 0, 0, 0], "a page");
        let len = [1024u16, 976][i];
        assert_eq!(&p[20..22], &2000u16.to_le_bytes(), "the whole");
        assert_eq!(&p[22..24], &((i * 1024) as u16).to_le_bytes(), "the offset");
        assert_eq!(&p[24..26], &len.to_le_bytes(), "the page");
        assert_eq!(&p[26..28], &[0, 0]);
        assert_eq!(p.len(), 28 + usize::from(len));
        joined.extend_from_slice(&p[28..]);
    }
    assert_eq!(joined, sdp.as_bytes());

    let header = |f: &Fixture| {
        let frame = f.tap.frames().pop().expect("nothing reached the gate");
        let p = frame[HEADER_LEN..].to_vec();
        assert_eq!(p.len(), 20);
        assert_eq!(&p[..16], uid.as_bytes());
        [p[16], p[17], p[18], p[19]]
    };
    f.remote.stop_v2ip_sdp(uid).expect("sent");
    assert_eq!(header(&f), [2, 0, 0, 0], "stop");
    f.remote.set_v2ip_sdp_screen(uid, 0xC3).expect("sent");
    assert_eq!(header(&f), [3, 0xC3, 0, 0], "screen");
    f.remote.request_v2ip_sink_sdp(uid).expect("sent");
    assert_eq!(header(&f), [4, 0, 0, 0], "request");
}

/// A description goes only to a OneIP output new enough to take it, and one
/// no output could take is refused before anything is sent.
#[test]
fn a_session_description_the_output_would_not_take_is_refused() {
    let f = Fixture::new();
    let old = uid_n(238);
    f.everything(old, 0x2D, "SD0002");
    assert!(refused(&f.remote.stop_v2ip_sdp(old)));
    assert!(refused(&f.remote.play_v2ip_sdp(old, "v=0")));

    let matrix = uid_n(239);
    f.feed(
        matrix,
        op::SYS_HELLO,
        &hello_payload(
            0x2E,
            "FF88",
            "SD0003",
            "4.8.0",
            DeviceFeature::VIDEO_ROUTING,
        ),
    );
    assert!(matches!(
        f.remote.play_v2ip_sdp(matrix, "v=0"),
        Err(ControlError::Unsupported(_))
    ));

    let unit = uid_n(240);
    f.everything(unit, 0x2E, "SD0004");
    f.tap.clear();
    for sdp in [String::new(), "x".repeat(2049)] {
        assert!(matches!(
            f.remote.play_v2ip_sdp(unit, &sdp),
            Err(ControlError::InvalidRequest(_))
        ));
    }
    assert!(f.tap.frames().is_empty(), "a refused description was sent");
    f.connect();
    f.remote
        .play_v2ip_sdp(unit, &"x".repeat(2048))
        .expect("an output takes 2 KB");
}

/// Settings for every device go out as the bare settings block, to everyone,
/// and change nothing here until a device reports them.
#[test]
fn settings_for_every_device_go_out_as_the_block_alone() {
    let f = Fixture::new();
    let uid = uid_n(229);
    f.everything(uid, 0x2A, "SA0001");
    report_settings(&f, uid, V2ipDeviceSetting::AUTO_POWER_SAVE);

    f.tap.clear();
    let settings = V2ipDeviceSettings {
        valid: V2ipDeviceSetting::STATUS_LED | V2ipDeviceSetting::AUTO_POWER_SAVE,
        flags: V2ipDeviceSetting::STATUS_LED,
        auto_power_save: 0x0304,
        ..V2ipDeviceSettings::default()
    };
    let _ = f.remote.set_all_v2ip_device_settings(settings);
    let frame = f.tap.frames().pop().expect("nothing reached the gate");
    assert_eq!(&frame[20..22], &op::V2IP_SETTINGS_ALL.0.to_le_bytes());
    assert_eq!(&frame[2..4], &0x2Au16.to_le_bytes(), "the stamp");
    let p = &frame[HEADER_LEN..];
    assert_eq!(p.len(), 48);
    assert_eq!(&p[..4], &((1u32 << 11) | (1 << 3)).to_le_bytes(), "valid");
    assert_eq!(&p[4..8], &(1u32 << 3).to_le_bytes(), "flags");
    assert_eq!(&p[14..16], &[0x04, 0x03], "the idle minutes");

    assert_eq!(
        f.remote
            .v2ip_device_settings(uid)
            .and_then(|s| s.auto_power_save()),
        Some(0),
        "a write for everyone was cached before any device took it"
    );
}

/// What every device would ignore is refused rather than broadcast.
#[test]
fn settings_every_device_would_ignore_are_refused() {
    let f = Fixture::new();
    let bad = [
        V2ipDeviceSettings::default(),
        V2ipDeviceSettings {
            valid: V2ipDeviceSetting::CLOCK_SET,
            ..V2ipDeviceSettings::default()
        },
        V2ipDeviceSettings {
            valid: V2ipDeviceSetting::IR_PROFILES,
            ..V2ipDeviceSettings::default()
        },
        V2ipDeviceSettings {
            valid: V2ipDeviceSetting::IR_PROFILE,
            ir_profile: V2IP_IR_PROFILE_MAX,
            ..V2ipDeviceSettings::default()
        },
        V2ipDeviceSettings {
            valid: V2ipDeviceSetting::IR_PROFILE_SINK,
            ir_profile_sink: V2IP_IR_PROFILE_NOT_SET - 1,
            ..V2ipDeviceSettings::default()
        },
        V2ipDeviceSettings {
            valid: V2ipDeviceSetting::POWER_SAVE_SCHEDULE,
            power_save: V2ipPowerSaveSchedule {
                start: [V2IP_MINUTES_PER_DAY; 7],
                end: [0; 7],
            },
            ..V2ipDeviceSettings::default()
        },
        V2ipDeviceSettings {
            valid: V2ipDeviceSetting::PTP_MODE,
            ptp_mode: V2ipPtpMode::from_wire(3),
            ..V2ipDeviceSettings::default()
        },
        V2ipDeviceSettings {
            valid: V2ipDeviceSetting::PTP_DOMAIN,
            ptp_domain: V2IP_PTP_DOMAIN_MAX + 1,
            ..V2ipDeviceSettings::default()
        },
    ];
    f.tap.clear();
    for settings in bad {
        assert!(
            matches!(
                f.remote.set_all_v2ip_device_settings(settings),
                Err(ControlError::InvalidRequest(_))
            ),
            "{settings:?}"
        );
    }
    assert!(f.tap.frames().is_empty(), "a refused write was sent");
}

/// A time zone goes out as two NUL-padded fields, to everyone, and one a
/// device could not hold is refused.
#[test]
fn a_time_zone_goes_out_in_its_two_fields() {
    let f = Fixture::new();
    f.tap.clear();
    let _ = f
        .remote
        .set_mesh_time_zone("Europe/Amsterdam", "CET-1CEST,M3.5.0,M10.5.0/3");
    let frame = f.tap.frames().pop().expect("nothing reached the gate");
    assert_eq!(&frame[20..22], &op::TIME_ZONE.0.to_le_bytes());
    assert_eq!(&frame[2..4], &0x2Au16.to_le_bytes(), "the stamp");
    let p = &frame[HEADER_LEN..];
    assert_eq!(p.len(), 112);
    assert_eq!(&p[..16], b"Europe/Amsterdam");
    assert!(p[16..48].iter().all(|b| *b == 0));
    assert_eq!(&p[48..74], b"CET-1CEST,M3.5.0,M10.5.0/3");
    assert!(p[74..].iter().all(|b| *b == 0));

    f.tap.clear();
    for (zone, rule) in [
        ("", "UTC0"),
        ("UTC", ""),
        (&*"Z".repeat(48), "UTC0"),
        ("UTC", &*"R".repeat(64)),
        ("UT\0C", "UTC0"),
    ] {
        assert!(
            matches!(
                f.remote.set_mesh_time_zone(zone, rule),
                Err(ControlError::InvalidRequest(_))
            ),
            "{zone:?} {rule:?}"
        );
    }
    assert!(f.tap.frames().is_empty(), "a refused time zone was sent");
    let _ = f
        .remote
        .set_mesh_time_zone(&"Z".repeat(47), &"R".repeat(63));
    assert_eq!(f.tap.frames().len(), 1, "the longest names that fit");
}

/// Clearing the time zone sends the frame whole, with both fields empty.
#[test]
fn a_cleared_time_zone_goes_out_empty() {
    let f = Fixture::new();
    f.tap.clear();
    let _ = f.remote.clear_mesh_time_zone();
    let frame = f.tap.frames().pop().expect("nothing reached the gate");
    assert_eq!(&frame[20..22], &op::TIME_ZONE.0.to_le_bytes());
    let p = &frame[HEADER_LEN..];
    assert_eq!(p.len(), 112, "a receiver drops a shorter frame");
    assert!(p.iter().all(|b| *b == 0));
}

/// A time goes out as seconds since 1970 in a u32, and one outside that is
/// refused.
#[test]
fn a_time_goes_out_as_seconds_since_1970() {
    use std::time::{Duration, UNIX_EPOCH};

    let f = Fixture::new();
    f.tap.clear();
    let _ = f
        .remote
        .set_mesh_time(UNIX_EPOCH + Duration::from_secs(0x6ABB_8FC6));
    let frame = f.tap.frames().pop().expect("nothing reached the gate");
    assert_eq!(&frame[20..22], &op::TIME.0.to_le_bytes());
    assert_eq!(&frame[HEADER_LEN..], &[0xC6, 0x8F, 0xBB, 0x6A]);

    for time in [
        UNIX_EPOCH - Duration::from_secs(1),
        UNIX_EPOCH + Duration::from_secs(1 << 32),
    ] {
        assert!(matches!(
            f.remote.set_mesh_time(time),
            Err(ControlError::InvalidRequest(_))
        ));
    }
}

/// A device's clock reads as the time it announced, moved on by how long ago
/// that was.
#[test]
fn a_device_clock_moves_on_from_what_it_announced() {
    use std::time::{Duration, UNIX_EPOCH};

    let f = Fixture::new();
    let uid = uid_n(230);
    f.everything(uid, 0x2A, "CL0001");
    assert_eq!(f.remote.device_clock(uid), None);
    f.feed(uid, op::TIME, &0x6ABB_8FC6u32.to_le_bytes());

    // Have the frame arrive a minute ago.
    {
        let mut state = lock(&f.remote.shared.state);
        let clock = &mut state.device_mut(uid).expect("registered above").clock;
        let (utc, at) = clock.expect("no clock was read");
        *clock = Some((utc, at - Duration::from_secs(60)));
    }

    let announced = UNIX_EPOCH + Duration::from_secs(0x6ABB_8FC6);
    let clock = f.remote.device_clock(uid).expect("no clock was read");
    let ahead = clock
        .duration_since(announced)
        .expect("the clock went backwards");
    assert!(
        (Duration::from_secs(60)..Duration::from_secs(65)).contains(&ahead),
        "{ahead:?}"
    );
}

/// Announces `uid` as a OneIP sink that takes VLANs and has it report `block`.
fn vlan_device(f: &Fixture, uid: DeviceUid, serial: &str, block: &[u8; 16]) {
    f.feed(
        uid,
        op::SYS_HELLO,
        &hello_payload(
            0x2B,
            "OneIP",
            serial,
            "4.8.0",
            DeviceFeature::V2IP_SINK | DeviceFeature::VLAN,
        ),
    );
    let cfg = Cfg::addresses(uid, "239.1.2.3");
    f.feed(uid, op::V2IP_DEVICE_CFG, &cfg.bytes_with_vlan(block));
}

/// A VLAN write carries nothing but the VLAN block, and of that only what a
/// writer sets: the flags the device reports or the controller confirms with,
/// the uplink in use and the revert seconds go out cleared.
#[test]
fn a_vlan_write_carries_nothing_but_the_vlan_block() {
    let f = Fixture::new();
    let uid = uid_n(229);
    let has_sfp = (V2ipVlanFlag::VALID | V2ipVlanFlag::HAS_SFP).bits();
    let reported = vlan_block(has_sfp, 5, [6, 7, 8], 0, 2, 0);
    vlan_device(&f, uid, "VL0001", &reported);
    f.connect();

    f.tap.clear();
    f.remote
        .set_v2ip_vlan(
            uid,
            V2ipVlan {
                flags: V2ipVlanFlag::from_bits(0xFFFF),
                device: 0x0123,
                port: [0x0456, 0x0789, 0x0ABC],
                uplink: 1,
                active_uplink: 3,
                revert_s: 9,
            },
        )
        .expect("the device takes VLANs and has an SFP port");
    let frame = f.tap.frames().pop().expect("nothing reached the gate");
    let p = &frame[HEADER_LEN..];

    assert_eq!(p.len(), 192, "the payload stops short of the VLAN block");
    assert_eq!(&p[..16], uid.as_bytes(), "the frame names another device");
    assert!(
        p[16..40].iter().all(|b| *b == 0),
        "a source address would repoint the encoder"
    );
    assert!(
        !(V2IP_SOURCE_RATE_MIN..=V2IP_SOURCE_RATE_MAX).contains(&p[40]),
        "the rate is inside the valid range, so it would replace the device's"
    );
    assert!(
        p[41..176].iter().all(|b| *b == 0),
        "a field between the rate and the VLAN block is set"
    );
    assert_eq!(
        &p[176..],
        &[0x03, 0x00, 0x23, 0x01, 0x56, 0x04, 0x89, 0x07, 0xBC, 0x0A, 1, 0, 0, 0, 0, 0],
        "the VLAN block"
    );

    let cached = f.remote.v2ip_vlan(uid).expect("no VLAN configuration");
    assert_eq!(
        cached.device, 5,
        "a write was cached before the device ran it"
    );
}

/// A write the device would not take is refused before it is sent.
#[test]
fn a_vlan_write_the_device_would_not_take_is_refused() {
    let f = Fixture::new();
    let valid_only = V2ipVlanFlag::VALID.bits();
    let no_sfp = uid_n(230);
    vlan_device(
        &f,
        no_sfp,
        "VL0002",
        &vlan_block(valid_only, 0, [0; 3], 0, 2, 0),
    );
    let unreported = uid_n(231);
    f.feed(
        unreported,
        op::SYS_HELLO,
        &hello_payload(0x2B, "OneIP", "VL0003", "4.8.0", DeviceFeature::VLAN),
    );
    let without = uid_n(232);
    f.everything(without, 0x2B, "VL0004");
    f.connect();

    let write = |uplink: u8, device: u16| V2ipVlan {
        device,
        uplink,
        ..V2ipVlan::default()
    };
    let invalid = |r| matches!(r, Err(ControlError::InvalidRequest(_)));
    assert!(invalid(f.remote.set_v2ip_vlan(no_sfp, write(0, 4095))));
    assert!(invalid(f.remote.set_v2ip_vlan(no_sfp, write(4, 1))));
    assert!(matches!(
        f.remote.set_v2ip_vlan(no_sfp, write(1, 1)),
        Err(ControlError::Unsupported(_))
    ));
    assert!(matches!(
        f.remote.set_v2ip_vlan(unreported, write(0, 1)),
        Err(ControlError::NotReported(_))
    ));
    assert!(matches!(
        f.remote.set_v2ip_vlan(without, write(0, 1)),
        Err(ControlError::Unsupported(_))
    ));

    f.tap.clear();
    f.remote
        .set_v2ip_vlan(no_sfp, write(3, V2IP_VLAN_ID_MAX))
        .expect("the highest id and the last port are taken");
    assert_eq!(f.tap.frames().len(), 1);
}

/// Announces `uid` as a sink on `protocol` whose video processor reports
/// `features`, 0 for a processor that has reported nothing yet.
fn testcard_sink(f: &Fixture, uid: DeviceUid, protocol: u16, serial: &str, features: u64) {
    f.feed(
        uid,
        op::SYS_HELLO,
        &hello_payload(protocol, "OneIP", serial, "4.8.0", DeviceFeature::V2IP_SINK),
    );
    let mut cfg = Cfg::addresses(uid, "239.1.2.3");
    cfg.codec = features;
    f.feed(uid, op::V2IP_DEVICE_CFG, &cfg.bytes_with_options());
}

/// Each test card write puts its part at its own offsets behind its own bit,
/// and leaves every other byte zero.
#[test]
fn a_testcard_write_carries_its_part_behind_its_bit() {
    let f = Fixture::new();
    let uid = uid_n(233);
    testcard_sink(
        &f,
        uid,
        0x2B,
        "TC0003",
        V2ipFpgaFeature::SINK_TEST_PATTERN.bits(),
    );
    f.connect();

    let sent = |f: &Fixture| {
        let frame = f.tap.frames().pop().expect("nothing reached the gate");
        assert_eq!(&frame[20..22], &op::V2IP_TESTCARD.0.to_le_bytes());
        assert_eq!(&frame[2..4], &0x2Bu16.to_le_bytes(), "the stamp");
        let p = frame[HEADER_LEN..].to_vec();
        assert_eq!(p.len(), 56);
        assert_eq!(&p[..16], uid.as_bytes());
        p
    };

    f.tap.clear();
    f.remote
        .request_v2ip_testcard(uid)
        .expect("a test card sink");
    let p = sent(&f);
    assert!(
        p[16..].iter().all(|b| *b == 0),
        "a request carries only the uid"
    );

    f.tap.clear();
    f.remote
        .set_v2ip_test_pattern(uid, V2ipTestPattern::GRID, 0x0012_3456)
        .expect("a test card sink");
    let mut want = [0u8; 40];
    want[..8].copy_from_slice(&[1, 1, 4, 0, 0x56, 0x34, 0x12, 0]);
    assert_eq!(&sent(&f)[16..56], &want[..], "the pattern");

    f.tap.clear();
    let tone = V2ipTestTone {
        mode: V2ipToneMode::CONTINUOUS,
        freq: 0x0457,
        level: -23,
        channels: 6,
        rate: 48_000,
    };
    f.remote
        .set_v2ip_test_tone(uid, tone)
        .expect("a test card sink");
    let p = sent(&f);
    assert_eq!(&p[16..20], &[1, 2, 0, 1]);
    assert_eq!(
        &p[20..32],
        &[0, 0, 0, 0, 0x57, 0x04, 0xE9, 6, 0x80, 0xBB, 0, 0]
    );
    assert!(p[32..].iter().all(|b| *b == 0), "the tone");

    f.tap.clear();
    let sync = V2ipTestSync {
        period: 0x0312,
        lead: 0x0211,
        offset: 0x00AB_CDEF,
        beep_ms: 0x0765,
    };
    f.remote
        .set_v2ip_test_sync(uid, sync)
        .expect("a test card sink");
    let p = sent(&f);
    assert_eq!(&p[16..18], &[1, 4]);
    assert!(p[18..32].iter().all(|b| *b == 0));
    assert_eq!(
        &p[32..42],
        &[0x12, 0x03, 0x11, 0x02, 0xEF, 0xCD, 0xAB, 0x00, 0x65, 0x07],
        "the lip-sync"
    );
    assert!(p[42..].iter().all(|b| *b == 0), "reserved and counters");
    assert_eq!(f.remote.v2ip_testcard(uid), None, "a write was cached");
}

/// A write the sink could not take, and a sink that cannot draw a test card,
/// are refused before anything is sent.
#[test]
fn a_testcard_write_the_sink_would_not_take_is_refused() {
    let f = Fixture::new();
    let sink = uid_n(234);
    testcard_sink(
        &f,
        sink,
        0x2B,
        "TC0004",
        V2ipFpgaFeature::SINK_TEST_PATTERN.bits(),
    );
    let without = uid_n(235);
    testcard_sink(
        &f,
        without,
        0x2B,
        "TC0005",
        V2ipFpgaFeature::SINK_STATE.bits(),
    );
    let unreported = uid_n(236);
    testcard_sink(&f, unreported, 0x2B, "TC0006", 0);
    let old = uid_n(237);
    testcard_sink(
        &f,
        old,
        0x2A,
        "TC0007",
        V2ipFpgaFeature::SINK_TEST_PATTERN.bits(),
    );
    f.connect();
    f.tap.clear();

    let invalid = |r| matches!(r, Err(ControlError::InvalidRequest(_)));
    let r = &f.remote;
    assert!(invalid(r.set_v2ip_test_pattern(
        sink,
        V2ipTestPattern::from_wire(7),
        0
    )));
    assert!(invalid(r.set_v2ip_test_pattern(
        sink,
        V2ipTestPattern::FLAT,
        0x0100_0000
    )));
    let tone = V2ipTestTone {
        mode: V2ipToneMode::LINEUP,
        freq: 1000,
        level: 0,
        channels: 2,
        rate: 48_000,
    };
    for bad in [
        V2ipTestTone {
            mode: V2ipToneMode::from_wire(5),
            ..tone
        },
        V2ipTestTone { freq: 19, ..tone },
        V2ipTestTone {
            freq: 20_001,
            ..tone
        },
        V2ipTestTone { level: -61, ..tone },
        V2ipTestTone { level: 1, ..tone },
        V2ipTestTone {
            channels: 1,
            ..tone
        },
        V2ipTestTone {
            channels: 9,
            ..tone
        },
        V2ipTestTone {
            rate: 32_000,
            ..tone
        },
    ] {
        assert!(invalid(r.set_v2ip_test_tone(sink, bad)), "{bad:?}");
    }
    let sync = V2ipTestSync {
        period: 10,
        lead: 9,
        offset: 0x00FF_FFFF,
        beep_ms: 10_000,
    };
    for bad in [
        V2ipTestSync { lead: 10, ..sync },
        V2ipTestSync { period: 0, ..sync },
        V2ipTestSync {
            offset: 0x0100_0000,
            ..sync
        },
        V2ipTestSync { beep_ms: 0, ..sync },
        V2ipTestSync {
            beep_ms: 10_001,
            ..sync
        },
    ] {
        assert!(invalid(r.set_v2ip_test_sync(sink, bad)), "{bad:?}");
    }
    assert!(matches!(
        r.request_v2ip_testcard(without),
        Err(ControlError::Unsupported(_))
    ));
    assert!(matches!(
        r.request_v2ip_testcard(unreported),
        Err(ControlError::NotReported(_))
    ));
    assert!(refused(&r.request_v2ip_testcard(old)));
    assert!(f.tap.frames().is_empty(), "a refused write was sent");

    // The edges that are taken, and a tone turned off whatever it holds.
    r.set_v2ip_test_tone(sink, tone)
        .expect("a two-channel line-up");
    r.set_v2ip_test_tone(
        sink,
        V2ipTestTone {
            freq: 20,
            level: -60,
            channels: 8,
            rate: 96_000,
            ..tone
        },
    )
    .expect("the low edges");
    r.set_v2ip_test_tone(sink, V2ipTestTone::default())
        .expect("off");
    r.set_v2ip_test_sync(sink, sync).expect("the high edges");
    r.set_v2ip_test_sync(
        sink,
        V2ipTestSync {
            beep_ms: 1,
            ..V2ipTestSync::default()
        },
    )
    .expect("no marks at all");
    r.set_v2ip_test_pattern(sink, V2ipTestPattern::CARD, 0x00FF_FFFF)
        .expect("the last pattern and the widest colour");
    assert_eq!(f.tap.frames().len(), 6);
}

/// Has `uid` report a tree whose endpoint 1 can lock its source and whose
/// endpoint 0 cannot, but can be muted.
fn report_lockable_tree(f: &Fixture, uid: DeviceUid) {
    let mut p = vec![0u8; 68];
    p[4..20].copy_from_slice(uid.as_bytes());
    p[28..30].copy_from_slice(&2u16.to_le_bytes());
    p[37] = 1;
    let output = AudioFeatures::OUTPUT.bits() | AudioFeatures::MUTE.bits();
    p[44..48].copy_from_slice(&output.to_le_bytes());
    p[52] = 1;
    p[53] = 1;
    let input = AudioFeatures::INPUT.bits() | AudioFeatures::AUDIO_LOCK.bits();
    p[60..64].copy_from_slice(&input.to_le_bytes());
    f.feed(uid, op::V2IP_AUDIO, &p);
}

/// A lock goes out as the endpoint commands do, on sub-opcode 6, and only to
/// an endpoint that reports it can lock.
#[test]
fn an_audio_lock_goes_only_to_an_endpoint_that_can_lock() {
    let f = Fixture::new();
    let uid = uid_n(238);
    f.everything(uid, 0x2B, "AL0001");
    let unreported = uid_n(239);
    f.everything(unreported, 0x2B, "AL0002");
    report_lockable_tree(&f, uid);
    f.connect();

    f.tap.clear();
    assert!(matches!(
        f.remote.set_audio_endpoint_locked(uid, 0, true),
        Err(ControlError::Unsupported(_))
    ));
    assert!(matches!(
        f.remote.set_audio_endpoint_locked(uid, 7, true),
        Err(ControlError::Unsupported(_))
    ));
    assert!(matches!(
        f.remote.set_audio_endpoint_locked(unreported, 1, true),
        Err(ControlError::NotReported(_))
    ));
    assert!(f.tap.frames().is_empty(), "a refused lock was sent");

    f.remote
        .set_audio_endpoint_locked(uid, 1, true)
        .expect("endpoint 1 can lock");
    let frame = f.tap.frames().pop().expect("nothing reached the gate");
    let p = &frame[HEADER_LEN..];
    assert_eq!(p.len(), 28);
    assert_eq!(&p[..2], &[6, 0], "the LOCK sub-opcode");
    assert_eq!(&p[4..20], uid.as_bytes());
    assert_eq!(&p[20..28], &[1, 0, 0, 0, 1, 0, 0, 0]);

    // Mute and trigger go only to an endpoint with that feature too.
    f.tap.clear();
    assert!(matches!(
        f.remote.set_audio_endpoint_muted(uid, 1, true),
        Err(ControlError::Unsupported(_))
    ));
    assert!(matches!(
        f.remote.set_audio_endpoint_trigger(uid, 0, true),
        Err(ControlError::Unsupported(_))
    ));
    assert!(f.tap.frames().is_empty(), "a refused command was sent");
    f.remote
        .set_audio_endpoint_muted(uid, 0, true)
        .expect("endpoint 0 can be muted");
    let frame = f.tap.frames().pop().expect("nothing reached the gate");
    assert_eq!(&frame[HEADER_LEN..HEADER_LEN + 2], &[1, 0], "MUTE");
}

/// Handing a source's addresses back goes out as mesh operation 6 naming the
/// source, with the parameter and padding zero, stamped as every mesh
/// operation is.
#[test]
fn auto_addresses_goes_out_as_mesh_operation_6() {
    let f = Fixture::new();
    let uid = uid_n(240);
    f.everything(uid, 0x2B, "AA0001");
    f.connect();

    f.tap.clear();
    f.remote
        .auto_assign_v2ip_source_addresses(uid)
        .expect("a OneIP source on 0x2B");
    let frame = f.tap.frames().pop().expect("nothing reached the gate");
    assert_eq!(&frame[20..22], &op::MESH_OPERATION.0.to_le_bytes());
    assert_eq!(
        &frame[2..4],
        &protocol_for(op::MESH_OPERATION).to_le_bytes(),
        "the stamp"
    );
    let p = &frame[HEADER_LEN..];
    assert_eq!(p.len(), 40);
    assert_eq!(&p[..4], &[6, 0, 0, 0]);
    assert_eq!(&p[4..20], uid.as_bytes());
    assert!(p[20..].iter().all(|b| *b == 0), "the parameter and padding");
}

/// A device that is not a source, or that predates the operation, is refused
/// before anything is sent.
#[test]
fn auto_addresses_is_refused_where_it_would_be_ignored() {
    let f = Fixture::new();
    let old = uid_n(241);
    f.everything(old, 0x2A, "AA0002");
    let sink_only = uid_n(242);
    f.feed(
        sink_only,
        op::SYS_HELLO,
        &hello_payload(0x2B, "OneIP", "AA0003", "4.8.0", DeviceFeature::V2IP_SINK),
    );
    f.connect();
    f.tap.clear();

    assert!(matches!(
        f.remote.auto_assign_v2ip_source_addresses(old),
        Err(ControlError::Send(SendError::ProtocolTooOld {
            have: 0x2A,
            need: 0x2B,
            ..
        }))
    ));
    assert!(matches!(
        f.remote.auto_assign_v2ip_source_addresses(sink_only),
        Err(ControlError::Unsupported(_))
    ));
    assert!(matches!(
        f.remote.auto_assign_v2ip_source_addresses(uid_n(243)),
        Err(ControlError::UnknownDevice(_))
    ));
    assert!(f.tap.frames().is_empty(), "a refused operation was sent");
}

/// What an input's source shows reaches the bay's snapshot.
#[test]
fn a_bay_snapshot_carries_its_inputs_content() {
    let f = Fixture::new();
    let unit = uid_n(150);
    f.everything(unit, PROTOCOL_VERSION, "CT0001");
    f.feed(unit, op::BAY_CONTENT, b"\x01\x00name\0BBC One\0");

    let info = f.remote.bay(BayUid::new(unit, 1)).expect("no bay");
    assert_eq!(
        info.content,
        [crate::types::BayContentField {
            key: "name".to_owned(),
            value: "BBC One".to_owned(),
        }]
    );
}

/// The last overlay sync a management unit sent, and the last report a unit
/// gave, are kept per device.
#[test]
fn an_overlay_sync_and_its_report_are_kept_per_device() {
    let f = Fixture::new();
    let (manager, unit) = (uid_n(151), uid_n(152));
    f.feed(
        manager,
        op::SYS_HELLO,
        &hello_payload(0x2F, "Manager", "MG0002", "4.8.0", DeviceFeature::MANAGER),
    );
    f.everything(unit, PROTOCOL_VERSION, "OS0001");
    let header = |kind: u8, target: DeviceUid| {
        let mut p = vec![2, kind, 0x34, 0x12];
        p.extend_from_slice(target.as_bytes());
        p
    };
    let mut sync = header(0, DeviceUid::ZERO);
    sync.resize(80, 0);
    sync[20] = 1;
    f.feed(manager, op::V2IP_OSD_SYNC, &sync);
    let mut report = header(3, unit);
    report.resize(90, 0);
    report[20] = 1;
    f.feed(unit, op::V2IP_OSD_SYNC, &report);

    let sync = f.remote.osd_sync(manager).expect("no sync was kept");
    assert_eq!(
        (sync.seq, sync.what),
        (0x1234, crate::wire::OsdSyncWhat::CAPTIONS)
    );
    let report = f.remote.osd_sync_report(unit).expect("no report was kept");
    assert_eq!(report.seq, 0x1234);
    assert_eq!(report.state, crate::wire::OsdSyncState::DONE);
    assert_eq!(f.remote.osd_sync(unit), None);
    assert_eq!(f.remote.osd_sync_report(manager), None);
}

/// Power save disabled is a switch, and a level write switches it off, as the
/// device does on taking the level; a report that states both keeps its own.
#[test]
fn a_power_save_level_switches_power_save_disabled_off() {
    let f = Fixture::new();
    let uid = uid_n(237);
    f.everything(uid, 0x2F, "PD0001");
    let both = V2ipDeviceSetting::POWER_SAVE_LEVEL | V2ipDeviceSetting::POWER_SAVE_DISABLED;
    let cfg = Cfg::addresses(uid, "239.1.2.3");
    let mut p = cfg.bytes_with_power_save(
        both.bits(),
        V2ipDeviceSetting::POWER_SAVE_DISABLED.bits(),
        0,
        &V2ipPowerSaveSchedule::default(),
    );
    p.resize(224, 0);
    f.feed(uid, op::V2IP_DEVICE_CFG, &p);
    f.connect();
    let disabled = || {
        f.remote
            .v2ip_device_settings(uid)
            .and_then(|s| s.get(V2ipDeviceSetting::POWER_SAVE_DISABLED))
    };
    assert_eq!(disabled(), Some(true), "the report's own value");

    f.remote
        .set_v2ip_device_setting(uid, V2ipDeviceSetting::POWER_SAVE_DISABLED, true)
        .expect("a switch the device has reported");
    f.remote
        .set_v2ip_power_save_level(uid, V2ipPowerSaveLevel::default())
        .expect("a level the device has reported");
    assert_eq!(disabled(), Some(false));
}

/// Power save is switched on as a mesh controller switches it, with both
/// bits and a level of 0, and off with the switch alone. A device picks its
/// level itself, so a written level, from this client or another sender, is
/// never cached, and only switches power save on.
#[test]
fn power_save_is_switched_and_its_level_is_the_devices() {
    let f = Fixture::new();
    let (uid, controller) = (uid_n(239), uid_n(240));
    f.everything(uid, 0x30, "PD0002");
    f.feed(
        controller,
        op::SYS_HELLO,
        &hello_payload(0x30, "Ctrl", "PD0003", "4.8.0", DeviceFeature::MANAGER),
    );
    let both = V2ipDeviceSetting::POWER_SAVE_LEVEL | V2ipDeviceSetting::POWER_SAVE_DISABLED;
    let cfg = Cfg::addresses(uid, "239.1.2.3");
    let mut p = cfg.bytes_with_power_save(
        both.bits(),
        V2ipDeviceSetting::POWER_SAVE_DISABLED.bits(),
        0,
        &V2ipPowerSaveSchedule::default(),
    );
    p.resize(224, 0);
    p[175] = V2ipPowerSaveLevel::LOW.to_wire();
    f.feed(uid, op::V2IP_DEVICE_CFG, &p);
    f.connect();
    let cached = || {
        f.remote.v2ip_device_settings(uid).map(|s| {
            (
                s.get(V2ipDeviceSetting::POWER_SAVE_DISABLED),
                s.power_save_level(),
            )
        })
    };
    let low = Some(V2ipPowerSaveLevel::LOW);

    f.tap.clear();
    f.remote.set_v2ip_power_save(uid, true).expect("on");
    let frame = f.tap.frames().pop().expect("nothing reached the gate");
    let block = &frame[HEADER_LEN + 128..];
    assert_eq!(&block[..4], &both.bits().to_le_bytes(), "valid");
    assert_eq!(&block[4..8], &[0; 4], "flags");
    assert_eq!(block[47], 0, "the level");
    assert_eq!(cached(), Some((Some(false), low)));

    f.tap.clear();
    f.remote.set_v2ip_power_save(uid, false).expect("off");
    let frame = f.tap.frames().pop().expect("nothing reached the gate");
    let block = &frame[HEADER_LEN + 128..];
    let disabled = V2ipDeviceSetting::POWER_SAVE_DISABLED.bits().to_le_bytes();
    assert_eq!((&block[..4], &block[4..8]), (&disabled[..], &disabled[..]));
    assert_eq!(cached(), Some((Some(true), low)));

    f.remote
        .set_v2ip_power_save_level(uid, V2ipPowerSaveLevel::OFF)
        .expect("a level the device has reported");
    assert_eq!(cached(), Some((Some(false), low)), "this client's level");

    // Another sender's level: a flag outside `valid` says nothing, and one
    // behind its own bit decides.
    let write = |valid: V2ipDeviceSetting| {
        let disabled = V2ipDeviceSetting::POWER_SAVE_DISABLED.bits();
        let mut p =
            cfg.bytes_with_power_save(valid.bits(), disabled, 0, &V2ipPowerSaveSchedule::default());
        p[175] = V2ipPowerSaveLevel::OFF.to_wire();
        p
    };
    f.remote.set_v2ip_power_save(uid, false).expect("off");
    f.feed(
        controller,
        op::V2IP_DEVICE_CFG,
        &write(V2ipDeviceSetting::POWER_SAVE_LEVEL),
    );
    assert_eq!(cached(), Some((Some(false), low)), "another sender's level");
    f.feed(controller, op::V2IP_DEVICE_CFG, &write(both));
    assert_eq!(cached(), Some((Some(true), low)), "and its switch");
}

/// The mesh-wide switch writes what the per-device one does, in the frame
/// every device of the mesh applies.
#[test]
fn power_save_is_switched_on_the_whole_mesh_as_on_one_device() {
    let f = Fixture::new();
    let both = V2ipDeviceSetting::POWER_SAVE_LEVEL | V2ipDeviceSetting::POWER_SAVE_DISABLED;
    let disabled = V2ipDeviceSetting::POWER_SAVE_DISABLED;
    for (enabled, valid, flags) in [(true, both, 0), (false, disabled, disabled.bits())] {
        f.tap.clear();
        let _ = f.remote.set_all_v2ip_power_save(enabled);
        let frame = f.tap.frames().pop().expect("nothing reached the gate");
        let p = &frame[HEADER_LEN..];
        assert_eq!(p.len(), 48, "{enabled}");
        assert_eq!(&p[..4], &valid.bits().to_le_bytes(), "{enabled}");
        assert_eq!(&p[4..8], &flags.to_le_bytes(), "{enabled}");
        assert_eq!(p[47], 0, "{enabled}");
    }
}

/// The cloud connection is a switch, written to a device that reports it.
#[test]
fn the_cloud_connection_is_a_switch() {
    let f = Fixture::new();
    let uid = uid_n(238);
    f.everything(uid, 0x2F, "CL0001");
    report_settings(&f, uid, V2ipDeviceSetting::CLOUD);
    f.connect();

    f.tap.clear();
    f.remote
        .set_v2ip_device_setting(uid, V2ipDeviceSetting::CLOUD, true)
        .expect("a switch the device has reported");
    let frame = f.tap.frames().pop().expect("nothing reached the gate");
    let p = &frame[HEADER_LEN..];
    let cloud = V2ipDeviceSetting::CLOUD.bits().to_le_bytes();
    assert_eq!((&p[128..132], &p[132..136]), (&cloud[..], &cloud[..]));
}

/// A request for a source unit's encryption names it, or the zero uid for
/// every unit, as type 1 with a count of 0, stamped 0x30.
#[test]
fn a_privacy_request_names_the_unit_asked() {
    let f = Fixture::new();
    let uid = uid_n(245);
    f.everything(uid, 0x30, "PV0001");
    f.connect();

    let sent = |f: &Fixture| {
        let frame = f.tap.frames().pop().expect("nothing reached the gate");
        assert_eq!(&frame[20..22], &op::V2IP_PRIVACY.0.to_le_bytes());
        assert_eq!(&frame[2..4], &0x30u16.to_le_bytes(), "the stamp");
        frame[HEADER_LEN..].to_vec()
    };
    f.tap.clear();
    f.remote
        .request_v2ip_privacy(uid)
        .expect("a source on 0x30");
    let p = sent(&f);
    assert_eq!(
        (&p[..16], &p[16..]),
        (&uid.as_bytes()[..], &[1, 0, 0, 0][..])
    );

    f.remote.request_all_v2ip_privacy().expect("sent");
    let p = sent(&f);
    assert_eq!((&p[..16], &p[16..]), (&[0; 16][..], &[1, 0, 0, 0][..]));
}

/// A unit that is not a source, or that predates the report, is not asked.
#[test]
fn a_privacy_request_is_refused_where_it_would_be_ignored() {
    let f = Fixture::new();
    let old = uid_n(246);
    f.everything(old, 0x2F, "PV0002");
    let sink_only = uid_n(247);
    f.feed(
        sink_only,
        op::SYS_HELLO,
        &hello_payload(0x30, "OneIP", "PV0003", "4.8.0", DeviceFeature::V2IP_SINK),
    );
    f.connect();
    f.tap.clear();

    assert!(refused(&f.remote.request_v2ip_privacy(old)));
    assert!(matches!(
        f.remote.request_v2ip_privacy(sink_only),
        Err(ControlError::Unsupported(_))
    ));
    assert!(f.tap.frames().is_empty(), "a refused request was sent");
}

/// The bay names a handler was told of.
#[derive(Default)]
struct Names(Mutex<Vec<(BayUid, String)>>);

impl EventHandler for Names {
    fn on_name_changed(&self, bay: BayUid, name: String) {
        self.0.lock().expect("test handler").push((bay, name));
    }
}

/// A change this client makes to a unit's own bay is reported for the mesh
/// bays standing for it, as a change the unit reports is.
#[test]
fn a_change_made_here_reaches_the_mesh_bays() {
    let names = Arc::new(Names::default());
    let f = Fixture::with_handler(Arc::clone(&names) as Arc<dyn EventHandler>);
    let (owner, receiver) = (uid_n(238), uid_n(239));
    f.feed(
        owner,
        op::SYS_HELLO,
        &hello_payload(0x2F, "ONEIP", "MB0001", "4.8.0", DeviceFeature::V2IP_SOURCE),
    );
    f.feed(
        owner,
        op::SYS_BAY_CONFIG,
        &bay_config_rec(
            0,
            0,
            0,
            "Input 1",
            "Apple TV",
            BayStatus::NONE,
            BayFeatures::HDMI_IN | BayFeatures::V2IP_SOURCE_LOCAL,
        ),
    );
    f.feed(
        receiver,
        op::SYS_HELLO,
        &hello_payload(0x2F, "ONEIP", "MB0002", "4.8.0", DeviceFeature::V2IP_SINK),
    );
    let mut page = vec![0u8; 8];
    page[0..2].copy_from_slice(&((1u16 << 1) | 1).to_le_bytes());
    page[2..4].copy_from_slice(&1u16.to_le_bytes());
    page.extend_from_slice(owner.as_bytes());
    f.feed(receiver, op::V2IP_BAY_MAPPINGS, &page);
    f.connect();

    f.remote
        .set_bay_name(BayUid::new(owner, 0), "Kodi")
        .expect("a bay the unit has reported");
    let seen = names.0.lock().expect("test handler").clone();
    assert!(
        seen.contains(&(BayUid::new(receiver, 1), "Kodi".to_owned())),
        "{seen:?}"
    );
}

/// The stream TTL runs the settings block on to 16 bytes, and only a write
/// carrying it does; a TTL of 0 is refused.
#[test]
fn a_stream_ttl_write_runs_the_block_on() {
    let f = Fixture::new();
    let uid = uid_n(240);
    f.everything(uid, 0x2F, "TT0001");
    let cfg = Cfg::addresses(uid, "239.1.2.3");
    let ttl = V2ipDeviceSetting::STREAM_TTL;
    let mut p = cfg.bytes_with_power_save(ttl.bits(), 0, 0, &V2ipPowerSaveSchedule::default());
    p.resize(232, 0);
    p[224] = 32;
    f.feed(uid, op::V2IP_DEVICE_CFG, &p);
    let reported = f
        .remote
        .v2ip_device_settings(uid)
        .and_then(|s| s.ptp_messages())
        .map(|m| m.stream_ttl);
    assert_eq!(reported, Some(32));
    f.connect();

    let messages = V2ipPtpMessages {
        stream_ttl: 16,
        ..V2ipPtpMessages::default()
    };
    f.tap.clear();
    f.remote
        .set_v2ip_ptp_messages(uid, ttl, messages)
        .expect("the device has reported the setting");
    let frame = f.tap.frames().pop().expect("nothing reached the gate");
    let p = &frame[HEADER_LEN..];
    assert_eq!(p.len(), 232);
    assert_eq!(p[224], 16);
    assert!(p[225..].iter().all(|b| *b == 0), "a reserved byte is set");

    f.tap.clear();
    let all = V2ipDeviceSettings {
        valid: ttl,
        ptp_messages: messages,
        ..V2ipDeviceSettings::default()
    };
    f.remote
        .set_all_v2ip_device_settings(all)
        .expect("a TTL in range");
    let frame = f.tap.frames().pop().expect("nothing reached the gate");
    let p = &frame[HEADER_LEN..];
    assert_eq!((p.len(), p[56]), (64, 16));

    let zero = V2ipPtpMessages::default();
    assert!(matches!(
        f.remote.set_v2ip_ptp_messages(uid, ttl, zero),
        Err(ControlError::InvalidRequest(_))
    ));
    assert!(matches!(
        f.remote.set_all_v2ip_device_settings(V2ipDeviceSettings {
            ptp_messages: zero,
            ..all
        }),
        Err(ControlError::InvalidRequest(_))
    ));
}

/// A report reads the settings block by its length: one of 8 bytes, as every
/// device before the stream TTL sends, has no TTL to give, and a shorter one
/// none of the block's values.
#[test]
fn a_settings_block_is_read_by_its_length() {
    let f = Fixture::new();
    let uid = uid_n(241);
    f.everything(uid, 0x2F, "TT0002");
    let cfg = Cfg::addresses(uid, "239.1.2.3");
    let both = V2ipDeviceSetting::STREAM_TTL | V2ipDeviceSetting::SAP_INTERVAL;
    let report = |len: usize| {
        let mut p = cfg.bytes_with_power_save(both.bits(), 0, 0, &V2ipPowerSaveSchedule::default());
        p.resize(len, 0);
        if len > 223 {
            p[222] = 30;
        }
        p
    };
    let valid = || f.remote.v2ip_device_settings(uid).map(|s| s.valid);

    f.feed(uid, op::V2IP_DEVICE_CFG, &report(223));
    assert_eq!(valid(), None, "a block too short for any value");
    f.feed(uid, op::V2IP_DEVICE_CFG, &report(224));
    assert_eq!(valid(), Some(V2ipDeviceSetting::SAP_INTERVAL));
    f.feed(uid, op::V2IP_DEVICE_CFG, &report(232));
    assert_eq!(valid(), Some(both));
}

/// A unit refuses a stream TTL of 0, so another sender's write of one leaves
/// what it reported.
#[test]
fn another_senders_stream_ttl_of_0_changes_nothing() {
    let f = Fixture::new();
    let (uid, controller) = (uid_n(242), uid_n(243));
    f.everything(uid, 0x2F, "TT0003");
    f.feed(
        controller,
        op::SYS_HELLO,
        &hello_payload(0x2F, "Ctrl", "TT0004", "4.8.0", DeviceFeature::MANAGER),
    );
    let cfg = Cfg::addresses(uid, "239.1.2.3");
    let ttl = V2ipDeviceSetting::STREAM_TTL;
    let frame = |value| {
        let mut p = cfg.bytes_with_power_save(ttl.bits(), 0, 0, &V2ipPowerSaveSchedule::default());
        p.resize(232, 0);
        p[224] = value;
        p
    };
    let cached = || {
        f.remote
            .v2ip_device_settings(uid)
            .and_then(|s| s.ptp_messages())
            .map(|m| m.stream_ttl)
    };
    f.feed(uid, op::V2IP_DEVICE_CFG, &frame(32));
    f.feed(controller, op::V2IP_DEVICE_CFG, &frame(0));
    assert_eq!(cached(), Some(32));
    f.feed(controller, op::V2IP_DEVICE_CFG, &frame(20));
    assert_eq!(cached(), Some(20), "a write in range");
}

/// A unit listing a mesh input on port 3 and a mesh output on port 128, the
/// units they stand for, and those units' own bays.
fn mesh_listing() -> (Fixture, BayUid, BayUid, BayUid, BayUid) {
    let f = Fixture::new();
    let (listing, tx, rx) = (uid_n(241), uid_n(242), uid_n(243));
    f.everything(listing, 0x2F, "ML0001");
    f.feed(
        tx,
        op::SYS_HELLO,
        &hello_payload(0x2F, "ONEIP", "ML0002", "4.8.0", DeviceFeature::V2IP_SOURCE),
    );
    f.feed(
        tx,
        op::SYS_BAY_CONFIG,
        &bay_config_rec(
            0,
            0,
            0,
            "Input 1",
            "Kodi",
            BayStatus::NONE,
            BayFeatures::HDMI_IN | BayFeatures::V2IP_SOURCE_LOCAL,
        ),
    );
    f.feed(
        rx,
        op::SYS_HELLO,
        &hello_payload(0x2F, "ONEIP", "ML0003", "4.8.0", DeviceFeature::V2IP_SINK),
    );
    f.feed(
        rx,
        op::SYS_BAY_CONFIG,
        &bay_config_rec(
            127,
            1,
            0,
            "Output 1",
            "Den",
            BayStatus::NONE,
            BayFeatures::HDMI_OUT | BayFeatures::AUDIO_DIG_OUT | BayFeatures::V2IP_SINK_LOCAL,
        ),
    );
    f.feed(rx, op::AUDIO_VOLUME_MUTE, &[127, 30, 30, 0]);
    for (is_input, first, owner) in [(true, 3u16, tx), (false, 128, rx)] {
        let mut page = vec![0u8; 8];
        page[0..2].copy_from_slice(&((1u16 << 1) | u16::from(is_input)).to_le_bytes());
        page[2..4].copy_from_slice(&first.to_le_bytes());
        page.extend_from_slice(owner.as_bytes());
        f.feed(listing, op::V2IP_BAY_MAPPINGS, &page);
    }
    f.tap.clear();
    (
        f,
        BayUid::new(listing, 3),
        BayUid::new(listing, 128),
        BayUid::new(tx, 0),
        BayUid::new(rx, 127),
    )
}

/// The target uid and the port of the frame a call built.
fn sent_to(f: &Fixture, what: &str, call: Result<(), ControlError>) -> (DeviceUid, u16) {
    assert!(
        matches!(call, Err(ControlError::Send(SendError::NotConnected))),
        "{what} was refused rather than sent: {call:?}"
    );
    let frame = f.tap.frames().pop().expect("no frame was built");
    f.tap.clear();
    let p = &frame[HEADER_LEN..];
    let uid = DeviceUid::from_bytes(&p[..16]).expect("a uid");
    (uid, u16::from_le_bytes([p[16], p[17]]))
}

/// A command about a mesh bay goes to the unit it stands for, about that
/// unit's own bay: the unit listing it holds only a copy.
#[test]
fn a_mesh_bay_command_goes_to_the_unit_it_stands_for() {
    let (f, input, output, tx_bay, rx_bay) = mesh_listing();
    let r = &f.remote;
    type Call<'a> = Box<dyn Fn() -> Result<(), ControlError> + 'a>;
    let calls: [(&str, BayUid, Call<'_>); 7] = [
        (
            "rename",
            tx_bay,
            Box::new(|| r.set_bay_name(input, "Lounge")),
        ),
        ("hide", tx_bay, Box::new(|| r.set_bay_hidden(input, true))),
        (
            "action",
            tx_bay,
            Box::new(|| r.send_action(input, RcAction::POWER_ON)),
        ),
        ("key", tx_bay, Box::new(|| r.send_key(input, RcKey::PLAY))),
        ("power", rx_bay, Box::new(|| r.power_on(output))),
        (
            "amp",
            rx_bay,
            Box::new(|| r.set_amp_zone_settings(output, AmpZoneSettings::default())),
        ),
        ("volume step", rx_bay, Box::new(|| r.volume_up(output))),
    ];
    for (what, owner, call) in calls {
        assert_eq!(
            sent_to(&f, what, call()),
            (owner.device, owner.port),
            "{what}"
        );
    }

    // An EDID profile names only the unit.
    let call = r.select_edid_profile(input, EdidProfile::UHD_4K);
    assert_eq!(sent_to(&f, "edid", call).0, tx_bay.device);

    // A change that is sent is kept on the bay it was sent about, and read
    // through the mesh bay.
    f.connect();
    r.set_bay_name(input, "Lounge").expect("sent");
    r.set_bay_hidden(input, true).expect("sent");
    assert_eq!(
        r.bay(input).map(|b| (b.user_name, b.hidden)),
        Some(("Lounge".to_owned(), Some(true)))
    );
}

/// A route for a mesh output names the unit playing it, and its source in
/// the numbering of the unit listing it.
#[test]
fn a_mesh_output_is_routed_on_the_unit_it_stands_for() {
    let (f, _, output, _, rx_bay) = mesh_listing();
    let r = &f.remote;
    let format = Some(V2ipAudioFormat::STANDARD);
    type Call<'a> = Box<dyn Fn() -> Result<(), ControlError> + 'a>;
    let calls: [(&str, Call<'_>); 5] = [
        ("video", Box::new(|| r.select_video_source(output, 1))),
        ("audio", Box::new(|| r.select_audio_source(output, 1))),
        (
            "by name",
            Box::new(|| r.select_audio_source_by_name(output, "Apple TV", format)),
        ),
        (
            "manual",
            Box::new(|| r.select_source_addr(output, route(1), None)),
        ),
        (
            "manual audio",
            Box::new(|| {
                r.select_audio_source_addr(output, Ipv4Addr::new(239, 1, 1, 1), None, None)
            }),
        ),
    ];
    for (what, call) in calls {
        assert_eq!(sent_to(&f, what, call()).0, rx_bay.device, "{what}");
    }
}

/// A mesh bay whose unit has not reported its own bay has nowhere to send a
/// command to.
#[test]
fn a_mesh_bay_without_its_unit_refuses_commands() {
    let (f, input, _, _, _) = mesh_listing();
    let listing = input.device;
    let mut page = vec![0u8; 8];
    page[0..2].copy_from_slice(&((1u16 << 1) | 1).to_le_bytes());
    page[2..4].copy_from_slice(&4u16.to_le_bytes());
    page.extend_from_slice(uid_n(244).as_bytes());
    f.feed(listing, op::V2IP_BAY_MAPPINGS, &page);
    let call = f.remote.set_bay_name(BayUid::new(listing, 4), "Lounge");
    assert!(
        matches!(call, Err(ControlError::NotReported(_))),
        "{call:?}"
    );
    assert!(f.tap.frames().is_empty(), "a frame was built");
}

/// A report of the output's rotation and its step, from a device whose
/// output turns by 90 degrees.
fn rotation_report(cfg: &Cfg, degrees: u16) -> Vec<u8> {
    let rotation = V2ipDeviceSetting::SINK_ROTATION;
    let mut p = cfg.bytes_with_power_save(rotation.bits(), 0, 0, &V2ipPowerSaveSchedule::default());
    p.resize(232, 0);
    p[226..228].copy_from_slice(&degrees.to_le_bytes());
    p[228..230].copy_from_slice(&90u16.to_le_bytes());
    p
}

/// The rotation and its step are read from a block long enough to hold
/// both, and a shorter one gives neither while keeping the stream TTL.
#[test]
fn a_rotation_is_read_from_a_block_that_holds_its_step() {
    let f = Fixture::new();
    let uid = uid_n(248);
    f.everything(uid, 0x30, "RT0001");
    let cfg = Cfg::addresses(uid, "239.1.2.3");
    let both = V2ipDeviceSetting::STREAM_TTL | V2ipDeviceSetting::SINK_ROTATION;
    let report = |len: usize| {
        let mut p = rotation_report(&cfg, 270);
        p[128..132].copy_from_slice(&both.bits().to_le_bytes());
        p[224] = 32;
        p.truncate(len);
        p
    };
    let cached = || {
        f.remote.v2ip_device_settings(uid).map(|s| {
            (
                s.valid,
                s.ptp_messages.sink_rotation,
                s.ptp_messages.sink_rotation_step,
            )
        })
    };

    f.feed(uid, op::V2IP_DEVICE_CFG, &report(229));
    assert_eq!(cached(), Some((V2ipDeviceSetting::STREAM_TTL, 0, 0)));
    f.feed(uid, op::V2IP_DEVICE_CFG, &report(230));
    assert_eq!(cached(), Some((both, 270, 90)));
}

/// A write runs the block on to 16 bytes with the angle at 10 and no step,
/// and is refused unless the device turns its output by that angle.
#[test]
fn a_rotation_write_goes_by_the_devices_step() {
    let f = Fixture::new();
    let uid = uid_n(249);
    f.everything(uid, 0x30, "RT0002");
    let cfg = Cfg::addresses(uid, "239.1.2.3");
    f.feed(uid, op::V2IP_DEVICE_CFG, &rotation_report(&cfg, 0));
    f.connect();
    let rotation = V2ipDeviceSetting::SINK_ROTATION;
    let turned = |degrees| V2ipPtpMessages {
        sink_rotation: degrees,
        sink_rotation_step: 45,
        stream_ttl: 9,
        ..V2ipPtpMessages::default()
    };

    f.tap.clear();
    f.remote
        .set_v2ip_ptp_messages(uid, rotation, turned(180))
        .expect("a multiple of the step");
    let frame = f.tap.frames().pop().expect("nothing reached the gate");
    let p = &frame[HEADER_LEN..];
    assert_eq!(p.len(), 232);
    assert_eq!(p[224], 0, "a TTL the write does not carry");
    assert_eq!(&p[226..230], &[180, 0, 0, 0]);
    let cached = f.remote.v2ip_device_settings(uid).map(|s| {
        (
            s.ptp_messages.sink_rotation,
            s.ptp_messages.sink_rotation_step,
        )
    });
    assert_eq!(cached, Some((180, 90)), "the step is the device's");

    for degrees in [45, 360] {
        assert!(
            matches!(
                f.remote
                    .set_v2ip_ptp_messages(uid, rotation, turned(degrees)),
                Err(ControlError::InvalidRequest(_))
            ),
            "{degrees} was sent"
        );
    }

    f.tap.clear();
    let all = |degrees| V2ipDeviceSettings {
        valid: rotation,
        ptp_messages: turned(degrees),
        ..V2ipDeviceSettings::default()
    };
    f.remote
        .set_all_v2ip_device_settings(all(45))
        .expect("each device judges the step");
    let frame = f.tap.frames().pop().expect("nothing reached the gate");
    let p = &frame[HEADER_LEN..];
    assert_eq!((p.len(), &p[58..62]), (64, &[45, 0, 0, 0][..]));
    assert!(matches!(
        f.remote.set_all_v2ip_device_settings(all(360)),
        Err(ControlError::InvalidRequest(_))
    ));
}

/// Another sender's write is cached only by the device's step, which it
/// leaves as the device reported it.
#[test]
fn another_senders_rotation_is_kept_by_the_devices_step() {
    let f = Fixture::new();
    let (uid, controller) = (uid_n(250), uid_n(251));
    f.everything(uid, 0x30, "RT0003");
    f.feed(
        controller,
        op::SYS_HELLO,
        &hello_payload(0x30, "Ctrl", "RT0004", "4.8.0", DeviceFeature::MANAGER),
    );
    let cfg = Cfg::addresses(uid, "239.1.2.3");
    let write = |degrees: u16| {
        let mut p = rotation_report(&cfg, degrees);
        p[228..230].fill(0);
        p
    };
    let cached = || {
        f.remote.v2ip_device_settings(uid).map(|s| {
            (
                s.ptp_messages.sink_rotation,
                s.ptp_messages.sink_rotation_step,
            )
        })
    };
    f.feed(uid, op::V2IP_DEVICE_CFG, &rotation_report(&cfg, 90));
    f.feed(controller, op::V2IP_DEVICE_CFG, &write(45));
    assert_eq!(cached(), Some((90, 90)));
    f.feed(controller, op::V2IP_DEVICE_CFG, &write(180));
    assert_eq!(cached(), Some((180, 90)));
}

/// A device whose output cannot turn reports a step of 0, and takes only 0.
#[test]
fn an_output_that_cannot_turn_takes_only_0() {
    let f = Fixture::new();
    let uid = uid_n(252);
    f.everything(uid, 0x30, "RT0005");
    let mut p = rotation_report(&Cfg::addresses(uid, "239.1.2.3"), 0);
    p[228..230].fill(0);
    f.feed(uid, op::V2IP_DEVICE_CFG, &p);
    f.connect();
    let rotation = V2ipDeviceSetting::SINK_ROTATION;
    let turned = |degrees| V2ipPtpMessages {
        sink_rotation: degrees,
        ..V2ipPtpMessages::default()
    };
    assert!(matches!(
        f.remote.set_v2ip_ptp_messages(uid, rotation, turned(90)),
        Err(ControlError::InvalidRequest(_))
    ));
    f.remote
        .set_v2ip_ptp_messages(uid, rotation, turned(0))
        .expect("0 is every device's");
}

/// A device's settings frame reporting its stream TTL and, behind the second
/// valid word, its downmix: mode 1, level 1, LFE 70, state 3 and 6 channels.
fn downmix_report(cfg: &Cfg, valid_ext: u32) -> Vec<u8> {
    let ttl = V2ipDeviceSetting::STREAM_TTL;
    let mut p = cfg.bytes_with_power_save(ttl.bits(), 0, 0, &V2ipPowerSaveSchedule::default());
    p.resize(248, 0);
    p[224] = 64;
    p[232..236].copy_from_slice(&valid_ext.to_le_bytes());
    p[236..241].copy_from_slice(&[1, 1, 70, 3, 6]);
    p
}

/// The downmix [`downmix_report`] reports.
const REPORTED_DOWNMIX: V2ipDownmix = V2ipDownmix {
    mode: V2ipDownmixMode::AUTO,
    level: V2ipDownmixLevel::UNSCALED,
    lfe: 70,
    state: 3,
    channels: 6,
};

/// A device's own report is read from a block that holds the downmix's last
/// field, and one that holds it without the bit says the device has none.
#[test]
fn a_downmix_is_read_from_a_block_that_holds_it() {
    let f = Fixture::new();
    let uid = uid_n(253);
    f.everything(uid, 0x30, "DM0001");
    let cfg = Cfg::addresses(uid, "239.1.2.3");
    let report = |valid_ext: u32, len: usize| {
        let mut p = downmix_report(&cfg, valid_ext);
        p.truncate(len);
        p
    };

    f.feed(uid, op::V2IP_DEVICE_CFG, &report(1, 240));
    assert_eq!(f.remote.v2ip_downmix(uid), None);
    f.feed(uid, op::V2IP_DEVICE_CFG, &report(1, 241));
    assert_eq!(f.remote.v2ip_downmix(uid), Some(REPORTED_DOWNMIX));
    f.feed(uid, op::V2IP_DEVICE_CFG, &report(1, 240));
    assert_eq!(
        f.remote.v2ip_downmix(uid),
        Some(REPORTED_DOWNMIX),
        "a block too short to say"
    );
    f.feed(uid, op::V2IP_DEVICE_CFG, &report(0, 241));
    assert_eq!(f.remote.v2ip_downmix(uid), None);
}

/// A write carries the second valid word and the three settings, and no bit
/// of the first, and leaves the state and channels to the device.
#[test]
fn a_downmix_write_carries_its_three_settings() {
    let f = Fixture::new();
    let uid = uid_n(253);
    f.everything(uid, 0x30, "DM0002");
    let cfg = Cfg::addresses(uid, "239.1.2.3");
    f.feed(uid, op::V2IP_DEVICE_CFG, &downmix_report(&cfg, 1));
    f.connect();
    let mixed = V2ipDownmix {
        mode: V2ipDownmixMode::ALWAYS,
        level: V2ipDownmixLevel::UNSCALED,
        lfe: 100,
        state: 9,
        channels: 9,
    };

    f.tap.clear();
    f.remote
        .set_v2ip_downmix(uid, mixed)
        .expect("a device that mixes down");
    let frame = f.tap.frames().pop().expect("nothing reached the gate");
    let p = &frame[HEADER_LEN..];
    assert_eq!(p.len(), 248);
    assert_eq!(&p[..16], uid.as_bytes());
    assert_eq!(&p[128..132], &[0; 4], "a setting of the first valid word");
    assert_eq!(&p[232..241], &[1, 0, 0, 0, 2, 1, 100, 0, 0]);
    assert_eq!(
        f.remote.v2ip_downmix(uid),
        Some(V2ipDownmix {
            state: 3,
            channels: 6,
            ..mixed
        }),
        "the state and channels are the device's"
    );
}

/// A downmix out of range, or for a device that reports none, is refused
/// before anything is sent.
#[test]
fn a_downmix_the_device_would_not_take_is_not_sent() {
    let f = Fixture::new();
    let (mixes, cannot) = (uid_n(253), uid_n(255));
    f.everything(mixes, 0x30, "DM0003");
    f.everything(cannot, 0x30, "DM0004");
    f.feed(
        mixes,
        op::V2IP_DEVICE_CFG,
        &downmix_report(&Cfg::addresses(mixes, "239.1.2.3"), 1),
    );
    f.feed(
        cannot,
        op::V2IP_DEVICE_CFG,
        &downmix_report(&Cfg::addresses(cannot, "239.1.2.4"), 0),
    );
    f.tap.clear();

    for refused in [
        V2ipDownmix {
            mode: V2ipDownmixMode::from_wire(3),
            ..REPORTED_DOWNMIX
        },
        V2ipDownmix {
            level: V2ipDownmixLevel::from_wire(2),
            ..REPORTED_DOWNMIX
        },
        V2ipDownmix {
            lfe: 101,
            ..REPORTED_DOWNMIX
        },
    ] {
        assert!(
            matches!(
                f.remote.set_v2ip_downmix(mixes, refused),
                Err(ControlError::InvalidRequest(_))
            ),
            "{refused:?} was sent"
        );
    }
    assert!(matches!(
        f.remote.set_v2ip_downmix(cannot, REPORTED_DOWNMIX),
        Err(ControlError::Unsupported(_))
    ));
    assert!(f.tap.frames().is_empty());
}

/// Another sender's write is applied only to a device that reports a
/// downmix, and keeps the state and channels it reported.
#[test]
fn another_senders_downmix_keeps_the_devices_state() {
    let f = Fixture::new();
    let (uid, controller) = (uid_n(253), uid_n(255));
    f.everything(uid, 0x30, "DM0005");
    f.feed(
        controller,
        op::SYS_HELLO,
        &hello_payload(0x30, "Ctrl", "DM0006", "4.8.0", DeviceFeature::MANAGER),
    );
    let cfg = Cfg::addresses(uid, "239.1.2.3");
    let mut write = downmix_report(&cfg, 1);
    write[236..241].copy_from_slice(&[2, 0, 10, 0, 0]);

    f.feed(uid, op::V2IP_DEVICE_CFG, &downmix_report(&cfg, 0));
    f.feed(controller, op::V2IP_DEVICE_CFG, &write);
    assert_eq!(f.remote.v2ip_downmix(uid), None, "a device that has none");

    f.feed(uid, op::V2IP_DEVICE_CFG, &downmix_report(&cfg, 1));
    let mut unflagged = write.clone();
    unflagged[232..236].fill(0);
    f.feed(controller, op::V2IP_DEVICE_CFG, &unflagged);
    assert_eq!(
        f.remote.v2ip_downmix(uid),
        Some(REPORTED_DOWNMIX),
        "a write without the bit"
    );
    f.feed(controller, op::V2IP_DEVICE_CFG, &write);
    assert_eq!(
        f.remote.v2ip_downmix(uid),
        Some(V2ipDownmix {
            mode: V2ipDownmixMode::ALWAYS,
            level: V2ipDownmixLevel::NORMALISED,
            lfe: 10,
            ..REPORTED_DOWNMIX
        })
    );
}

/// A third-party message goes to every device at the lowest stamp, with the
/// flag byte, the three identifying bytes and the payload as given.
#[test]
fn a_third_party_message_goes_out_as_given() {
    use crate::types::{ThirdPartyVendor, THIRD_PARTY_PAYLOAD_MAX};
    use crate::wire::ThirdPartyVendorFlag;

    let f = Fixture::new();
    f.tap.clear();
    let body = crate::testing::poisoned(41);
    let mut vendor = ThirdPartyVendor::private(*b"ODK");
    vendor.flags |= ThirdPartyVendorFlag::from_bits(0x40);
    let _ = f.remote.send_third_party(vendor, &body);
    let frame = f.tap.frames().pop().expect("nothing reached the gate");
    assert_eq!(&frame[20..22], &[0x55, 0x00], "the opcode");
    assert_eq!(&frame[2..4], &[0x01, 0x00], "the stamp");
    let p = &frame[HEADER_LEN..];
    assert_eq!(&p[..4], &[0x41, b'O', b'D', b'K']);
    assert_eq!(&p[4..], &body[..]);

    f.tap.clear();
    let _ = f
        .remote
        .send_third_party(ThirdPartyVendor::oui([0, 0x1A, 0x2B]), &[]);
    let frame = f.tap.frames().pop().expect("an empty payload was refused");
    assert_eq!(&frame[HEADER_LEN..], &[0, 0, 0x1A, 0x2B]);

    f.tap.clear();
    let _ = f
        .remote
        .send_third_party(vendor, &vec![0; THIRD_PARTY_PAYLOAD_MAX]);
    let frame = f
        .tap
        .frames()
        .pop()
        .expect("the largest payload was refused");
    assert_eq!(frame.len() + 28, 1500, "the largest frame fills the MTU");

    f.tap.clear();
    assert!(matches!(
        f.remote
            .send_third_party(vendor, &vec![0; THIRD_PARTY_PAYLOAD_MAX + 1]),
        Err(ControlError::InvalidRequest(_))
    ));
    assert!(f.tap.frames().is_empty(), "an oversized message was sent");
}

/// Asking an output for its description sets bit 0 of the request's value,
/// and is refused below 0x31, where an output ignores the bit.
#[test]
fn asking_for_the_description_sets_the_request_bit() {
    let f = Fixture::new();
    let uid = uid_n(241);
    f.everything(uid, 0x31, "SD0005");
    f.connect();
    f.tap.clear();
    f.remote.request_v2ip_sink_description(uid).expect("sent");
    let frame = f.tap.frames().pop().expect("nothing reached the gate");
    assert_eq!(&frame[20..22], &op::V2IP_SINK_SDP.0.to_le_bytes());
    assert_eq!(&frame[2..4], &0x2Eu16.to_le_bytes(), "the stamp");
    let p = &frame[HEADER_LEN..];
    assert_eq!(&p[..16], uid.as_bytes());
    assert_eq!(&p[16..20], &[4, 1, 0, 0], "a request for the description");

    let old = uid_n(242);
    f.everything(old, 0x30, "SD0006");
    f.tap.clear();
    assert!(refused(&f.remote.request_v2ip_sink_description(old)));
    assert!(f.tap.frames().is_empty(), "a refused request was sent");
}

/// A unit is located for 30 s or until stopped, only once it has reported
/// that it can be, and the cached status says so at once.
#[test]
fn a_unit_is_located_only_once_it_says_it_can_be() {
    let f = Fixture::new();
    let uid = uid_n(243);
    f.everything(uid, 0x31, "LC0001");
    f.connect();
    f.tap.clear();
    assert!(matches!(
        f.remote.locate_unit(uid, true),
        Err(ControlError::NotReported(_))
    ));
    let report = |flags: u32| {
        let mut p = flags.to_le_bytes().to_vec();
        p.resize(104, 0);
        f.feed(uid, op::SYS_UNIT_STATUS, &p);
    };
    report(UnitFlag::FAN_SPEED.bits());
    assert!(matches!(
        f.remote.locate_unit(uid, true),
        Err(ControlError::Unsupported(_))
    ));
    assert!(matches!(
        f.remote.stop_locating_unit(uid),
        Err(ControlError::Unsupported(_))
    ));
    assert!(f.tap.frames().is_empty(), "a refused locate was sent");

    report((UnitFlag::LOCATE_SUPPORTED | UnitFlag::FAN_SPEED).bits());
    let flags = |f: &Fixture| f.remote.unit_status(uid).expect("reported").flags;
    let sent = |f: &Fixture| {
        let frame = f.tap.frames().pop().expect("nothing reached the gate");
        assert_eq!(&frame[20..22], &op::SYS_UNIT_COMMAND.0.to_le_bytes());
        let p = &frame[HEADER_LEN..];
        assert_eq!(&p[..16], uid.as_bytes());
        [p[16], p[17]]
    };
    f.remote.locate_unit(uid, true).expect("sent");
    assert_eq!(sent(&f), [4, 0x08], "timed");
    assert!(flags(&f).has(UnitFlag::LOCATE));
    f.remote.locate_unit(uid, false).expect("sent");
    assert_eq!(sent(&f), [4, 0x04], "until stopped");
    f.remote.stop_locating_unit(uid).expect("sent");
    assert_eq!(sent(&f), [4, 0x00], "stop");
    assert_eq!(
        flags(&f),
        UnitFlag::LOCATE_SUPPORTED | UnitFlag::FAN_SPEED,
        "a stop clears the flag and nothing else"
    );
}
