// Author: Lars Op den Kamp (lars@opdenkamp-it.nl)
// Copyright (c) 2026 Op den Kamp IT Solutions

//! The device registry and everything reachable from it.
//!
//! State is addressed by identifier rather than by reference: a device is a
//! [`DeviceUid`] and a bay a [`BayUid`]. A cyclic pointer graph is the shape
//! this naturally wants, and neither Rust's borrow rules nor a C ABI can carry
//! one; the same identifiers serve both.
//!
//! Every mutator takes the event queue it appends to. The queue is drained
//! after the state lock is released, so an event handler may call back into
//! the library.

mod bay;
mod controller;
mod device;
mod links;
mod tree;

use std::collections::hash_map::DefaultHasher;
use std::collections::{HashMap, VecDeque};
use std::hash::{Hash, Hasher};
use std::net::Ipv4Addr;
use std::time::Duration;
use std::time::Instant;

use crate::event::Event;
use crate::types::{MacAddress, MeshBays, MeshConfig, OsdSyncRequest};
use crate::wire::{BayFeatures, BayUid, DeviceFeature, DeviceUid, LinkFeature};

pub(crate) use bay::Bay;
pub(crate) use device::{Device, HelloInfo, MeshBayChange, SdpPages, CONFIG_GRACE};
pub(crate) use links::{BayLink, BayLinks};
use tree::Tree;

/// Every device this client has heard from, and the links between their bays.
#[derive(Debug)]
pub(crate) struct State {
    /// This client's own identifier, so its own frames can be ignored.
    pub(crate) uid: DeviceUid,
    /// The mesh controller this client follows, zero for none: what a unit
    /// judges a write to itself against.
    pub(crate) mesh_controller: DeviceUid,
    pub(crate) devices: HashMap<DeviceUid, Device>,
    pub(crate) links: BayLinks,
    /// How many frames from other senders have parsed, whatever they carried.
    pub(crate) frames_received: u64,
    /// A peer asked this client to announce itself, and it has not yet.
    pub(crate) hello_requested: bool,
    /// Devices that may hold state this client never received, and have not
    /// yet been asked for it.
    pub(crate) state_requests: Vec<DeviceUid>,
    /// Overlay sync requests this client may be the one to answer: those
    /// asking for a new sync, and those naming this client as the source.
    pub(crate) osd_requests: Vec<OsdSyncRequest>,
    /// When this client last asked for an overlay sync of its own (seq 0):
    /// a sync naming it is taken from any device for 10 s after.
    pub(crate) osd_sync_asked: Option<Instant>,
    /// The syncs this client last asked settings of, as their source and
    /// seq, newest last: their settings are taken from that source.
    pub(crate) osd_settings_asked: VecDeque<(DeviceUid, u16)>,
    /// The kept settings of each unit, by that unit, as a device last sent
    /// them whole.
    pub(crate) mesh_configs: HashMap<DeviceUid, MeshConfig>,
    /// Each controller's numbering, by controller, as a device last sent it
    /// whole.
    pub(crate) mesh_bays: HashMap<DeviceUid, MeshBays>,
    /// The settings each sender is still sending, by sender.
    pub(crate) mesh_config_transfers: HashMap<DeviceUid, MeshConfigTransfer>,
    /// The lister and the unit of this client's last request to replace one,
    /// until it is answered.
    pub(crate) mesh_replace_pending: Option<(DeviceUid, DeviceUid)>,
    /// When this client last asked which units it can replace.
    pub(crate) mesh_replace_queried: Option<Instant>,
    /// A dedicated controller whose `CONTROLLER_STATE` requests and data are
    /// taken though it is of no mesh of this client's: one replacing a router
    /// of it.
    pub(crate) controller_state_allowed: Option<DeviceUid>,
    /// `CONTROLLER_STATE` requests to answer, as their sender and header.
    pub(crate) controller_state_requests: Vec<(DeviceUid, crate::wire::StateHeader)>,
    /// The parts of other dedicated controllers' state being pulled.
    pub(crate) controller_pulls: Vec<controller::ControllerPull>,
    /// `CONTROLLER_STATE` requests the pulls raised, to send, as their
    /// addressee and payload.
    pub(crate) controller_asks: Vec<(DeviceUid, Vec<u8>)>,
    /// Where each registered device, this client included, sits.
    tree: Tree,
    /// The source address of the last hello captured from each sender below
    /// the IP stack, and when.
    hello_sources: HashMap<DeviceUid, (MacAddress, Instant)>,
    /// A digest and arrival time of each datagram received in the last
    /// [`DUPLICATE_WINDOW`], at most [`DUPLICATE_RING`] of them.
    recent: VecDeque<(u64, Instant)>,
}

