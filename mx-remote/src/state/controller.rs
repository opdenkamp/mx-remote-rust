// Author: Lars Op den Kamp (lars@opdenkamp-it.nl)
// Copyright (c) 2026 Op den Kamp IT Solutions

//! Pulling another dedicated controller's state over `CONTROLLER_STATE`
//! (0x58): which senders a frame is taken from, and each part being pulled,
//! window by window.

use std::time::{Duration, Instant};

use crate::event::Event;
use crate::types::ControllerPart;
use crate::wire::{build_controller_request, crc32, DeviceUid, StateHeader, CONTROLLER_WINDOW_MAX};

use super::State;

/// How long a pull waits for DATA before asking again from the first byte it
/// does not have.
pub(crate) const PULL_RETRY: Duration = Duration::from_secs(2);

/// How many asks in a row without a byte of progress a pull makes before it
/// gives up.
pub(crate) const PULL_ASKS: u8 = 10;

/// One part of another dedicated controller's state being pulled.
#[derive(Debug)]
pub(crate) struct ControllerPull {
    pub(crate) device: DeviceUid,
    pub(crate) part: u8,
    pub(crate) generation: u32,
    pub(crate) length: u32,
    pub(crate) crc: u32,
    pub(crate) bytes: Vec<u8>,
    /// The end of the window last asked for.
    window_end: u32,
    /// When it last asked or took bytes.
    last: Instant,
    /// Asks since the last byte taken.
    idle_asks: u8,
    /// A frame out of order or of another generation ended the window, and
    /// it asked again: further such frames, the rest of that window, are
    /// dropped until the answer starts.
    broken: bool,
}

impl ControllerPull {
    fn have(&self) -> u32 {
        self.bytes.len() as u32
    }

    /// Asks for the next window, from the first byte it does not have.
    fn ask(&mut self, now: Instant, asks: &mut Vec<(DeviceUid, Vec<u8>)>) {
        let offset = self.have();
        let count = (self.length - offset).min(CONTROLLER_WINDOW_MAX);
        self.window_end = offset + count;
        self.last = now;
        asks.push((
            self.device,
            build_controller_request(self.part, self.generation, offset, count),
        ));
    }
}

/// How a DATA frame left a pull.
enum Taken {
    Pulling,
    Complete,
    Failed,
}

impl State {
    /// Whether a `CONTROLLER_STATE` request or data is taken from `uid`: a
    /// dedicated controller, one that sent a hello, of this client's own
    /// mesh, so one whose controller is this client's controller or which is
    /// that controller; or the one the application allowed, which this
    /// client is replacing a router of its mesh with.
    pub(crate) fn takes_controller_state(&self, uid: DeviceUid) -> bool {
        let Some(device) = self.device(uid) else {
            return false;
        };
        if device.controller_hello.is_none() {
            return false;
        }
        if self.controller_state_allowed == Some(uid) {
            return true;
        }
        let controller = self.mesh_controller;
        !controller.is_zero() && (uid == controller || device.mesh_master == controller)
    }

    /// Starts pulling `part` of `device`'s state `generation`, in place of a
    /// pull of the same part under way. An empty part completes at once.
    pub(crate) fn start_controller_pull(
        &mut self,
        device: DeviceUid,
        generation: u32,
        wanted: ControllerPart,
        now: Instant,
        ev: &mut Vec<Event>,
    ) {
        let ControllerPart { part, length, crc } = wanted;
        self.controller_pulls
            .retain(|p| !(p.device == device && p.part == part));
        if length == 0 {
            ev.push(if crc == crc32(&[]) {
                Event::ControllerPartReceived {
                    device,
                    part,
                    generation,
                    bytes: Vec::new(),
                }
            } else {
                Event::ControllerPartFailed {
                    device,
                    part,
                    generation,
                }
            });
            return;
        }
        let mut pull = ControllerPull {
            device,
            part,
            generation,
            length,
            crc,
            bytes: Vec::new(),
            window_end: 0,
            last: now,
            idle_asks: 0,
            broken: false,
        };
        pull.ask(now, &mut self.controller_asks);
        self.controller_pulls.push(pull);
    }

