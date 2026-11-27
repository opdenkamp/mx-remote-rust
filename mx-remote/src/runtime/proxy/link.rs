// Author: Lars Op den Kamp (lars@opdenkamp-it.nl)
// Copyright (c) 2026 Op den Kamp IT Solutions

//! A driver's link to a OneIP controller's proxy: the join, the session it
//! makes, and following the controller to a new address.
//!
//! Nothing here touches a socket or a clock. It takes what arrived and the
//! time, and says what to send and what to report, so every step of the
//! exchange is testable on its own.

use std::collections::HashMap;
use std::net::Ipv4Addr;
use std::time::{Duration, Instant};

use crate::raw::FrameHeader;
use crate::wire::DeviceUid;

use super::crypto::{
    controller_proof, driver_proof, open, proofs_match, seal, session_key, JoinNonce, Key, Proof,
    Replay, TAG_LEN,
};
use super::{ProxyJoinStatus, ProxyResult};

/// The `PROXY` opcode.
const OP_PROXY: u16 = 0x59;
/// What every `PROXY` frame is stamped with.
const PROXY_STAMP: u16 = 0x36;
/// The opcode of a hello.
const OP_HELLO: u16 = 0x00;
/// The proxy's protocol version this client speaks.
const VERSION: u8 = 1;

/// The `PROXY` sub-ops this client sends or takes. Sub-op 0, the
/// announcement, it takes as a flat client does.
mod sub {
    pub(super) const JOIN: u8 = 1;
    pub(super) const CHALLENGE: u8 = 2;
    pub(super) const PROOF: u8 = 3;
    pub(super) const RESULT: u8 = 4;
    pub(super) const LEAVE: u8 = 5;
    pub(super) const DATA: u8 = 6;
    pub(super) const INFO: u8 = 7;
}

/// How long a join waits for the controller's answer, from `JOIN` to
/// `RESULT`: as long as the controller holds its nonce.
pub(crate) const JOIN_TIMEOUT: Duration = Duration::from_secs(10);

/// The waits before joining an address again after a join there failed:
/// the first failure, the second, the third, and every one after.
const BACKOFF: [Duration; 4] = [
    Duration::from_secs(10),
    Duration::from_secs(20),
    Duration::from_secs(40),
    Duration::from_secs(60),
];

/// Where the `DATA` counter sits in a `PROXY` frame, after the header and
/// the sub-op, and where the sealed part starts.
const COUNTER_AT: usize = FrameHeader::LEN + 1;
const SEALED_AT: usize = COUNTER_AT + 8;

/// What the link asks of its owner.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Out {
    /// Send this datagram to the controller at this address.
    Send(Ipv4Addr, Vec<u8>),
    /// Report the join's state.
    Status(ProxyJoinStatus),
    /// A frame from the mesh, relayed by the controller at this address.
    Frame(Ipv4Addr, Vec<u8>),
}

/// A join under way.
#[derive(Debug)]
struct Pending {
    address: Ipv4Addr,
    nd: JoinNonce,
    /// The controller's nonce, once its challenge arrived.
    nc: Option<JoinNonce>,
    sent: Instant,
}

/// A join that succeeded.
#[derive(Debug)]
struct Session {
    address: Ipv4Addr,
    d2c: Key,
    c2d: Key,
    /// The last counter sent.
    sent: u64,
    received: Replay,
    lease: Duration,
    /// When a valid `DATA` from the controller last arrived, or the join
    /// succeeded.
    heard: Instant,
}

/// Failed joins to one address, and when the next may go.
#[derive(Clone, Copy, Debug)]
struct Backoff {
    failures: usize,
    next: Instant,
}

/// The link.
#[derive(Debug)]
pub(crate) struct Link {
    key: Key,
    /// This client's uid, the driver's.
    driver: DeviceUid,
    /// The controller's uplink uid, pinned.
    controller: DeviceUid,
    /// The address the controller is joined at, or last known at.
    address: Ipv4Addr,
    session: Option<Session>,
    pending: Option<Pending>,
    backoff: HashMap<Ipv4Addr, Backoff>,
    /// The controller's uid on its mesh, from its last `INFO`.
    mesh_uid: Option<DeviceUid>,
}

