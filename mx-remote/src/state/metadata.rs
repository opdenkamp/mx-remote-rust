// Author: Lars Op den Kamp (lars@opdenkamp-it.nl)
// Copyright (c) 2026 Op den Kamp IT Solutions

//! The port metadata a mesh keeps, over `MESH_CONFIG` (0x57) kinds 6 to 8.
//!
//! Every unit holds the metadata of every port of its mesh, and a
//! generation. The controller sends a port's metadata to every unit when it
//! changes, and an index of every port's crc32 under its generation
//! periodically and 1 s after a change. A unit taking an index asks its
//! sender for each port it lacks or holds differently, and once the whole
//! index has arrived drops every port it does not name and takes its
//! generation.
//!
//! A member takes its controller's index when it names the member's own
//! generation or a newer one, and answers an older one with its own index,
//! sent to the controller; the controller takes a member's index only when
//! it names a newer generation, which is how a new controller learns the
//! mesh's metadata. Metadata is taken from the unit's own controller at any
//! time, and from the sender of the index it last took for 30 s after the
//! index started. Every unit answers a request for a port's metadata.

use std::collections::{BTreeMap, BTreeSet};
use std::time::{Duration, Instant};

use crate::event::Event;
use crate::types::{
    MeshPort, PortMetadata, PortMetadataField, PortMetadataStore, PORT_METADATA_MAX,
};
use crate::wire::{
    build_metadata_index_pages, build_port_metadata, build_port_metadata_request, crc32,
    decode_metadata_index, decode_port_metadata, encode_port_metadata, DeviceUid,
    MetadataIndexEntry,
};

use super::State;

/// How long metadata is taken from the sender of an index, from when its
/// first frame was taken.
pub(crate) const METADATA_PULL_WINDOW: Duration = Duration::from_secs(30);

/// How often a member answers an older index of its controller's with its
/// own, at most.
pub(crate) const METADATA_OFFER_EVERY: Duration = Duration::from_secs(60);

/// How long after a change of its metadata the controller sends its index.
pub(crate) const METADATA_INDEX_AFTER_CHANGE: Duration = Duration::from_secs(1);

/// What a metadata or index frame carries past its kind, flags and subject:
/// the offset, the length and the crc32 its header gives, and its body.
#[derive(Clone, Copy, Debug)]
pub(crate) struct MetadataFrame<'a> {
    pub(crate) first: u16,
    pub(crate) total: u16,
    pub(crate) crc: u32,
    pub(crate) body: &'a [u8],
}

/// The index this client is taking, from its first frame on.
#[derive(Debug)]
struct IndexPull {
    from: DeviceUid,
    generation: u32,
    total: u16,
    crc: u32,
    /// Entries taken so far, which the next frame has to start at.
    taken: usize,
    /// The ports the frames taken so far name.
    named: BTreeSet<MeshPort>,
    /// When its first frame was taken: metadata is taken from its sender for
    /// [`METADATA_PULL_WINDOW`] after.
    started: Instant,
    /// Every frame has arrived and been acted on.
    complete: bool,
}

/// The port metadata this client holds, and where it stands in taking more.
#[derive(Debug, Default)]
pub(crate) struct Metadata {
    pub(crate) generation: u32,
    /// Each port holding metadata: its fields, in order, and their crc32 as
    /// carried.
    ports: BTreeMap<MeshPort, (Vec<PortMetadataField>, u32)>,
    pull: Option<IndexPull>,
    /// The holder this client last asked for a port's metadata, and when:
    /// its answer is taken for [`METADATA_PULL_WINDOW`].
    asked: Option<(DeviceUid, Instant)>,
    /// When this client last offered its own index to its controller.
    offered: Option<Instant>,
    /// When the controller's index is next due, `None` until this client
    /// first runs as controller.
    pub(crate) index_due: Option<Instant>,
    /// Frames to send, each to one device or, with `None`, to every unit.
    pub(crate) outbox: Vec<(Option<DeviceUid>, Vec<u8>)>,
}

/// Why metadata can't be set or restored.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum MetadataRefusal {
    /// A field is invalid, or there are too many or too long.
    Invalid,
    /// A port is listed twice.
    Repeated,
    /// A key is set twice, which a unit's API refuses though it takes one
    /// on the wire.
    RepeatedKey,
}

impl Metadata {
    /// The fields of `port`, `None` when it holds none.
    pub(crate) fn get(&self, port: MeshPort) -> Option<Vec<PortMetadataField>> {
        self.ports.get(&port).map(|(fields, _)| fields.clone())
    }

    /// All the metadata held, with the generation.
    pub(crate) fn store(&self) -> PortMetadataStore {
        PortMetadataStore {
            generation: self.generation,
            ports: self
                .ports
                .iter()
                .map(|(port, (fields, _))| PortMetadata {
                    port: *port,
                    fields: fields.clone(),
                })
                .collect(),
        }
    }

