// Author: Lars Op den Kamp (lars@opdenkamp-it.nl)
// Copyright (c) 2026 Op den Kamp IT Solutions

//! The port metadata a mesh keeps: fields an installer recorded about the
//! device on each port, which every unit holds and returns with its ports.
//!
//! This client takes part as a unit does: as a member it follows its
//! controller's index, as controller it sends its own, and it answers every
//! request for a port's metadata. The application serves the API that sets
//! metadata, through [`Remote::set_port_metadata`], and persists what it
//! holds, through [`Remote::port_metadata_store`] and
//! [`Remote::restore_port_metadata`].

use std::time::{Duration, Instant};

use crate::state::MetadataRefusal;
use crate::types::{MeshPort, PortMetadataField, PortMetadataStore};
use crate::wire::{build_port_metadata_request, op, Addressee, DeviceUid};

use super::control::{device_of, Command, ControlError};
use super::{lock, Remote, Shared};

/// The shortest interval between the controller's periodic indexes.
const INDEX_EVERY_MIN: Duration = Duration::from_secs(45);

/// The width of the random spread over [`INDEX_EVERY_MIN`].
const INDEX_EVERY_SPREAD: Duration = Duration::from_secs(15);

/// Draws the interval to the controller's next periodic index, 45 to 60 s,
/// so the controllers of several meshes do not fall into step.
pub(super) fn next_index_interval() -> Duration {
    let mut bytes = [0u8; 2];
    if getrandom::getrandom(&mut bytes).is_err() {
        return INDEX_EVERY_MIN + INDEX_EVERY_SPREAD / 2;
    }
    let fraction = u64::from(u16::from_be_bytes(bytes));
    let spread = INDEX_EVERY_SPREAD.as_millis() as u64 * fraction / 65536;
    INDEX_EVERY_MIN + Duration::from_millis(spread)
}

fn refused(refusal: MetadataRefusal) -> ControlError {
    ControlError::InvalidRequest(match refusal {
        MetadataRefusal::Invalid => {
            "port metadata is at most 16 fields of at most 1024 bytes together, each key \
             1-31 characters of A-Z, a-z, 0-9, _ and -, each value 1-255 bytes of UTF-8 \
             without control characters, \" or \\"
        }
        MetadataRefusal::Repeated => "a port listed twice",
        MetadataRefusal::RepeatedKey => "a port metadata key set twice",
    })
}

impl Remote {
    /// The metadata of `port` this client holds, `None` when it holds none.
    pub fn port_metadata(&self, port: MeshPort) -> Option<Vec<PortMetadataField>> {
        self.shared.read(|state| state.metadata.get(port))
    }

    /// All the port metadata this client holds, with its generation: what a
    /// controller persists, and restores with
    /// [`Remote::restore_port_metadata`].
    pub fn port_metadata_store(&self) -> PortMetadataStore {
        self.shared.read(|state| state.metadata.store())
    }

    /// Replaces all the port metadata this client holds with `store`, as a
    /// unit loads what it persisted; best before [`Remote::start`]. Nothing
    /// is reported or sent for it, but as controller this client sends its
    /// index within 1 s. A port of no fields is left out.
    ///
    /// Refused, holding what it held, for a port listed twice or a field
    /// [`Remote::set_port_metadata`] refuses.
    pub fn restore_port_metadata(&self, store: &PortMetadataStore) -> Result<(), ControlError> {
        let now = Instant::now();
        self.shared.mutate(|state, _| {
            state.metadata.restore(store).map_err(refused)?;
            if state.controls_mesh() {
                state.metadata.index_due = Some(now + Duration::from_secs(1));
            }
            Ok(())
        })
    }

    /// Sets the metadata of `port`, as the API of the unit serving it does:
    /// `fields` in the order given, none removing the port's metadata.
    /// When that changes what this client holds, the generation goes up by
    /// one and both are reported to this client's own handler; as controller
    /// it also sends the port's metadata to every unit at once and its index
    /// 1 s later, and its members take them. A unit in no mesh keeps it
    /// alone.
    ///
    /// Refused while this client follows another unit as its controller,
    /// whose API it is, and for metadata no unit takes: more than 16 fields
    /// or 1024 bytes as carried, a key other than 1 to 31 characters of
    /// `A-Z`, `a-z`, `0-9`, `_` and `-`, or a value other than 1 to 255
    /// bytes without control characters, `"` or `\`, or a key set twice.
    /// (Metadata received with a repeated key is taken as it is, as a unit
    /// takes it, and [`Remote::restore_port_metadata`] takes it back.)
    pub fn set_port_metadata(
        &self,
        port: MeshPort,
        fields: Vec<PortMetadataField>,
    ) -> Result<(), ControlError> {
        let now = Instant::now();
        self.shared.mutate(|state, ev| {
            if !state.mesh_controller.is_zero() && !state.controls_mesh() {
                return Err(ControlError::InvalidRequest(
                    "a mesh member's port metadata is set on its controller",
                ));
            }
            state
                .set_port_metadata(port, fields, now, ev)
                .map_err(refused)
        })?;
        self.shared.send_metadata_outbox();
        Ok(())
    }

    /// Asks `holder` for the metadata of `port`. Every unit answers with
    /// what it holds, no fields when none, and the answer is taken from
    /// `holder` for 30 s: it arrives at
    /// [`EventHandler::on_port_metadata_changed`](crate::EventHandler::on_port_metadata_changed)
    /// when it differs from what this client holds, and
    /// [`Remote::port_metadata`] reads it.
    pub fn request_port_metadata(
        &self,
        holder: DeviceUid,
        port: MeshPort,
    ) -> Result<(), ControlError> {
        self.shared.command(move |state| {
            let d = device_of(state, holder)?;
            Ok(Command::new(
                Addressee::direct(d, d.heard_path(Instant::now())),
                op::MESH_CONFIG,
                build_port_metadata_request(port),
            )
            .then(move |state, _| state.metadata.asked_now(holder, Instant::now())))
        })
    }
}

impl Shared {
    /// Sends the metadata frames the state queued: each to every unit, or
    /// direct to the device it names.
    pub(super) fn send_metadata_outbox(&self) {
        let outbox = std::mem::take(&mut lock(&self.state).metadata.outbox);
        for (to, payload) in outbox {
            let to = match to {
                None => Addressee::Broadcast,
                Some(device) => match self.direct(device) {
                    Some(to) => to,
                    None => continue,
                },
            };
            let _ = self.send(&to, op::MESH_CONFIG, &payload);
        }
    }
}