/// A `PROXY` frame from `sender`.
fn proxy_frame(sender: DeviceUid, payload: &[u8]) -> Vec<u8> {
    let mut frame = vec![0; FrameHeader::LEN];
    FrameHeader {
        protocol: PROXY_STAMP,
        sender,
        opcode: OP_PROXY,
        payload_len: u16::try_from(payload.len()).unwrap_or(u16::MAX),
    }
    .write_into(&mut frame);
    frame.extend_from_slice(payload);
    frame
}

fn nonce_at(payload: &[u8], at: usize) -> Option<JoinNonce> {
    payload.get(at..at + 16)?.try_into().ok()
}

impl Link {
    /// A link to the controller `controller` at `address`, joined with
    /// `key` as `driver`.
    pub(crate) fn new(
        key: Key,
        driver: DeviceUid,
        controller: DeviceUid,
        address: Ipv4Addr,
    ) -> Self {
        Self {
            key,
            driver,
            controller,
            address,
            session: None,
            pending: None,
            backoff: HashMap::new(),
            mesh_uid: None,
        }
    }

    /// Whether a session is up.
    pub(crate) fn joined(&self) -> bool {
        self.session.is_some()
    }

    /// The controller's uid on its mesh, once its `INFO` has said.
    pub(crate) fn mesh_uid(&self) -> Option<DeviceUid> {
        self.mesh_uid
    }

    /// Whether a join to `address` may go at `now`.
    fn may_join(&self, address: Ipv4Addr, now: Instant) -> bool {
        self.pending.is_none() && self.backoff.get(&address).map_or(true, |b| now >= b.next)
    }

    /// Starts a join to `address` with the fresh nonce `nd`.
    fn join(&mut self, address: Ipv4Addr, nd: JoinNonce, now: Instant, out: &mut Vec<Out>) {
        let mut payload = vec![sub::JOIN, VERSION];
        payload.extend_from_slice(&nd);
        out.push(Out::Send(address, proxy_frame(self.driver, &payload)));
        self.pending = Some(Pending {
            address,
            nd,
            nc: None,
            sent: now,
        });
    }

    /// Ends the join under way as failed, holding off the next one to its
    /// address.
    fn fail_join(&mut self, now: Instant) {
        if let Some(p) = self.pending.take() {
            let b = self.backoff.entry(p.address).or_insert(Backoff {
                failures: 0,
                next: now,
            });
            b.next = now + BACKOFF[b.failures.min(BACKOFF.len() - 1)];
            b.failures += 1;
        }
    }

    /// Ends the session, reporting why.
    fn drop_session(&mut self, why: ProxyJoinStatus, out: &mut Vec<Out>) {
        if self.session.take().is_some() {
            out.push(Out::Status(why));
        }
    }

    /// What is due at `now`: a join that went unanswered fails, a lease
    /// with nothing from the controller expires, and a link with no session
    /// joins again, as the backoff allows. `nd` makes a fresh nonce, or
    /// none when the host has no entropy to give, and then no join goes.
    pub(crate) fn tick(
        &mut self,
        now: Instant,
        nd: impl FnOnce() -> Option<JoinNonce>,
    ) -> Vec<Out> {
        let mut out = Vec::new();
        if self
            .pending
            .as_ref()
            .is_some_and(|p| now.saturating_duration_since(p.sent) >= JOIN_TIMEOUT)
        {
            self.fail_join(now);
            out.push(Out::Status(ProxyJoinStatus::NoAnswer));
        }
        if self
            .session
            .as_ref()
            .is_some_and(|s| now.saturating_duration_since(s.heard) >= s.lease)
        {
            self.drop_session(ProxyJoinStatus::LeaseExpired, &mut out);
        }
        if self.session.is_none() && self.may_join(self.address, now) {
            if let Some(nd) = nd() {
                self.join(self.address, nd, now, &mut out);
            }
        }
        out
    }

