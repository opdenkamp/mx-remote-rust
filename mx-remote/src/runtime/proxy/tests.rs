// Author: Lars Op den Kamp (lars@opdenkamp-it.nl)
// Copyright (c) 2026 Op den Kamp IT Solutions

//! The link to a driver proxy, against the contract's published fixtures
//! (§8) where they reach, and against frames sealed the same way where they
//! do not.

use std::net::Ipv4Addr;
use std::time::{Duration, Instant};

use super::crypto::tests::{hex, key, nc, nd, UC, UD};
use super::crypto::{seal, session_key};
use super::link::{Link, Out, JOIN_TIMEOUT};
use super::{ProxyJoinStatus, ProxyResult};
use crate::raw::FrameHeader;
use crate::wire::DeviceUid;

const AT: Ipv4Addr = Ipv4Addr::new(192, 168, 1, 20);
const MOVED: Ipv4Addr = Ipv4Addr::new(192, 168, 1, 21);
const MESH: DeviceUid = DeviceUid::from_array([0xC1; 16]);

const JOIN: &str =
    "50383600d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0590012000101101112131415161718191a1b1c1d1e1f";
const CHALLENGE: &str = "50383600c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c05900210002101112131415161718191a1b1c1d1e1f202122232425262728292a2b2c2d2e2f";
const PROOF: &str = "50383600d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d05900410003101112131415161718191a1b1c1d1e1f202122232425262728292a2b2c2d2e2ff22e793df352fff4d76660875d67c080378813eb426ed89d739ac2eac0c6fa2b";
const RESULT_0: &str = "50383600c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0590044000400003c101112131415161718191a1b1c1d1e1f202122232425262728292a2b2c2d2e2fe8dd2bdcf883b25ff4bf51be09abbd2a50cdf385707d5fea765b97767c54a528";
const INFO_SEALED: &str = "50383600c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c05900430006000000000000000187cdcacbf3e79148c2806b8bdd922cee0af9d74226b7740fa86b1809e05f116a2e3724aeb335b5d4125614eb84803f2022b09e69d1f587e19e84";
const HELLO_SEALED: &str = "50383600d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d059006700060000000000000001fa977a5a91314acac6ed75a026ef148dc8d95f229f50ed75c0f805dca743ccdbac5abdf7c6203e492ef318491a6b0b05b2b7916615fa247a06c18c9de9b2786b9fc757b80e45812cf36e08389fb623d68a9288a52a3a39d9471e90ad2b1e";
const HELLO: &str = "50380100d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0000036003600486f6d65204173736973742e00000000000000000000000000000000000000000000000000000000000000000000000000000800";

fn link() -> Link {
    Link::new(key(), UD, UC, AT)
}

fn fixed() -> Option<[u8; 16]> {
    Some(nd())
}

/// A link joined with the published nonces, as of `now`.
fn joined(now: Instant) -> Link {
    let mut l = link();
    let _ = l.tick(now, fixed);
    let _ = l.receive(AT, &hex(CHALLENGE), now);
    let outs = l.receive(AT, &hex(RESULT_0), now);
    assert_eq!(
        outs,
        vec![Out::Status(ProxyJoinStatus::Joined {
            lease: Duration::from_secs(60)
        })]
    );
    l
}

/// A `PROXY` frame from `sender`.
fn proxy(sender: DeviceUid, payload: &[u8]) -> Vec<u8> {
    let mut f = vec![0; FrameHeader::LEN];
    FrameHeader {
        protocol: 0x36,
        sender,
        opcode: 0x59,
        payload_len: payload.len() as u16,
    }
    .write_into(&mut f);
    f.extend_from_slice(payload);
    f
}

/// `inner` sealed by the controller at `counter`, as `DATA`.
fn from_controller(counter: u64, inner: &[u8]) -> Vec<u8> {
    let c2d = session_key(&key(), &nd(), &nc(), UD, UC, b"c2d");
    let mut f = proxy(UC, &[0; 9]);
    f[22..24].copy_from_slice(&((9 + inner.len() + 16) as u16).to_le_bytes());
    f[24] = 6;
    f[25..33].copy_from_slice(&counter.to_be_bytes());
    let sealed = seal(&c2d, counter, &f, inner);
    f.extend_from_slice(&sealed);
    f
}

/// A hello from `sender`, as the mesh relays one.
fn hello_from(sender: DeviceUid) -> Vec<u8> {
    let mut f = hex(HELLO);
    f[4..20].copy_from_slice(sender.as_bytes());
    f
}