/// The settings of one unit a sender is part way through sending: every
/// frame names the same kind, unit, length and crc32, and brings the bytes
/// that follow the ones already here.
#[derive(Debug)]
pub(crate) struct MeshConfigTransfer {
    pub(crate) kind: u8,
    pub(crate) flags: u8,
    pub(crate) subject: DeviceUid,
    pub(crate) total: u16,
    pub(crate) crc: u32,
    pub(crate) bytes: Vec<u8>,
}

/// How long a datagram identical to one already received counts as its
/// second copy. Short, because a key press carries nothing that tells two
/// presses apart.
const DUPLICATE_WINDOW: Duration = Duration::from_millis(100);

/// How long the captured source address of a hello waits for the hello's UDP
/// copy to register its sender.
const HELLO_SOURCE_PENDING: Duration = Duration::from_secs(10);

/// How many recent datagrams the duplicate check compares against.
const DUPLICATE_RING: usize = 64;

impl State {
    pub(crate) fn new(uid: DeviceUid) -> Self {
        Self {
            uid,
            mesh_controller: DeviceUid::ZERO,
            devices: HashMap::new(),
            links: BayLinks::default(),
            frames_received: 0,
            hello_requested: false,
            state_requests: Vec::new(),
            osd_requests: Vec::new(),
            osd_sync_asked: None,
            osd_settings_asked: VecDeque::new(),
            mesh_configs: HashMap::new(),
            mesh_bays: HashMap::new(),
            mesh_config_transfers: HashMap::new(),
            mesh_replace_pending: None,
            mesh_replace_queried: None,
            controller_state_allowed: None,
            controller_state_requests: Vec::new(),
            controller_pulls: Vec::new(),
            controller_asks: Vec::new(),
            tree: Tree::new(uid),
            hello_sources: HashMap::new(),
            recent: VecDeque::with_capacity(DUPLICATE_RING),
        }
    }

    /// Whether `data` repeats a datagram received within
    /// [`DUPLICATE_WINDOW`] of `now`, recording it when it does not.
    ///
    /// A frame sent to both endpoints arrives once by each, and the first
    /// copy is the one acted on.
    pub(crate) fn is_duplicate(&mut self, data: &[u8], now: Instant) -> bool {
        let mut hasher = DefaultHasher::new();
        data.hash(&mut hasher);
        let digest = hasher.finish();
        self.recent
            .retain(|(_, at)| now.saturating_duration_since(*at) < DUPLICATE_WINDOW);
        if self.recent.iter().any(|(d, _)| *d == digest) {
            return true;
        }
        if self.recent.len() == DUPLICATE_RING {
            self.recent.pop_front();
        }
        self.recent.push_back((digest, now));
        false
    }

    /// Whether some peer has to be reached by broadcast as well: it has not
    /// been heard by multicast within the path window, or it reports that its
    /// own multicast fails. Every registered peer counts, one gone silent
    /// included, as a unit judges by every peer it knows.
    pub(crate) fn needs_broadcast(&self, now: Instant) -> bool {
        self.peers().any(|d| {
            d.broadcast_only(now) || d.hello.features.has(DeviceFeature::STATUS_MCAST_FAULT)
        })
    }

    /// Whether this client's own multicast fails: a peer, not about to
    /// reboot, is heard by broadcast but not by multicast.
    pub(crate) fn multicast_fault(&self, now: Instant) -> bool {
        self.peers().any(|d| {
            !d.hello.features.has(DeviceFeature::STATUS_REBOOT)
                && d.broadcast_only(now)
                && d.heard_broadcast_lately(now)
        })
    }

