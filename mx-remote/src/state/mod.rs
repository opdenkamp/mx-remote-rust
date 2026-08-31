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
mod device;
mod links;

use std::collections::HashMap;
use std::net::Ipv4Addr;
use std::time::Instant;

use crate::event::Event;
use crate::wire::{BayFeatures, BayUid, DeviceUid, LinkFeature};

pub(crate) use bay::Bay;
pub(crate) use device::{Device, HelloInfo};
pub(crate) use links::{BayLink, BayLinks};

/// Every device this client has heard from, and the links between their bays.
#[derive(Debug)]
pub(crate) struct State {
    /// This client's own identifier, so its own frames can be ignored.
    pub(crate) uid: DeviceUid,
    pub(crate) devices: HashMap<DeviceUid, Device>,
    pub(crate) links: BayLinks,
    /// How many frames from other senders have parsed, whatever they carried.
    pub(crate) frames_received: u64,
}

impl State {
    pub(crate) fn new(uid: DeviceUid) -> Self {
        Self {
            uid,
            devices: HashMap::new(),
            links: BayLinks::default(),
            frames_received: 0,
        }
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

    /// The bay a link record names: a device serial and a port name.
    pub(crate) fn bay_by_serial_and_name(&self, serial: &str, port_name: &str) -> Option<BayUid> {
        Some(self.device_by_serial(serial)?.bay_by_name(port_name)?.uid())
    }

    /// The source bay advertising the given multicast group.
    ///
    /// A V2IP sink names its route by group address, so this is what turns a
    /// stream subscription back into the bay producing it.
    pub(crate) fn bay_by_stream_ip(&self, ip: Ipv4Addr, audio: bool) -> Option<BayUid> {
        self.devices.values().find_map(|device| {
            let input = device.first_input()?;
            let source = device.v2ip_source_for(input)?;
            let stream = if audio { source.audio } else { source.video };
            (stream.ip == ip).then(|| input.uid())
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
        self.devices
            .entry(uid)
            .or_insert_with(|| Device::new(uid, hello.clone(), now))
            .apply_hello(hello, now, ev);
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
            if old.linked_serial == new.linked_serial && old.features == new.features {
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
