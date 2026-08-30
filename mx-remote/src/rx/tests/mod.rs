// Author: Lars Op den Kamp (lars@opdenkamp-it.nl)
// Copyright (c) 2026 Op den Kamp IT Solutions

//! Receive-path tests and the fixtures they share.

mod commands;
mod handlers;
mod proto28;
mod state;
mod subsystems;

use std::net::Ipv4Addr;
use std::time::Instant;

use crate::event::Event;
use crate::state::{Bay, Device, State};
use crate::testing::*;
use crate::wire::{DeviceFeature, DeviceUid, Opcode, PROTOCOL_VERSION};

use super::process_frame;

/// A registry fed by hand, collecting every event the handlers produce.
pub(super) struct Harness {
    pub(super) state: State,
    pub(super) events: Vec<Event>,
    pub(super) sender: DeviceUid,
}

impl Harness {
    /// A registry whose only known peer will be `uid_n(n)`.
    pub(super) fn new(n: u8) -> Self {
        Self {
            // A client uid no fixture uses, so nothing is mistaken for this
            // client's own frame and dropped.
            state: State::new(uid_n(0xFE)),
            events: Vec::new(),
            sender: uid_n(n),
        }
    }

    pub(super) fn feed(&mut self, op: Opcode, payload: &[u8]) {
        self.feed_as(self.sender, op, payload);
    }

    pub(super) fn feed_as(&mut self, sender: DeviceUid, op: Opcode, payload: &[u8]) {
        self.feed_full(sender, op, PROTOCOL_VERSION, payload, Instant::now());
    }

    pub(super) fn feed_proto(&mut self, op: Opcode, protocol: u16, payload: &[u8]) {
        self.feed_full(self.sender, op, protocol, payload, Instant::now());
    }

    pub(super) fn feed_at(&mut self, op: Opcode, payload: &[u8], timestamp: Instant) {
        self.feed_full(self.sender, op, PROTOCOL_VERSION, payload, timestamp);
    }

    pub(super) fn feed_full(
        &mut self,
        sender: DeviceUid,
        op: Opcode,
        protocol: u16,
        payload: &[u8],
        timestamp: Instant,
    ) {
        let data = datagram(sender, op, protocol, payload);
        let address = Some(Ipv4Addr::new(10, 8, 8, 9));
        let events = process_frame(&mut self.state, &data, address, timestamp);
        self.events.extend(events);
    }

    /// Announces `sender` as a device with the given advertisement.
    pub(super) fn hello(&mut self, protocol: u16, name: &str, serial: &str, f: DeviceFeature) {
        let payload = hello_payload(protocol, name, serial, "4.8.0", f);
        self.feed(crate::wire::op::SYS_HELLO, &payload);
    }

    pub(super) fn device(&self) -> &Device {
        self.state
            .device(self.sender)
            .expect("device not registered")
    }

    pub(super) fn bay(&self, port: u16) -> &Bay {
        self.device().bay(port).expect("bay not registered")
    }

    /// Whether an event matching `pred` was produced.
    pub(super) fn saw(&self, pred: impl Fn(&Event) -> bool) -> bool {
        self.events.iter().any(pred)
    }
}
