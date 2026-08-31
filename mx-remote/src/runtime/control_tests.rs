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
use crate::testing::{bay_config_rec, datagram, hello_payload, stream_rec, uid_n};
use crate::types::{
    ActionTransmitRequest, AmpZoneSettings, AudioChangeSource, BayNameChange, EdidProfileChange,
    EdidRequest, KeyTransmitRequest, V2ipAudioFormat, V2ipRoute, V2ipRouteTarget,
};
use crate::wire::{
    build_amp_zone_settings, build_v2ip_manual_source_switch, op, protocol_for, Addressee,
    BayFeatures, BayStatus, BayUid, Conn, DeviceFeature, DeviceUid, EdidProfile, MultiviewerSource,
    MultiviewerViewMode, Opcode, RcAction, RcKey, SendError, StreamAddr, V2ipStreams, HEADER_LEN,
    PROTOCOL_VERSION, V2IP_AUDIO_DEFAULT_CHANNELS, V2IP_AUDIO_DEFAULT_SAMPLE_RATE, V2IP_PORT_ANC,
    V2IP_PORT_AUDIO, V2IP_PORT_VIDEO,
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

    /// Announces a unit that is every kind of thing a guarded send addresses:
    /// a V2IP source on port 1, a V2IP sink and amplifier zone on port 2, a
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
                    | DeviceFeature::AUDIO_ROUTING,
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
    ("set_audio_endpoint_muted", |r, d| {
        r.set_audio_endpoint_muted(d, 1, true)
    }),
    ("select_audio_endpoint_input", |r, d| {
        r.select_audio_endpoint_input(d, 1, d, 2)
    }),
    ("set_multiviewer_view_mode", |r, d| {
        r.set_multiviewer_view_mode(d, MultiviewerViewMode::PIP)
    }),
    ("set_multiviewer_video_source", |r, d| {
        r.set_multiviewer_video_source(d, 1, MultiviewerSource::from_wire(2))
    }),
    ("multiviewer_auto_route", |r, d| r.multiviewer_auto_route(d)),
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
    f.feed(sender, op::V2IP_MANUAL_SRC_SWITCH, &payload);

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
/// trip only proves something if it asserts against the one the handler
/// actually updates.
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

    // Keyed off whoever sent the frame, so it lands on the peer's bay.
    round_trip("set_volume", f.remote.set_volume(out, 37, Some(false)));
    let peer_out = f.remote.bay(BayUid::new(peer, 2)).expect("no peer bay");
    assert_eq!(
        peer_out.volume.and_then(|v| v.volume_left),
        Some(37),
        "volume did not round trip onto the sender's bay"
    );
    assert_ne!(
        f.remote
            .bay(out)
            .expect("no bay")
            .volume
            .and_then(|v| v.volume_left),
        Some(37),
        "volume also landed on the addressed bay; this handler is sender-keyed"
    );
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

#[test]
fn a_multiviewer_frame_is_stamped_above_its_opcodes_floor() {
    let f = Fixture::new();
    let uid = uid_n(196);
    f.everything(uid, 0x28, "MV0001");

    f.tap.clear();
    let _ = f
        .remote
        .set_multiviewer_view_mode(uid, MultiviewerViewMode::PIP);
    let frame = f.tap.frames().pop().expect("nothing reached the gate");

    // This opcode is stamped at 0x20 rather than the 0x16 its own table
    // declares, and the gate has to check against what is stamped: a receiver
    // drops whatever is above its version, whichever number the table holds.
    let stamped = u16::from_le_bytes([frame[2], frame[3]]);
    assert_eq!(stamped, 0x20);
    assert!(stamped > protocol_for(op::V2IP_MULTIVIEWER));
    assert_eq!(frame.len(), HEADER_LEN + 25);
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
    f.everything(peer, 0x28, "EP0003");

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

    let mapped = uid_n(60);
    let mut config_source = mapped.as_bytes().to_vec();
    config_source.push(3);
    config_source.extend_from_slice(&[0; 7]);

    // The sub-opcodes are written out rather than read from `mv_sub`, which is
    // the table under test: comparing it against itself would hold for any
    // value in it. These are `MultiviewerOpcode` in the reference Python
    // library, which is where the numbering comes from.
    //
    // Window two throughout, which the status report numbers 2 and these
    // commands 1. Every other parameter differs from its neighbour, so a
    // transposed pair shows.
    type Call = fn(&Remote, DeviceUid) -> Result<(), ControlError>;
    let calls: Vec<(&str, u8, Vec<u8>, Call)> = vec![
        (
            "view_mode",
            1,
            vec![MultiviewerViewMode::PIP.to_wire()],
            |r, d| r.set_multiviewer_view_mode(d, MultiviewerViewMode::PIP),
        ),
        ("video_source", 2, vec![1, 2], |r, d| {
            r.set_multiviewer_video_source(d, 1, MultiviewerSource::from_wire(2))
        }),
        ("audio_source", 3, vec![1], |r, d| {
            r.set_multiviewer_audio_source(d, MultiviewerSource::from_wire(2))
        }),
        ("audio_volume", 4, vec![70, 1], |r, d| {
            r.set_multiviewer_audio_volume(d, 70, true)
        }),
        ("remote_control", 6, vec![1], |r, d| {
            r.set_multiviewer_remote_control(d, MultiviewerSource::from_wire(2))
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

    let mut dolby = vec![0u8; 18];
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
