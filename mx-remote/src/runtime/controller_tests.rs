// Author: Lars Op den Kamp (lars@opdenkamp-it.nl)
// Copyright (c) 2026 Op den Kamp IT Solutions

//! `CONTROLLER_STATE` between two dedicated controllers: serving the parts of
//! a state, and pulling them window by window (routers.md).

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::testing::uid_n;
use crate::wire::{crc32, StateHeader, HEADER_LEN};

use super::*;

const FROM: Ipv4Addr = Ipv4Addr::new(10, 8, 8, 9);
const OP: u16 = 0x58;

/// Records every frame a client sends.
#[derive(Default)]
struct Tap(Mutex<Vec<Vec<u8>>>);

impl Tap {
    fn frames(&self) -> Vec<Vec<u8>> {
        self.0.lock().expect("tap").clone()
    }

    fn clear(&self) {
        self.0.lock().expect("tap").clear();
    }
}

/// A part received: its device, part, generation and bytes.
type Received = (DeviceUid, u8, u32, Vec<u8>);

/// The parts a client received or gave up on.
#[derive(Default)]
struct Parts {
    received: Mutex<Vec<Received>>,
    failed: Mutex<Vec<(DeviceUid, u8, u32)>>,
}

impl EventHandler for Parts {
    fn on_controller_part_received(
        &self,
        device: DeviceUid,
        part: u8,
        generation: u32,
        bytes: Vec<u8>,
    ) {
        self.received
            .lock()
            .expect("test handler")
            .push((device, part, generation, bytes));
    }

    fn on_controller_part_failed(&self, device: DeviceUid, part: u8, generation: u32) {
        self.failed
            .lock()
            .expect("test handler")
            .push((device, part, generation));
    }
}

fn dedicated(n: u8, handler: Arc<dyn EventHandler>) -> (Remote, Arc<Tap>) {
    let remote = Remote::new(
        Config {
            uid: Some(uid_n(n)),
            dedicated_controller: true,
            ..Config::default()
        },
        handler,
    )
    .expect("a client given its own uid reads nothing from disk");
    let tap = Arc::new(Tap::default());
    let sink = Arc::clone(&tap);
    lock(&remote.shared.tx).set_tap(Arc::new(move |frame: &[u8]| {
        sink.0.lock().expect("tap").push(frame.to_vec())
    }));
    (remote, tap)
}

/// The `CONTROLLER_STATE` frames a client sent since the last call, as their
/// header and part bytes.
fn state_frames(tap: &Tap) -> Vec<Vec<u8>> {
    let frames = tap.frames();
    tap.clear();
    frames
        .into_iter()
        .filter(|f| u16::from_le_bytes([f[20], f[21]]) == OP)
        .collect()
}

fn header(frame: &[u8]) -> StateHeader {
    StateHeader::decode(&frame[HEADER_LEN..]).expect("a header")
}

fn deliver(frames: &[Vec<u8>], to: &Remote) {
    for frame in frames {
        to.shared.process_datagram(frame, FROM);
    }
}

struct Pair {
    primary: Remote,
    primary_tap: Arc<Tap>,
    failsafe: Remote,
    failsafe_tap: Arc<Tap>,
    parts: Arc<Parts>,
}

impl Pair {
    /// A primary leading its mesh, and a failsafe following it, each having
    /// had the other's hello and controller hello.
    fn new() -> Self {
        let (primary, primary_tap) = dedicated(220, Arc::new(()));
        let parts = Arc::new(Parts::default());
        let (failsafe, failsafe_tap) = dedicated(221, Arc::clone(&parts) as Arc<dyn EventHandler>);
        primary.shared.announce();
        failsafe.shared.announce();
        deliver(&primary_tap.frames(), &failsafe);
        deliver(&failsafe_tap.frames(), &primary);
        primary_tap.clear();
        failsafe_tap.clear();
        primary.set_mesh_controller(primary.uid());
        failsafe.set_mesh_controller(primary.uid());
        // The primary knows the failsafe follows it from its membership
        // report; here it is told outright.
        lock(&primary.shared.state)
            .devices
            .get_mut(&failsafe.uid())
            .expect("known")
            .mesh_master = primary.uid();
        primary_tap.clear();
        failsafe_tap.clear();
        Self {
            primary,
            primary_tap,
            failsafe,
            failsafe_tap,
            parts,
        }
    }

