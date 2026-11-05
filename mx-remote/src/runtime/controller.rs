// Author: Lars Op den Kamp (lars@opdenkamp-it.nl)
// Copyright (c) 2026 Op den Kamp IT Solutions

//! A dedicated controller's `CONTROLLER_STATE` (0x58): the hello it sends,
//! the parts of its state it serves, and the parts of another's it pulls
//! (routers.md).

use std::time::Instant;

use crate::types::{ControllerHello, ControllerPart, CONTROLLER_PARTS_MAX};
use crate::wire::{
    build_controller_data, crc32, op, Addressee, ControllerRole, DeviceUid, StateHeader,
    CONTROLLER_DATA_MAX, CONTROLLER_WINDOW_MAX,
};

use super::{lock, ControlError, Remote, Shared};

/// What this client, as a dedicated controller, announces of itself and
/// serves to another.
#[derive(Debug, Default)]
pub(super) struct OwnController {
    role: ControllerRole,
    primary: DeviceUid,
    generation: u32,
    /// Each part, its bytes and their crc32, in the order the hello lists
    /// them.
    parts: Vec<(u8, Vec<u8>, u32)>,
    /// When the hello last went out, `None` since the sockets opened.
    pub(super) sent: Option<Instant>,
}

impl OwnController {
    /// The hello this client sends.
    pub(super) fn hello(&self) -> ControllerHello {
        ControllerHello {
            role: self.role,
            generation: self.generation,
            primary: self.primary,
            parts: self
                .parts
                .iter()
                .map(|(part, bytes, crc)| ControllerPart {
                    part: *part,
                    length: bytes.len() as u32,
                    crc: *crc,
                })
                .collect(),
        }
    }

    /// The DATA frames answering `request`: the window it asks for, at most
    /// [`CONTROLLER_WINDOW_MAX`] bytes, in frames of at most
    /// [`CONTROLLER_DATA_MAX`]. One frame without bytes for a request past the
    /// end, and for one of a generation no longer held, which carries the
    /// current generation. A part not held is answered as an empty one.
    fn answer(&self, request: &StateHeader) -> Vec<Vec<u8>> {
        let (bytes, crc) = self
            .parts
            .iter()
            .find(|(part, _, _)| *part == request.part)
            .map_or((&[][..], crc32(&[])), |(_, bytes, crc)| (&bytes[..], *crc));
        let length = bytes.len() as u32;
        let empty = |offset: u32| {
            build_controller_data(request.part, self.generation, offset, length, crc, &[])
        };
        if request.generation != self.generation || request.offset >= length {
            return vec![empty(request.offset)];
        }
        let end = length.min(
            request
                .offset
                .saturating_add(request.length.min(CONTROLLER_WINDOW_MAX)),
        );
        bytes[request.offset as usize..end as usize]
            .chunks(CONTROLLER_DATA_MAX)
            .enumerate()
            .map(|(i, chunk)| {
                let offset = request.offset + (i * CONTROLLER_DATA_MAX) as u32;
                build_controller_data(request.part, self.generation, offset, length, crc, chunk)
            })
            .collect()
    }
}

impl Remote {
    /// The `CONTROLLER_STATE` hello a device last sent, `None` until it has
    /// sent one; a device that has is a dedicated controller.
    pub fn controller_hello(&self, uid: DeviceUid) -> Option<ControllerHello> {
        self.shared
            .read(|state| state.device(uid)?.controller_hello.clone())
    }

    /// Sets the role and the primary this client's `CONTROLLER_STATE` hello
    /// carries as a dedicated controller
    /// ([`Config::dedicated_controller`](crate::Config::dedicated_controller)),
    /// and sends it at once when they changed. It starts as role
    /// [`ControllerRole::NONE`] with no primary.
    pub fn set_controller_role(
        &self,
        role: ControllerRole,
        primary: DeviceUid,
    ) -> Result<(), ControlError> {
        self.own_controller(|own| {
            let changed = own.role != role || own.primary != primary;
            own.role = role;
            own.primary = primary;
            changed
        })
    }