    /// Replaces everything held with `store`, as a unit loads what it
    /// persisted. A port of no fields is left out.
    pub(crate) fn restore(&mut self, store: &PortMetadataStore) -> Result<(), MetadataRefusal> {
        let mut ports = BTreeMap::new();
        for p in &store.ports {
            let encoded = encode_port_metadata(&p.fields).ok_or(MetadataRefusal::Invalid)?;
            if p.fields.is_empty() {
                continue;
            }
            if ports
                .insert(p.port, (p.fields.clone(), crc32(&encoded)))
                .is_some()
            {
                return Err(MetadataRefusal::Repeated);
            }
        }
        self.ports = ports;
        self.generation = store.generation;
        self.pull = None;
        Ok(())
    }

    /// Records that `holder` was just asked for a port's metadata, whose
    /// answer is then taken from it for [`METADATA_PULL_WINDOW`].
    pub(crate) fn asked_now(&mut self, holder: DeviceUid, now: Instant) {
        self.asked = Some((holder, now));
    }

    /// The index of what is held, as entries.
    fn entries(&self) -> Vec<MetadataIndexEntry> {
        self.ports
            .iter()
            .map(|(port, (_, crc))| MetadataIndexEntry {
                port: *port,
                crc: *crc,
            })
            .collect()
    }

    /// The frames of this client's index, sent as `own`.
    pub(crate) fn index_frames(&self, own: DeviceUid) -> Vec<Vec<u8>> {
        build_metadata_index_pages(own, self.generation, &self.entries())
    }

    /// The metadata frame of `port` as held: no fields when none.
    fn metadata_frame(&self, port: MeshPort) -> Vec<u8> {
        let encoded = self
            .ports
            .get(&port)
            .and_then(|(fields, _)| encode_port_metadata(fields))
            .unwrap_or_default();
        build_port_metadata(port, &encoded)
    }

    /// Sets `port` to `fields`, no fields removing its metadata, reporting
    /// a change as from `device`. Whether anything changed.
    fn put(
        &mut self,
        device: DeviceUid,
        port: MeshPort,
        fields: Vec<PortMetadataField>,
        crc: u32,
        ev: &mut Vec<Event>,
    ) -> bool {
        let changed = if fields.is_empty() {
            self.ports.remove(&port).is_some()
        } else {
            let old = self.ports.insert(port, (fields.clone(), crc));
            old.map_or(true, |(old, _)| old != fields)
        };
        if changed {
            ev.push(Event::PortMetadataChanged {
                device,
                metadata: PortMetadata { port, fields },
            });
        }
        changed
    }

    fn set_generation(&mut self, device: DeviceUid, generation: u32, ev: &mut Vec<Event>) {
        if self.generation != generation {
            self.generation = generation;
            ev.push(Event::PortMetadataGenerationChanged { device, generation });
        }
    }
}

impl State {
    /// Whether this client is the controller of a mesh: its own.
    pub(crate) fn controls_mesh(&self) -> bool {
        !self.mesh_controller.is_zero() && self.mesh_controller == self.uid
    }

    /// Sets `port`'s metadata through this client's own API, as a
    /// controller or a unit in no mesh does: raises the generation when it
    /// changed, and as controller sends the port's metadata to every unit at
    /// once and the index [`METADATA_INDEX_AFTER_CHANGE`] later. No fields
    /// removes the port's metadata.
    pub(crate) fn set_port_metadata(
        &mut self,
        port: MeshPort,
        fields: Vec<PortMetadataField>,
        now: Instant,
        ev: &mut Vec<Event>,
    ) -> Result<(), MetadataRefusal> {
        let encoded = encode_port_metadata(&fields).ok_or(MetadataRefusal::Invalid)?;
        let repeated = fields
            .iter()
            .enumerate()
            .any(|(i, f)| fields[..i].iter().any(|g| g.key == f.key));
        if repeated {
            return Err(MetadataRefusal::RepeatedKey);
        }
        let own = self.uid;
        let m = &mut self.metadata;
        if !m.put(own, port, fields, crc32(&encoded), ev) {
            return Ok(());
        }
        let generation = m.generation.wrapping_add(1);
        m.set_generation(own, generation, ev);
        if self.controls_mesh() {
            let m = &mut self.metadata;
            m.outbox.push((None, build_port_metadata(port, &encoded)));
            m.index_due = Some(now + METADATA_INDEX_AFTER_CHANGE);
        }
        Ok(())
    }

    /// Queues the controller's index when it is due, and schedules the next
    /// one `period` later. Nothing while this client controls no mesh.
    pub(crate) fn check_metadata_index(&mut self, now: Instant, period: Duration) {
        if !self.controls_mesh() {
            return;
        }
        let due = self.metadata.index_due.map_or(true, |at| now >= at);
        if !due {
            return;
        }
        let frames = self.metadata.index_frames(self.uid);
        self.metadata
            .outbox
            .extend(frames.into_iter().map(|f| (None, f)));
        self.metadata.index_due = Some(now + period);
    }

    /// Takes a request for `port`'s metadata: every unit answers the
    /// requester with what it holds, no fields when it holds none.
    pub(crate) fn on_metadata_request(&mut self, sender: DeviceUid, port: MeshPort) {
        let frame = self.metadata.metadata_frame(port);
        self.metadata.outbox.push((Some(sender), frame));
    }