    fn peers(&self) -> impl Iterator<Item = &Device> + '_ {
        let own = self.uid;
        self.devices.values().filter(move |d| d.uid != own)
    }

    /// Whether `sender` is management to this client, as a unit judges it
    /// against its own stored controller: that controller, or a management
    /// application.
    pub(crate) fn manages_this_client(&self, sender: DeviceUid) -> bool {
        sender == self.mesh_controller
            || self
                .device(sender)
                .is_some_and(|d| d.hello.features.has(DeviceFeature::MANAGER))
    }

    pub(crate) fn device(&self, uid: DeviceUid) -> Option<&Device> {
        self.devices.get(&uid)
    }

    pub(crate) fn device_mut(&mut self, uid: DeviceUid) -> Option<&mut Device> {
        self.devices.get_mut(&uid)
    }

    pub(crate) fn device_by_serial(&self, serial: &str) -> Option<&Device> {
        self.devices.values().find(|d| d.hello.serial == serial)
    }

    pub(crate) fn bay(&self, uid: BayUid) -> Option<&Bay> {
        self.device(uid.device)?.bay(uid.port)
    }

    pub(crate) fn bay_mut(&mut self, uid: BayUid) -> Option<&mut Bay> {
        self.device_mut(uid.device)?.bay_mut(uid.port)
    }

    /// The bay a mesh bay stands for: bay 0 in the same direction on the unit
    /// it maps to, once that unit has reported it. `None` for a bay that is
    /// not a mesh bay.
    pub(crate) fn mesh_owner(&self, bay: &Bay) -> Option<&Bay> {
        if !bay.is_v2ip_remote() {
            return None;
        }
        self.device(bay.v2ip_uid)?.local_bay(bay.is_input())
    }

    /// What describes `bay`: the bay a mesh bay stands for, or `bay` itself.
    pub(crate) fn described<'a>(&'a self, bay: &'a Bay) -> &'a Bay {
        self.mesh_owner(bay).unwrap_or(bay)
    }

    /// The input bay on `device` carrying the given user-assigned name.
    ///
    /// Inputs only, and hidden ones are skipped: this resolves the name a
    /// picker shows, and a picker does not offer what it does not list.
    pub(crate) fn input_by_user_name<'a>(
        &'a self,
        device: &'a Device,
        name: &str,
    ) -> Option<&'a Bay> {
        device.bays.values().filter(|b| b.is_input()).find(|b| {
            let d = self.described(b);
            d.hidden != Some(true) && d.user_name() == name
        })
    }

    /// Repeats each change to a unit's own bay 0 for the mesh bays standing
    /// for it, which describe themselves through it.
    ///
    /// A request that arrived on the bay, such as a key press, concerns the
    /// bay it arrived on and is not repeated.
    pub(crate) fn mirror_mesh_events(&self, ev: &mut Vec<Event>) {
        let mut mirrored = Vec::new();
        for event in ev.iter() {
            let Some(uid) = event.mirrored_bay() else {
                continue;
            };
            for device in self.devices.values() {
                for bay in device.bays.values() {
                    if self.mesh_owner(bay).is_some_and(|o| o.uid() == uid) {
                        mirrored.push(event.for_bay(bay.uid()));
                    }
                }
            }
        }
        ev.extend(mirrored);
    }

    /// The bay a link record names: a device serial and a port name.
    pub(crate) fn bay_by_serial_and_name(&self, serial: &str, port_name: &str) -> Option<BayUid> {
        Some(self.device_by_serial(serial)?.bay_by_name(port_name)?.uid())
    }

    /// The source bay advertising the given multicast group.
    ///
    /// A OneIP sink names its route by group address, so this is what turns a
    /// stream subscription back into the bay producing it.
    pub(crate) fn bay_by_stream_ip(&self, ip: Ipv4Addr, audio: bool) -> Option<BayUid> {
        self.devices.values().find_map(|device| {
            let input = device.first_input()?;
            let source = device.v2ip_source_for(input)?;
            let stream = if audio { source.audio } else { source.video };
            (stream.ip == ip).then(|| self.described(input).uid())
        })
    }

    /// The cross-device identity the link registry keys a bay by.
    pub(crate) fn link_key(&self, uid: BayUid) -> Option<BayUid> {
        let device = self.device(uid.device)?;
        Some(device.link_key(device.bay(uid.port)?))
    }

    /// The bay on another device that this bay's link record names, once that
    /// bay has been discovered.
    ///
    /// Only this bay's own half of the link is needed. The far half decides
    /// what media the link carries, because that is read from both ends'
    /// features, but the far bay is addressable as soon as it is known.
    pub(crate) fn linked_bay(&self, uid: BayUid) -> Option<BayUid> {
        let link = self.links.get(self.link_key(uid)?)?;
        if !link.is_configured() {
            return None;
        }
        self.bay_by_serial_and_name(&link.linked_serial, &link.linked_bay)
    }

    /// The bay carrying the volume for `uid`, which is `uid` itself unless the
    /// mesh put its volume control on another device.
    ///
    /// A OneIP output wired to an amplifier zone has no volume of its own: the
    /// link is how the mesh says the zone is what that output's volume means.
    /// Reads and commands both resolve through this, so addressing the output
    /// reaches the amplifier the installer wired it to.
    pub(crate) fn volume_bay(&self, uid: BayUid) -> BayUid {
        if self.bay(uid).is_some_and(Bay::has_volume_control) {
            return uid;
        }
        match self.linked_bay(uid) {
            Some(far) if self.bay(far).is_some_and(Bay::has_volume_control) => far,
            _ => uid,
        }
    }

    /// Registers or refreshes a device from a hello frame.
    pub(crate) fn apply_hello(
        &mut self,
        uid: DeviceUid,
        hello: HelloInfo,
        now: Instant,
        ev: &mut Vec<Event>,
    ) {
        let tree = &mut self.tree;
        self.devices
            .entry(uid)
            .or_insert_with(|| {
                tree.add(uid);
                Device::new(uid, hello.clone(), now)
            })
            .apply_hello(hello, now, ev);
    }

    /// Every device and this client, in the order a unit lists them: depth
    /// first, a parent before its children, with how deep each sits.
    pub(crate) fn device_tree(&self) -> Vec<(DeviceUid, usize)> {
        self.tree.walk()
    }

    /// Reshapes the tree by a topology list `sender` reported, as a unit does
    /// with one differing from the last.
    pub(crate) fn apply_topology(&mut self, sender: DeviceUid, listed: &[DeviceUid]) {
        self.tree.apply_topology(sender, listed);
    }

    /// Every device, in the order a unit lists them, this client left out.
    pub(crate) fn devices_in_order(&self) -> Vec<DeviceUid> {
        let own = self.uid;
        self.tree
            .walk()
            .into_iter()
            .map(|(uid, _)| uid)
            .filter(|&uid| uid != own)
            .collect()
    }

    /// Forgets every device silent long enough for a unit to forget it, with
    /// the links it reported and the source address of its hellos, as of
    /// `now`.
    pub(crate) fn forget_silent(&mut self, now: Instant, ev: &mut Vec<Event>) {
        let silent: Vec<DeviceUid> = self
            .devices
            .values()
            .filter(|d| d.is_forgotten(now))
            .map(|d| d.uid)
            .collect();
        for device in silent {
            self.devices.remove(&device);
            self.tree.remove(device);
            self.links.forget_device(device);
            ev.push(Event::DeviceUnregistered { device });
        }
        // A capture of a hello no device was registered by within the window
        // came from a sender this client does not take hellos from, or from
        // one just forgotten, whose last hello is older than that.
        let devices = &self.devices;
        self.hello_sources.retain(|uid, (_, at)| {
            devices.contains_key(uid) || now.saturating_duration_since(*at) < HELLO_SOURCE_PENDING
        });
    }

    /// Records the source address a hello from `uid` was captured with.
    ///
    /// The capture and the UDP copy of the hello arrive by different paths in
    /// either order, so one for a sender not registered yet is kept for
    /// [`HELLO_SOURCE_PENDING`].
    pub(crate) fn note_hello_source(&mut self, uid: DeviceUid, mac: MacAddress, now: Instant) {
        self.hello_sources.insert(uid, (mac, now));
    }

    /// The source address of the last hello captured from `uid`.
    pub(crate) fn hello_source(&self, uid: DeviceUid) -> Option<MacAddress> {
        self.hello_sources.get(&uid).map(|(mac, _)| *mac)
    }

    /// Records the link one bay reports, announcing it to both ends.
    ///
    /// A device reports only its own half of a link. The two halves are
    /// matched by each naming the other's serial and port name, and a link
    /// counts as connected only once both halves agree - which is what decides
    /// the media a link carries, since that is read from the two bays'
    /// features.
    pub(crate) fn update_link(
        &mut self,
        origin: BayUid,
        linked_serial: String,
        linked_bay: String,
        features: u32,
        ev: &mut Vec<Event>,
    ) {
        let Some(key) = self.link_key(origin) else {
            return;
        };
        let Some((origin_serial, origin_bay_name)) = self.bay_identity(origin) else {
            return;
        };
        let new = BayLink {
            linked_serial,
            linked_bay,
            features,
        };

        if let Some(old) = self.links.get(key) {
            // A repeat of the same record is the device re-sending its
            // configuration, not a change.
            if old == &new {
                return;
            }
            if old.is_configured() {
                let old_serial = old.linked_serial.clone();
                let far = self.bay_by_serial_and_name(&old_serial, &old.linked_bay.clone());
                ev.push(Event::BayUnlinked {
                    bay: origin,
                    linked_serial: old_serial,
                    bay_name: origin_bay_name.clone(),
                });
                if let Some(far) = far {
                    ev.push(Event::BayUnlinked {
                        bay: far,
                        linked_serial: origin_serial.clone(),
                        bay_name: origin_bay_name.clone(),
                    });
                }
            }
        }

        self.links.insert(key, new.clone());
        if !new.is_configured() {
            return;
        }
        let far = self.bay_by_serial_and_name(&new.linked_serial, &new.linked_bay);
        let features = self.link_features(origin, far);
        ev.push(Event::BayLinked {
            bay: origin,
            linked_serial: new.linked_serial.clone(),
            bay_name: origin_bay_name.clone(),
            features,
        });
        if let Some(far) = far {
            ev.push(Event::BayLinked {
                bay: far,
                linked_serial: origin_serial,
                bay_name: origin_bay_name,
                features,
            });
        }
    }

    /// The serial of a bay's device and the bay's own port name.
    fn bay_identity(&self, uid: BayUid) -> Option<(String, String)> {
        let device = self.device(uid.device)?;
        let bay = device.bay(uid.port)?;
        Some((device.serial().to_owned(), bay.port_name.clone()))
    }

    /// The media a link carries, which is the pairing of the two bays'
    /// connectors and is empty until both ends name each other.
    fn link_features(&self, origin: BayUid, far: Option<BayUid>) -> LinkFeature {
        let Some(far) = far else {
            return LinkFeature::NONE;
        };
        if !self.link_is_connected(origin, far) {
            return LinkFeature::NONE;
        }
        let (Some(left), Some(right)) = (self.bay(origin), self.bay(far)) else {
            return LinkFeature::NONE;
        };
        let (left, right) = (left.features, right.features);
        let mut rv = LinkFeature::NONE;
        let pairs = [
            (
                BayFeatures::HDMI_OUT,
                BayFeatures::HDMI_IN,
                LinkFeature::VIDEO_HDMI,
            ),
            (
                BayFeatures::AUDIO_DIG_OUT,
                BayFeatures::AUDIO_DIG_IN,
                LinkFeature::AUDIO_OPTICAL,
            ),
            (
                BayFeatures::AUDIO_ANA_OUT,
                BayFeatures::AUDIO_ANA_IN,
                LinkFeature::AUDIO_ANALOG,
            ),
            (BayFeatures::IR_OUT, BayFeatures::IR_IN, LinkFeature::IR),
            (BayFeatures::RC_OUT, BayFeatures::RC_IN, LinkFeature::RC),
        ];
        for (out, into, feature) in pairs {
            if (left.has(out) && right.has(into)) || (left.has(into) && right.has(out)) {
                rv |= feature;
            }
        }
        rv
    }

    /// Whether the far bay's own link record names the origin back.
    fn link_is_connected(&self, origin: BayUid, far: BayUid) -> bool {
        let Some(far_key) = self.link_key(far) else {
            return false;
        };
        let Some(far_link) = self.links.get(far_key) else {
            return false;
        };
        let Some((origin_serial, origin_bay_name)) = self.bay_identity(origin) else {
            return false;
        };
        far_link.is_configured()
            && far_link.linked_serial == origin_serial
            && far_link.linked_bay == origin_bay_name
    }
}