    /// Passes requests and data between the two until neither sends more.
    fn pump(&self) {
        for _ in 0..64 {
            let asks = state_frames(&self.failsafe_tap);
            let data = state_frames(&self.primary_tap);
            if asks.is_empty() && data.is_empty() {
                return;
            }
            deliver(&asks, &self.primary);
            deliver(&data, &self.failsafe);
        }
        panic!("the pull never settled");
    }

    fn received(&self) -> Vec<Received> {
        self.parts.received.lock().expect("test handler").clone()
    }

    fn failed(&self) -> Vec<(DeviceUid, u8, u32)> {
        self.parts.failed.lock().expect("test handler").clone()
    }

    fn close(self) {
        self.primary.close();
        self.failsafe.close();
    }
}

fn state(len: usize) -> Vec<u8> {
    (0..len).map(|i| (i * 7 % 251) as u8).collect()
}

#[test]
fn a_part_is_pulled_in_windows_of_8192_bytes_in_frames_of_1024() {
    let pair = Pair::new();
    let bytes = state(20_000);
    pair.primary
        .set_controller_parts(3, vec![(0, bytes.clone())])
        .expect("parts");
    state_frames(&pair.primary_tap);
    pair.failsafe
        .pull_controller_part(pair.primary.uid(), 0, 3, 20_000, crc32(&bytes))
        .expect("pulling");

    let asks = state_frames(&pair.failsafe_tap);
    assert_eq!(asks.len(), 1);
    let ask = header(&asks[0]);
    assert_eq!((ask.kind, ask.part, ask.generation), (1, 0, 3));
    assert_eq!((ask.offset, ask.length), (0, 8192));
    deliver(&asks, &pair.primary);
    let data = state_frames(&pair.primary_tap);
    assert_eq!(data.len(), 8, "the window in frames of 1024");
    for (i, frame) in data.iter().enumerate() {
        let h = header(frame);
        assert_eq!((h.kind, h.offset, h.length), (2, i as u32 * 1024, 20_000));
        assert_eq!(h.crc, crc32(&bytes));
        assert_eq!(frame.len() - HEADER_LEN - 20, 1024);
    }
    deliver(&data, &pair.failsafe);
    let next = state_frames(&pair.failsafe_tap);
    assert_eq!(next.len(), 1, "the next window once this one is in");
    assert_eq!(
        (header(&next[0]).offset, header(&next[0]).length),
        (8192, 8192)
    );
    deliver(&next, &pair.primary);
    pair.pump();

    assert_eq!(pair.received(), [(pair.primary.uid(), 0, 3, bytes)]);
    assert!(pair.failed().is_empty());
    pair.close();
}

#[test]
fn a_lost_frame_ends_the_window_and_it_is_asked_again_from_there_once() {
    let pair = Pair::new();
    let bytes = state(5000);
    pair.primary
        .set_controller_parts(1, vec![(0, bytes.clone())])
        .expect("parts");
    state_frames(&pair.primary_tap);
    pair.failsafe
        .pull_controller_part(pair.primary.uid(), 0, 1, 5000, crc32(&bytes))
        .expect("pulling");
    deliver(&state_frames(&pair.failsafe_tap), &pair.primary);
    let mut data = state_frames(&pair.primary_tap);
    assert_eq!(data.len(), 5);
    data.remove(1);
    deliver(&data, &pair.failsafe);
    let asks = state_frames(&pair.failsafe_tap);
    assert_eq!(asks.len(), 1, "once, however many frames followed the gap");
    assert_eq!(header(&asks[0]).offset, 1024);
    deliver(&asks, &pair.primary);
    pair.pump();
    assert_eq!(pair.received(), [(pair.primary.uid(), 0, 1, bytes)]);
    pair.close();
}