    /// Sets the parts of this client's state, as a dedicated controller:
    /// each part number with its bytes. The hello then lists each part's
    /// length and crc32 under `generation`, and goes at once when either
    /// changed. A request from another dedicated controller of this client's
    /// mesh, or the one [`Remote::allow_controller_state`] names, is answered
    /// from them, for this generation only. It starts with no parts and
    /// generation 0.
    ///
    /// Refused for more than [`CONTROLLER_PARTS_MAX`] parts, a part listed
    /// twice, or one over `u32::MAX` bytes.
    pub fn set_controller_parts(
        &self,
        generation: u32,
        parts: Vec<(u8, Vec<u8>)>,
    ) -> Result<(), ControlError> {
        if parts.len() > CONTROLLER_PARTS_MAX {
            return Err(ControlError::InvalidRequest(
                "too many controller state parts",
            ));
        }
        for (i, (part, bytes)) in parts.iter().enumerate() {
            if parts[..i].iter().any(|(other, _)| other == part) {
                return Err(ControlError::InvalidRequest(
                    "a controller state part listed twice",
                ));
            }
            if u32::try_from(bytes.len()).is_err() {
                return Err(ControlError::InvalidRequest(
                    "a controller state part too long",
                ));
            }
        }
        let parts: Vec<(u8, Vec<u8>, u32)> = parts
            .into_iter()
            .map(|(part, bytes)| {
                let crc = crc32(&bytes);
                (part, bytes, crc)
            })
            .collect();
        self.own_controller(|own| {
            let changed = own.generation != generation || own.parts != parts;
            own.generation = generation;
            own.parts = parts;
            changed
        })
    }

    /// Changes what this client announces as a dedicated controller, and
    /// sends its hello at once when `change` says it changed.
    fn own_controller(
        &self,
        change: impl FnOnce(&mut OwnController) -> bool,
    ) -> Result<(), ControlError> {
        if !self.shared.dedicated_controller {
            return Err(ControlError::InvalidRequest(
                "only a dedicated controller sends a controller hello",
            ));
        }
        let changed = change(&mut lock(&self.shared.own_controller));
        if changed {
            self.shared.announce_controller(Instant::now(), true);
        }
        Ok(())
    }

    /// Also takes `CONTROLLER_STATE` requests and data from `uid`, a
    /// dedicated controller of no mesh of this client's: one this client is
    /// replacing a router of its mesh with (routers.md, Receiver). `None`
    /// takes them only from this client's mesh again.
    pub fn allow_controller_state(&self, uid: Option<DeviceUid>) {
        self.shared
            .mutate(|state, _| state.controller_state_allowed = uid);
    }

    /// Pulls `part` of `from`'s state `generation`, which its hello lists
    /// with `length` and `crc`, in place of a pull of the same part under
    /// way. It asks for windows of up to 8192 bytes, each from the first byte
    /// it does not have, and again from there when no data arrives for 2 s.
    /// The part arrives at
    /// [`EventHandler::on_controller_part_received`](crate::EventHandler::on_controller_part_received)
    /// once whole and its crc32 matches, or the pull ends at
    /// [`EventHandler::on_controller_part_failed`](crate::EventHandler::on_controller_part_failed).
    /// Pulls of other parts, or of other devices, run alongside.
    ///
    /// Data is taken only from a dedicated controller of this client's mesh,
    /// or the one [`Remote::allow_controller_state`] names.
    pub fn pull_controller_part(
        &self,
        from: DeviceUid,
        part: u8,
        generation: u32,
        length: u32,
        crc: u32,
    ) -> Result<(), ControlError> {
        let now = Instant::now();
        self.shared.mutate(|state, ev| {
            let known = state.device(from).is_some_and(|d| d.online);
            if !known {
                return Err(ControlError::UnknownDevice(from));
            }
            let wanted = ControllerPart { part, length, crc };
            state.start_controller_pull(from, generation, wanted, now, ev);
            Ok(())
        })?;
        self.shared.send_controller_asks();
        Ok(())
    }
}

impl Shared {
    /// Answers the `CONTROLLER_STATE` requests a frame raised, each direct
    /// to the controller that asked.
    pub(super) fn answer_controller_requests(&self, requests: Vec<(DeviceUid, StateHeader)>) {
        if !self.dedicated_controller {
            return;
        }
        for (to, request) in requests {
            let frames = lock(&self.own_controller).answer(&request);
            let Some(to) = self.direct(to) else {
                continue;
            };
            for frame in frames {
                let _ = self.send(&to, op::CONTROLLER_STATE, &frame);
            }
        }
    }

    /// Sends the requests the pulls raised.
    pub(super) fn send_controller_asks(&self) {
        let asks = std::mem::take(&mut lock(&self.state).controller_asks);
        for (to, payload) in asks {
            if let Some(to) = self.direct(to) {
                let _ = self.send(&to, op::CONTROLLER_STATE, &payload);
            }
        }
    }

    /// A device addressed alone, by the endpoint it is heard by.
    fn direct(&self, device: DeviceUid) -> Option<Addressee> {
        let now = Instant::now();
        self.read(|state| {
            state
                .device(device)
                .map(|d| Addressee::direct(d, d.heard_path(now)))
        })
    }
}