    /// Takes a datagram that arrived on the proxy port from `from`.
    pub(crate) fn receive(&mut self, from: Ipv4Addr, data: &[u8], now: Instant) -> Vec<Out> {
        let mut out = Vec::new();
        let Some(header) = FrameHeader::parse_accepted(data) else {
            return out;
        };
        if header.opcode != OP_PROXY || header.sender != self.controller {
            return out;
        }
        let payload = &data[FrameHeader::LEN..];
        match payload.first().copied() {
            Some(sub::CHALLENGE) => self.challenge(from, payload, &mut out),
            Some(sub::RESULT) => self.result(from, payload, now, &mut out),
            Some(sub::DATA) => self.data(from, data, now, &mut out),
            _ => {}
        }
        out
    }

    /// Takes a hello or announcement heard on the mesh ports from `from`: a
    /// hello from the pinned controller at another address starts a join
    /// there, as the backoff allows, keeping the session until it succeeds.
    pub(crate) fn overheard(
        &mut self,
        from: Ipv4Addr,
        data: &[u8],
        now: Instant,
        nd: impl FnOnce() -> Option<JoinNonce>,
    ) -> Vec<Out> {
        let mut out = Vec::new();
        let moved = FrameHeader::parse_accepted(data).is_some_and(|h| {
            h.sender == self.controller && h.opcode == OP_HELLO && from != self.address
        });
        if moved && self.may_join(from, now) {
            if let Some(nd) = nd() {
                self.join(from, nd, now, &mut out);
            }
        }
        out
    }

    fn challenge(&mut self, from: Ipv4Addr, payload: &[u8], out: &mut Vec<Out>) {
        let Some(p) = self.pending.as_mut() else {
            return;
        };
        if from != p.address || p.nc.is_some() || nonce_at(payload, 1) != Some(p.nd) {
            return;
        }
        let Some(nc) = nonce_at(payload, 17) else {
            return;
        };
        p.nc = Some(nc);
        let proof = driver_proof(&self.key, &p.nd, &nc, self.driver, self.controller);
        let mut reply = vec![sub::PROOF];
        reply.extend_from_slice(&p.nd);
        reply.extend_from_slice(&nc);
        reply.extend_from_slice(&proof);
        out.push(Out::Send(p.address, proxy_frame(self.driver, &reply)));
    }

    fn result(&mut self, from: Ipv4Addr, payload: &[u8], now: Instant, out: &mut Vec<Out>) {
        let (Some(&status), Some(lease)) = (
            payload.get(1),
            payload
                .get(2..4)
                .and_then(|b| <[u8; 2]>::try_from(b).ok())
                .map(u16::from_be_bytes),
        ) else {
            return;
        };
        let status = ProxyResult(status);
        let pending_here = self.pending.as_ref().is_some_and(|p| p.address == from);
        if status == ProxyResult::JOINED {
            if pending_here {
                self.accept_join(payload, lease, now, out);
            }
            return;
        }
        if pending_here {
            self.fail_join(now);
            out.push(Out::Status(ProxyJoinStatus::Refused(status)));
        } else if self.session.as_ref().is_some_and(|s| s.address == from) {
            self.drop_session(ProxyJoinStatus::Refused(status), out);
        }
    }