#[test]
fn a_pull_asks_again_after_2_s_without_data() {
    let pair = Pair::new();
    let bytes = state(100);
    pair.failsafe
        .pull_controller_part(pair.primary.uid(), 0, 1, 100, crc32(&bytes))
        .expect("pulling");
    assert_eq!(state_frames(&pair.failsafe_tap).len(), 1);
    let now = Instant::now();
    pair.failsafe
        .shared
        .probe_once(now + Duration::from_secs(1));
    assert!(state_frames(&pair.failsafe_tap).is_empty(), "within 2 s");
    pair.failsafe
        .shared
        .probe_once(now + Duration::from_secs(3));
    let asks = state_frames(&pair.failsafe_tap);
    assert_eq!(asks.len(), 1);
    assert_eq!(header(&asks[0]).offset, 0);
    pair.close();
}

#[test]
fn a_pull_gives_up_after_ten_asks_without_progress() {
    let pair = Pair::new();
    pair.failsafe
        .pull_controller_part(pair.primary.uid(), 0, 1, 100, 0x1234)
        .expect("pulling");
    // Kept online by its hellos, which keep coming.
    let mut now = Instant::now();
    for _ in 0..11 {
        now += Duration::from_secs(2);
        lock(&pair.failsafe.shared.state)
            .devices
            .get_mut(&pair.primary.uid())
            .expect("known")
            .last_ping = now;
        pair.failsafe.shared.probe_once(now);
    }
    assert_eq!(
        state_frames(&pair.failsafe_tap).len(),
        11,
        "the first and ten more"
    );
    assert_eq!(pair.failed(), [(pair.primary.uid(), 0, 1)]);
    pair.close();
}

#[test]
fn a_generation_no_longer_held_ends_the_pull() {
    let pair = Pair::new();
    let bytes = state(3000);
    pair.primary
        .set_controller_parts(1, vec![(0, bytes.clone())])
        .expect("parts");
    pair.primary
        .set_controller_parts(2, vec![(0, state(10))])
        .expect("parts");
    state_frames(&pair.primary_tap);
    pair.failsafe
        .pull_controller_part(pair.primary.uid(), 0, 1, 3000, crc32(&bytes))
        .expect("pulling");
    deliver(&state_frames(&pair.failsafe_tap), &pair.primary);
    let data = state_frames(&pair.primary_tap);
    assert_eq!(data.len(), 1, "one frame without bytes");
    let h = header(&data[0]);
    assert_eq!((h.generation, data[0].len() - HEADER_LEN), (2, 20));
    deliver(&data, &pair.failsafe);
    assert_eq!(pair.failed(), [(pair.primary.uid(), 0, 1)]);
    assert!(state_frames(&pair.failsafe_tap).is_empty());
    pair.close();
}

#[test]
fn a_part_other_than_the_one_asked_for_ends_the_pull() {
    let pair = Pair::new();
    pair.primary
        .set_controller_parts(1, vec![(0, state(300))])
        .expect("parts");
    state_frames(&pair.primary_tap);
    pair.failsafe
        .pull_controller_part(pair.primary.uid(), 0, 1, 300, 0xBAD)
        .expect("pulling");
    pair.pump();
    assert_eq!(pair.failed(), [(pair.primary.uid(), 0, 1)]);
    assert!(pair.received().is_empty());
    pair.close();
}

#[test]
fn an_empty_part_arrives_at_once() {
    let pair = Pair::new();
    pair.failsafe
        .pull_controller_part(pair.primary.uid(), 4, 9, 0, crc32(&[]))
        .expect("pulling");
    assert!(state_frames(&pair.failsafe_tap).is_empty());
    assert_eq!(pair.received(), [(pair.primary.uid(), 4, 9, Vec::new())]);
    pair.close();
}