/// The published join, step by step: `JOIN`, the `PROOF` answering the
/// `CHALLENGE`, and a `RESULT 0` taken with its proof.
#[test]
fn the_published_join_holds() {
    let now = Instant::now();
    let mut l = link();
    assert_eq!(l.tick(now, fixed), vec![Out::Send(AT, hex(JOIN))]);
    assert_eq!(
        l.receive(AT, &hex(CHALLENGE), now),
        vec![Out::Send(AT, hex(PROOF))]
    );
    assert!(!l.joined());
    let _ = l.receive(AT, &hex(RESULT_0), now);
    assert!(l.joined());
}

/// The published `INFO` opens, and names the controller on its mesh.
#[test]
fn the_published_info_opens() {
    let now = Instant::now();
    let mut l = joined(now);
    assert_eq!(l.receive(AT, &hex(INFO_SEALED), now), vec![]);
    assert_eq!(l.mesh_uid(), Some(MESH));
}

/// The first frame this client sends seals to the published `DATA`.
#[test]
fn the_published_hello_seals() {
    let mut l = joined(Instant::now());
    assert_eq!(l.seal(&hex(HELLO)), Some((AT, hex(HELLO_SEALED))));
}

/// A frame the controller relays opens to itself, once.
#[test]
fn a_relayed_frame_is_taken_once() {
    let now = Instant::now();
    let mut l = joined(now);
    let frame = hello_from(uid(9));
    let data = from_controller(2, &frame);
    assert_eq!(l.receive(AT, &data, now), vec![Out::Frame(AT, frame)]);
    assert_eq!(l.receive(AT, &data, now), vec![], "a replay was taken");
}

fn uid(n: u8) -> DeviceUid {
    DeviceUid::from_array([n; 16])
}

/// Nothing is sent before a session is up.
#[test]
fn nothing_is_sealed_before_a_join() {
    let mut l = link();
    assert_eq!(l.seal(&hex(HELLO)), None);
}

/// A challenge for another nonce, from another address or under another
/// uid is not answered.
#[test]
fn only_the_joins_own_challenge_is_answered() {
    let now = Instant::now();
    let mut wrong_nonce = hex(CHALLENGE);
    wrong_nonce[25] ^= 1;
    let mut wrong_uid = hex(CHALLENGE);
    wrong_uid[4] ^= 1;
    for (from, data) in [(AT, wrong_nonce), (MOVED, hex(CHALLENGE)), (AT, wrong_uid)] {
        let mut l = link();
        let _ = l.tick(now, fixed);
        assert_eq!(l.receive(from, &data, now), vec![], "{from}");
    }
}

/// A `RESULT 0` is taken only with the controller's proof for this join,
/// and only after its challenge.
#[test]
fn a_join_is_taken_only_with_the_controllers_proof() {
    let now = Instant::now();
    let mut early = link();
    let _ = early.tick(now, fixed);
    assert_eq!(
        early.receive(AT, &hex(RESULT_0), now),
        vec![],
        "before the challenge"
    );

    for at in [6, 22, 40, 67] {
        let mut forged = hex(RESULT_0);
        forged[FrameHeader::LEN + at] ^= 1;
        let mut l = link();
        let _ = l.tick(now, fixed);
        let _ = l.receive(AT, &hex(CHALLENGE), now);
        assert_eq!(l.receive(AT, &forged, now), vec![], "byte {at} changed");
        assert!(!l.joined());
    }
}

/// A join with no answer fails after 10 s, and the next join to that
/// address waits 10, 20, 40, then 60 s after each failure.
#[test]
fn an_unanswered_join_backs_off() {
    let start = Instant::now();
    let mut l = link();
    let mut at = start;
    let _ = l.tick(at, fixed);
    for wait in [10, 20, 40, 60, 60] {
        at += JOIN_TIMEOUT;
        let outs = l.tick(at, fixed);
        assert_eq!(outs, vec![Out::Status(ProxyJoinStatus::NoAnswer)]);
        let wait = Duration::from_secs(wait);
        assert_eq!(l.tick(at + wait - Duration::from_millis(1), fixed), vec![]);
        at += wait;
        assert_eq!(
            l.tick(at, fixed),
            vec![Out::Send(AT, hex(JOIN))],
            "after {wait:?}"
        );
    }
}