    /// A `RESULT 0`: taken only with the controller's proof for this join.
    fn accept_join(&mut self, payload: &[u8], lease: u16, now: Instant, out: &mut Vec<Out>) {
        let Some(p) = self.pending.as_ref() else {
            return;
        };
        let Some(nc) = p.nc else {
            return;
        };
        let proof: Option<Proof> = payload.get(36..68).and_then(|b| b.try_into().ok());
        let expected = controller_proof(&self.key, &p.nd, &nc, self.driver, self.controller, lease);
        let valid = nonce_at(payload, 4) == Some(p.nd)
            && nonce_at(payload, 20) == Some(nc)
            && proof.is_some_and(|proof| proofs_match(&proof, &expected));
        if !valid {
            return;
        }
        let (address, nd) = (p.address, p.nd);
        self.pending = None;
        self.backoff.remove(&address);
        let key =
            |direction| session_key(&self.key, &nd, &nc, self.driver, self.controller, direction);
        let lease = Duration::from_secs(u64::from(lease));
        self.session = Some(Session {
            address,
            d2c: key(b"d2c"),
            c2d: key(b"c2d"),
            sent: 0,
            received: Replay::default(),
            lease,
            heard: now,
        });
        self.address = address;
        out.push(Out::Status(ProxyJoinStatus::Joined { lease }));
    }

    fn data(&mut self, from: Ipv4Addr, data: &[u8], now: Instant, out: &mut Vec<Out>) {
        let Some(s) = self.session.as_mut().filter(|s| s.address == from) else {
            return;
        };
        let Some(counter) = data
            .get(COUNTER_AT..SEALED_AT)
            .and_then(|b| <[u8; 8]>::try_from(b).ok())
            .map(u64::from_be_bytes)
        else {
            return;
        };
        let sealed = &data[SEALED_AT.min(data.len())..];
        if sealed.len() < TAG_LEN || !s.received.fresh(counter) {
            return;
        }
        let Some(inner) = open(&s.c2d, counter, &data[..SEALED_AT], sealed) else {
            return;
        };
        s.received.take(counter);
        s.heard = now;
        let address = s.address;
        match FrameHeader::parse_accepted(&inner) {
            Some(h) if h.opcode == OP_PROXY => self.inner_proxy(h, &inner, out),
            Some(_) => out.push(Out::Frame(address, inner)),
            None => {}
        }
    }

    /// A `PROXY` frame inside `DATA`: the controller's `INFO` and `LEAVE`
    /// are taken, and any other is ignored.
    fn inner_proxy(&mut self, header: FrameHeader, inner: &[u8], out: &mut Vec<Out>) {
        if header.sender != self.controller {
            return;
        }
        match inner.get(FrameHeader::LEN..) {
            Some([sub::INFO, VERSION, uid @ ..]) => {
                if let Some(uid) = uid.get(..16).and_then(|b| <[u8; 16]>::try_from(b).ok()) {
                    self.mesh_uid = Some(DeviceUid::from_array(uid));
                }
            }
            Some([sub::LEAVE, ..]) => self.drop_session(ProxyJoinStatus::Left, out),
            _ => {}
        }
    }

    /// Seals `frame` for the controller, `None` while no session is up.
    pub(crate) fn seal(&mut self, frame: &[u8]) -> Option<(Ipv4Addr, Vec<u8>)> {
        let driver = self.driver;
        let s = self.session.as_mut()?;
        let declared = u16::try_from(9 + frame.len() + TAG_LEN).ok()?;
        s.sent += 1;
        let mut datagram = vec![0; FrameHeader::LEN];
        FrameHeader {
            protocol: PROXY_STAMP,
            sender: driver,
            opcode: OP_PROXY,
            payload_len: declared,
        }
        .write_into(&mut datagram);
        datagram.push(sub::DATA);
        datagram.extend_from_slice(&s.sent.to_be_bytes());
        let sealed = seal(&s.d2c, s.sent, &datagram, frame);
        datagram.extend_from_slice(&sealed);
        Some((s.address, datagram))
    }

    /// The `LEAVE` that ends the session, sealed, and the session ended.
    pub(crate) fn leave(&mut self) -> Option<(Ipv4Addr, Vec<u8>)> {
        let leave = proxy_frame(self.driver, &[sub::LEAVE]);
        let sealed = self.seal(&leave);
        self.session = None;
        sealed
    }
}