/// A request past the end, and one for a part not held, get one frame with
/// no bytes; a window is never more than 8192 bytes.
#[test]
fn requests_are_answered_with_at_most_the_window_and_nothing_past_the_end() {
    let pair = Pair::new();
    pair.primary
        .set_controller_parts(5, vec![(0, state(9000)), (2, state(10))])
        .expect("parts");
    state_frames(&pair.primary_tap);
    let failsafe = pair.failsafe.uid();
    let ask = |part: u8, offset: u32, length: u32| {
        let payload = crate::wire::build_controller_request(part, 5, offset, length);
        let frame = crate::testing::datagram(failsafe, op::CONTROLLER_STATE, 0x34, &payload);
        pair.primary.shared.process_datagram(&frame, FROM);
        state_frames(&pair.primary_tap)
    };
    let answer = ask(0, 0, 20_000);
    assert_eq!(answer.len(), 8, "8192 bytes at most");
    let answer = ask(0, 8192, 8192);
    assert_eq!(answer.len(), 1);
    assert_eq!(answer[0].len() - HEADER_LEN - 20, 9000 - 8192);
    let answer = ask(0, 9000, 100);
    assert_eq!(answer.len(), 1);
    assert_eq!(answer[0].len() - HEADER_LEN, 20, "past the end");
    assert_eq!(header(&answer[0]).length, 9000);
    let answer = ask(3, 0, 100);
    assert_eq!(answer.len(), 1, "a part not held");
    assert_eq!(header(&answer[0]).length, 0);
    let answer = ask(2, 4, 100);
    assert_eq!(answer.len(), 1);
    assert_eq!(&answer[0][HEADER_LEN + 20..], &state(10)[4..]);
    pair.close();
}

/// A request is answered only for a dedicated controller of this client's
/// mesh, or the one allowed.
#[test]
fn a_request_from_outside_the_mesh_is_answered_only_once_allowed() {
    let pair = Pair::new();
    pair.primary
        .set_controller_parts(1, vec![(0, state(10))])
        .expect("parts");
    let (stranger, stranger_tap) = dedicated(222, Arc::new(()));
    stranger.shared.announce();
    deliver(&stranger_tap.frames(), &pair.primary);
    state_frames(&pair.primary_tap);
    let payload = crate::wire::build_controller_request(0, 1, 0, 10);
    let frame = crate::testing::datagram(stranger.uid(), op::CONTROLLER_STATE, 0x34, &payload);
    pair.primary.shared.process_datagram(&frame, FROM);
    assert!(state_frames(&pair.primary_tap).is_empty(), "of no mesh");
    pair.primary.allow_controller_state(Some(stranger.uid()));
    pair.primary.shared.process_datagram(&frame, FROM);
    assert_eq!(state_frames(&pair.primary_tap).len(), 1, "allowed");
    pair.primary.allow_controller_state(None);
    pair.primary.shared.process_datagram(&frame, FROM);
    assert!(state_frames(&pair.primary_tap).is_empty(), "no longer");
    stranger.close();
    pair.close();
}

/// A request from a device that sent no controller hello is dropped, even
/// from the mesh's own controller.
#[test]
fn a_request_from_a_sender_that_sent_no_controller_hello_is_dropped() {
    let (primary, primary_tap) = dedicated(223, Arc::new(()));
    let peer = uid_n(224);
    primary
        .shared
        .process_datagram(&super::tests::hello_datagram(peer, "Unit", "PB0224"), FROM);
    primary.set_mesh_controller(peer);
    primary
        .set_controller_parts(1, vec![(0, state(10))])
        .expect("parts");
    primary_tap.clear();
    let payload = crate::wire::build_controller_request(0, 1, 0, 10);
    let frame = crate::testing::datagram(peer, op::CONTROLLER_STATE, 0x34, &payload);
    primary.shared.process_datagram(&frame, FROM);
    assert!(state_frames(&primary_tap).is_empty());
    primary.close();
}

/// Parts the hello can't list, or lists ambiguously, are refused.
#[test]
fn controller_parts_past_five_or_listed_twice_are_refused() {
    let (remote, _) = dedicated(212, Arc::new(()));
    assert!(remote
        .set_controller_parts(1, (0..6).map(|p| (p, vec![p])).collect())
        .is_err());
    assert!(remote
        .set_controller_parts(1, vec![(1, vec![1]), (1, vec![2])])
        .is_err());
    assert!(remote
        .set_controller_parts(1, (0..5).map(|p| (p, vec![p])).collect())
        .is_ok());
    remote.close();
}