/// A refusal is reported, and backs off as no answer does.
#[test]
fn a_refused_join_is_reported_and_backs_off() {
    let now = Instant::now();
    let mut l = link();
    let _ = l.tick(now, fixed);
    let refusal = proxy(UC, &[4, 1, 0, 60]);
    assert_eq!(
        l.receive(AT, &refusal, now),
        vec![Out::Status(ProxyJoinStatus::Refused(
            ProxyResult::BAD_PROOF
        ))]
    );
    assert_eq!(l.tick(now + Duration::from_secs(9), fixed), vec![]);
    assert_eq!(
        l.tick(now + Duration::from_secs(10), fixed),
        vec![Out::Send(AT, hex(JOIN))]
    );
}

/// A session with nothing from the controller for its lease expires, and
/// the link joins again at once.
#[test]
fn a_silent_lease_expires_and_joins_again() {
    let now = Instant::now();
    let mut l = joined(now);
    let _ = l.receive(AT, &hex(INFO_SEALED), now + Duration::from_secs(30));
    let lease_end = now + Duration::from_secs(90);
    assert_eq!(l.tick(lease_end - Duration::from_millis(1), fixed), vec![]);
    assert_eq!(
        l.tick(lease_end, fixed),
        vec![
            Out::Status(ProxyJoinStatus::LeaseExpired),
            Out::Send(AT, hex(JOIN))
        ]
    );
    assert!(!l.joined());
}

/// A frame that fails its check is dropped and renews nothing.
#[test]
fn a_tampered_frame_is_dropped() {
    let now = Instant::now();
    let mut l = joined(now);
    let mut data = from_controller(2, &hello_from(uid(9)));
    let last = data.len() - 1;
    data[last] ^= 1;
    assert_eq!(l.receive(AT, &data, now + Duration::from_secs(59)), vec![]);
    assert_eq!(
        l.tick(now + Duration::from_secs(60), fixed).first(),
        Some(&Out::Status(ProxyJoinStatus::LeaseExpired))
    );
}

/// `DATA` from anywhere but the session's address is not taken.
#[test]
fn data_from_elsewhere_is_not_taken() {
    let now = Instant::now();
    let mut l = joined(now);
    assert_eq!(
        l.receive(MOVED, &from_controller(2, &hello_from(uid(9))), now),
        vec![]
    );
}

/// The controller's `LEAVE` and a `NOT_JOINED` from it end the session.
#[test]
fn the_controller_ends_a_session() {
    let now = Instant::now();
    let mut l = joined(now);
    let leave = from_controller(2, &proxy(UC, &[5]));
    assert_eq!(
        l.receive(AT, &leave, now),
        vec![Out::Status(ProxyJoinStatus::Left)]
    );
    assert!(!l.joined());

    let mut l = joined(now);
    assert_eq!(
        l.receive(AT, &proxy(UC, &[4, 4, 0, 0]), now),
        vec![Out::Status(ProxyJoinStatus::Refused(
            ProxyResult::NOT_JOINED
        ))]
    );
    assert!(!l.joined());
    assert_eq!(l.tick(now, fixed), vec![Out::Send(AT, hex(JOIN))]);
}

/// A `PROXY` frame inside `DATA` other than the controller's own `INFO` and
/// `LEAVE` is neither relayed nor taken.
#[test]
fn other_proxy_frames_inside_data_are_ignored() {
    let now = Instant::now();
    let mut l = joined(now);
    let mut info_from_other = proxy(uid(9), &[7, 1]);
    info_from_other.extend_from_slice(&[0xEE; 16]);
    info_from_other[22..24].copy_from_slice(&18u16.to_le_bytes());
    let leave_from_other = proxy(uid(9), &[5]);
    let announce = proxy(UC, &[0, 1]);
    for (i, inner) in [info_from_other, leave_from_other, announce]
        .into_iter()
        .enumerate()
    {
        let data = from_controller(2 + i as u64, &inner);
        assert_eq!(l.receive(AT, &data, now), vec![], "{i}");
    }
    assert!(l.joined());
    assert_eq!(l.mesh_uid(), None);
}