    /// Takes a DATA frame: bytes of a part being pulled from its sender.
    pub(crate) fn on_controller_data(
        &mut self,
        sender: DeviceUid,
        header: &StateHeader,
        data: &[u8],
        now: Instant,
        ev: &mut Vec<Event>,
    ) {
        let Some(at) = self
            .controller_pulls
            .iter()
            .position(|p| p.device == sender && p.part == header.part)
        else {
            return;
        };
        let taken = take_data(
            &mut self.controller_pulls[at],
            header,
            data,
            now,
            &mut self.controller_asks,
        );
        let finished = match taken {
            Taken::Pulling => return,
            Taken::Complete => true,
            Taken::Failed => false,
        };
        let pull = self.controller_pulls.swap_remove(at);
        ev.push(if finished {
            Event::ControllerPartReceived {
                device: pull.device,
                part: pull.part,
                generation: pull.generation,
                bytes: pull.bytes,
            }
        } else {
            Event::ControllerPartFailed {
                device: pull.device,
                part: pull.part,
                generation: pull.generation,
            }
        });
    }

    /// Asks again for a pull that has waited [`PULL_RETRY`] for data, and
    /// gives up on one whose device went offline, or after [`PULL_ASKS`] asks
    /// without progress.
    pub(crate) fn check_controller_pulls(&mut self, now: Instant, ev: &mut Vec<Event>) {
        let mut failed = Vec::new();
        for (i, pull) in self.controller_pulls.iter_mut().enumerate() {
            let online = self.devices.get(&pull.device).is_some_and(|d| d.online);
            if !online {
                failed.push(i);
                continue;
            }
            if now.saturating_duration_since(pull.last) < PULL_RETRY {
                continue;
            }
            if pull.idle_asks >= PULL_ASKS {
                failed.push(i);
                continue;
            }
            pull.idle_asks += 1;
            pull.broken = false;
            pull.ask(now, &mut self.controller_asks);
        }
        for i in failed.into_iter().rev() {
            let pull = self.controller_pulls.swap_remove(i);
            ev.push(Event::ControllerPartFailed {
                device: pull.device,
                part: pull.part,
                generation: pull.generation,
            });
        }
    }
}

/// Takes one DATA frame into its pull.
fn take_data(
    pull: &mut ControllerPull,
    header: &StateHeader,
    data: &[u8],
    now: Instant,
    asks: &mut Vec<(DeviceUid, Vec<u8>)>,
) -> Taken {
    if header.generation != pull.generation {
        // No bytes and the current generation: the sender no longer holds
        // the one asked for.
        if data.is_empty() {
            return Taken::Failed;
        }
        return restart(pull, now, asks);
    }
    if header.length != pull.length || header.crc != pull.crc {
        // Not the part its hello described.
        return Taken::Failed;
    }
    if data.is_empty() || header.offset != pull.have() {
        return restart(pull, now, asks);
    }
    let room = (pull.length - pull.have()) as usize;
    pull.bytes.extend_from_slice(&data[..data.len().min(room)]);
    pull.broken = false;
    pull.last = now;
    pull.idle_asks = 0;
    if pull.have() == pull.length {
        return if crc32(&pull.bytes) == pull.crc {
            Taken::Complete
        } else {
            Taken::Failed
        };
    }
    if pull.have() >= pull.window_end {
        pull.ask(now, asks);
    }
    Taken::Pulling
}

/// A frame out of order, or bytes of another generation, end the window:
/// asks again from the first byte it does not have, once until the answer
/// starts arriving.
fn restart(pull: &mut ControllerPull, now: Instant, asks: &mut Vec<(DeviceUid, Vec<u8>)>) -> Taken {
    if !pull.broken {
        if pull.idle_asks >= PULL_ASKS {
            return Taken::Failed;
        }
        pull.idle_asks += 1;
        pull.broken = true;
        pull.ask(now, asks);
    }
    Taken::Pulling
}