    /// Whether metadata from `sender` is taken as of `now`: from this
    /// client's controller, other than itself, at any time; from the sender
    /// of the index it last took, and the holder it last asked, for
    /// [`METADATA_PULL_WINDOW`].
    fn takes_metadata_from(&self, sender: DeviceUid, now: Instant) -> bool {
        let within = |at: Instant| now.saturating_duration_since(at) < METADATA_PULL_WINDOW;
        (!self.mesh_controller.is_zero() && sender == self.mesh_controller && sender != self.uid)
            || self
                .metadata
                .pull
                .as_ref()
                .is_some_and(|p| p.from == sender && within(p.started))
            || self
                .metadata
                .asked
                .is_some_and(|(holder, at)| holder == sender && within(at))
    }

    /// Takes a port's metadata, `body` its fields as carried, `total` and
    /// `crc` as its header gives them. Dropped from a sender it is not taken
    /// from, and when the body is not `total` bytes, fails the crc32, or
    /// holds a field a unit would refuse.
    pub(crate) fn on_port_metadata(
        &mut self,
        sender: DeviceUid,
        port: MeshPort,
        frame: MetadataFrame<'_>,
        now: Instant,
        ev: &mut Vec<Event>,
    ) {
        let MetadataFrame {
            total, crc, body, ..
        } = frame;
        if !self.takes_metadata_from(sender, now) {
            return;
        }
        if body.len() != usize::from(total) || body.len() > PORT_METADATA_MAX {
            return;
        }
        if crc32(body) != crc {
            return;
        }
        let Some(fields) = decode_port_metadata(body) else {
            return;
        };
        self.metadata.put(sender, port, fields, crc, ev);
    }

    /// Takes a frame of an index from `sender`: `first` the index of its
    /// first entry, `total` the entries of the whole index and `crc` their
    /// crc32, `body` the generation and this frame's entries.
    ///
    /// A first frame starts a new index, in place of one under way, when it
    /// is taken at all: a member takes its controller's when it names the
    /// member's generation or a newer one, and answers an older one with
    /// its own index, at most every [`METADATA_OFFER_EVERY`]; the controller
    /// takes a member's when it names a newer generation. A later frame is
    /// taken only from the same sender, with the same crc32, starting at the
    /// first entry not yet taken.
    pub(crate) fn on_metadata_index(
        &mut self,
        sender: DeviceUid,
        frame: MetadataFrame<'_>,
        now: Instant,
        ev: &mut Vec<Event>,
    ) {
        let MetadataFrame {
            first,
            total,
            crc,
            body,
        } = frame;
        let Some((generation, entries)) = decode_metadata_index(body) else {
            return;
        };
        if first == 0 {
            if !self.starts_metadata_pull(sender, generation, now) {
                return;
            }
            self.metadata.pull = Some(IndexPull {
                from: sender,
                generation,
                total,
                crc,
                taken: 0,
                named: BTreeSet::new(),
                started: now,
                complete: false,
            });
        }
        let m = &mut self.metadata;
        let Some(pull) = m.pull.as_mut() else {
            return;
        };
        if pull.complete
            || pull.from != sender
            || pull.crc != crc
            || pull.taken != usize::from(first)
        {
            return;
        }
        pull.taken += entries.len();
        for entry in &entries {
            pull.named.insert(entry.port);
            let held = m.ports.get(&entry.port).map(|(_, crc)| *crc);
            if held != Some(entry.crc) {
                m.outbox
                    .push((Some(sender), build_port_metadata_request(entry.port)));
            }
        }
        if pull.taken < usize::from(pull.total) {
            return;
        }
        pull.complete = true;
        let named = std::mem::take(&mut pull.named);
        let generation = pull.generation;
        let dropped: Vec<MeshPort> = m
            .ports
            .keys()
            .filter(|port| !named.contains(port))
            .copied()
            .collect();
        for port in dropped {
            m.put(sender, port, Vec::new(), 0, ev);
        }
        m.set_generation(sender, generation, ev);
    }

    /// Whether a first index frame from `sender` naming `generation` starts
    /// an index this client takes, queuing its own index for its controller
    /// when the controller's names an older generation.
    fn starts_metadata_pull(&mut self, sender: DeviceUid, generation: u32, now: Instant) -> bool {
        let controller = self.mesh_controller;
        if controller.is_zero() || sender == self.uid {
            return false;
        }
        if self.controls_mesh() {
            let member = self
                .device(sender)
                .and_then(|d| d.reported_mesh_master())
                .is_some_and(|master| master == self.uid);
            return member && generation > self.metadata.generation;
        }
        if sender != controller {
            return false;
        }
        if generation >= self.metadata.generation {
            return true;
        }
        let m = &mut self.metadata;
        let due = m.offered.map_or(true, |at| {
            now.saturating_duration_since(at) >= METADATA_OFFER_EVERY
        });
        if due {
            m.offered = Some(now);
            let frames = m.index_frames(self.uid);
            m.outbox
                .extend(frames.into_iter().map(|f| (Some(controller), f)));
        }
        false
    }
}