/// A hello from the controller at another address starts a join there,
/// one at a time, keeping the session until it succeeds; then the session
/// moves.
#[test]
fn the_controller_is_followed_to_a_new_address() {
    let now = Instant::now();
    let mut l = joined(now);
    let hello = hello_from(UC);
    assert_eq!(l.overheard(AT, &hello, now, fixed), vec![], "same address");
    assert_eq!(
        l.overheard(MOVED, &hello_from(uid(9)), now, fixed),
        vec![],
        "another uid"
    );
    assert_eq!(
        l.overheard(MOVED, &hello, now, fixed),
        vec![Out::Send(MOVED, hex(JOIN))]
    );
    assert_eq!(
        l.overheard(MOVED, &hello, now, fixed),
        vec![],
        "a second join"
    );
    assert!(l.joined(), "the session went before the new one was up");
    assert_eq!(l.seal(&hex(HELLO)).map(|(a, _)| a), Some(AT));

    let _ = l.receive(MOVED, &hex(CHALLENGE), now);
    let _ = l.receive(MOVED, &hex(RESULT_0), now);
    assert_eq!(l.seal(&hex(HELLO)).map(|(a, _)| a), Some(MOVED));
    assert_eq!(
        l.receive(AT, &hex(INFO_SEALED), now),
        vec![],
        "the old address"
    );
}

/// Leaving seals a `LEAVE` and ends the session.
#[test]
fn leaving_seals_a_leave() {
    let mut l = joined(Instant::now());
    let (to, data) = l.leave().expect("a session to leave");
    assert_eq!(to, AT);
    let d2c = session_key(&key(), &nd(), &nc(), UD, UC, b"d2c");
    let inner = super::crypto::open(&d2c, 1, &data[..33], &data[33..]).expect("opens");
    assert_eq!(inner, proxy(UD, &[5]));
    assert!(!l.joined());
    assert_eq!(l.leave(), None);
}

/// Once followed, a session that ends is joined again at the new address.
#[test]
fn a_rejoin_goes_to_the_address_followed() {
    let now = Instant::now();
    let mut l = joined(now);
    let _ = l.overheard(MOVED, &hello_from(UC), now, fixed);
    let _ = l.receive(MOVED, &hex(CHALLENGE), now);
    let _ = l.receive(MOVED, &hex(RESULT_0), now);
    let lease_end = now + Duration::from_secs(60);
    assert_eq!(
        l.tick(lease_end, fixed),
        vec![
            Out::Status(ProxyJoinStatus::LeaseExpired),
            Out::Send(MOVED, hex(JOIN))
        ]
    );
}

/// A join that succeeds clears the backoff its address built up: the next
/// join there to fail waits 10 s again.
#[test]
fn a_join_that_succeeds_clears_the_backoff() {
    let mut at = Instant::now();
    let mut l = link();
    let _ = l.tick(at, fixed);
    for wait in [10, 20] {
        at += JOIN_TIMEOUT;
        let _ = l.tick(at, fixed);
        at += Duration::from_secs(wait);
        let _ = l.tick(at, fixed);
    }
    let _ = l.receive(AT, &hex(CHALLENGE), at);
    let _ = l.receive(AT, &hex(RESULT_0), at);
    assert!(l.joined());
    let _ = l.receive(AT, &from_controller(2, &proxy(UC, &[5])), at);
    assert_eq!(l.tick(at, fixed), vec![Out::Send(AT, hex(JOIN))]);
    at += JOIN_TIMEOUT;
    let _ = l.tick(at, fixed);
    assert_eq!(
        l.tick(at + Duration::from_secs(10), fixed),
        vec![Out::Send(AT, hex(JOIN))],
        "the backoff before the join that succeeded still counts"
    );
}

/// An `INFO` of another version is not read.
#[test]
fn only_a_version_1_info_is_read() {
    let now = Instant::now();
    let mut l = joined(now);
    let mut info = proxy(UC, &[7, 2]);
    info.extend_from_slice(MESH.as_bytes());
    info[22..24].copy_from_slice(&18u16.to_le_bytes());
    let _ = l.receive(AT, &from_controller(2, &info), now);
    assert_eq!(l.mesh_uid(), None);
}

/// A refusal from anywhere but the address being joined is not taken.
#[test]
fn a_refusal_from_elsewhere_is_not_taken() {
    let now = Instant::now();
    let mut l = link();
    let _ = l.tick(now, fixed);
    assert_eq!(l.receive(MOVED, &proxy(UC, &[4, 1, 0, 60]), now), vec![]);
    let _ = l.receive(AT, &hex(CHALLENGE), now);
    let _ = l.receive(AT, &hex(RESULT_0), now);
    assert!(l.joined(), "the join was abandoned");
}
