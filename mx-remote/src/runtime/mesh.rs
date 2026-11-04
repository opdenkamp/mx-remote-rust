// Author: Lars Op den Kamp (lars@opdenkamp-it.nl)
// Copyright (c) 2026 Op den Kamp IT Solutions

//! Replacing a unit of a mesh, and the settings a mesh keeps of its units.
//!
//! A unit outside any mesh asks which offline units it can replace. Each
//! controller lists its own, and a member whose controller is offline lists
//! that controller. The unit then asks the device that listed one to replace
//! it, and the device answers.
//!
//! Every unit in a mesh keeps its own settings, and announces their length
//! and crc32. The controller keeps a copy of every member's, and every member
//! one of its controller's, so the settings of a unit that fails pass to the
//! unit that replaces it. Any device may ask for a unit's settings, from the
//! unit itself or from a device keeping a copy.
//!
//! These are served by a loadable module, which answers nothing a unit
//! without it is sent.

use std::time::{Duration, Instant};

use crate::rx::replace_list_kept;
use crate::types::{
    MeshConfig, MeshConfigDigest, MeshReplaceUnit, MeshReplaceable, MeshSetting,
    MESH_REPLACE_NAME_MAX, MESH_REPLACE_UNITS_MAX,
};
use crate::wire::{
    build_mesh_config_hello, build_mesh_config_pages, build_mesh_config_request,
    build_mesh_replace_query, build_mesh_replace_target, build_mesh_replace_units, config_kind,
    encode_mesh_settings, op, replace_kind, Addressee, DeviceFeature, DeviceUid, MeshReplaceResult,
    MESH_CONFIG_FLAG_COPY,
};

use super::control::{device_of, Command, ControlError};
use super::{Remote, CLIENT_SERIAL};

/// How often a unit asks which units it can replace, at most.
const QUERY_INTERVAL: Duration = Duration::from_secs(2);

/// The features a unit replacing another must have of the other's.
const REPLACE_FEATURES: DeviceFeature = DeviceFeature::from_bits(
    DeviceFeature::V2IP_SOURCE.bits()
        | DeviceFeature::V2IP_SINK.bits()
        | DeviceFeature::MULTIVIEWER.bits(),
);

/// Whether a unit counts `serial` as a default one, which it refuses to have
/// replace another: every serial starting `P9SN0000` but this client's
/// default.
fn is_default_serial(serial: &str) -> bool {
    serial.starts_with("P9SN0000") && serial != CLIENT_SERIAL
}

/// Encodes settings to send, refusing what no unit takes.
fn encoded(settings: &[MeshSetting]) -> Result<Vec<u8>, ControlError> {
    encode_mesh_settings(settings).ok_or(ControlError::InvalidRequest(
        "kept settings are names and values of at most 255 bytes, with no NUL, \
         of at most 1984 bytes together",
    ))
}

impl Remote {
    // ---- replacing a unit of a mesh ----

    /// Asks every device which units this client can replace.
    ///
    /// Every controller answers with its offline units, and every member
    /// whose controller is offline with that controller.
    /// [`Remote::mesh_replaceable`] then reads them. A unit lists none until
    /// its controller has been up 120 s, and a device without the mesh
    /// module answers nothing.
    ///
    /// A call within 2 s of the last sends nothing, as a unit asks no more
    /// often: the answers to the last are still arriving.
    pub fn query_mesh_replaceable(&self) -> Result<(), ControlError> {
        let now = Instant::now();
        let due = self.shared.mutate(|state, _| {
            let due = state.mesh_replace_queried.map_or(true, |at| {
                now.saturating_duration_since(at) >= QUERY_INTERVAL
            });
            if due {
                state.mesh_replace_queried = Some(now);
            }
            due
        });
        if due {
            self.shared.send(
                &Addressee::Broadcast,
                op::MESH_REPLACE,
                &build_mesh_replace_query(),
            )?;
        }
        Ok(())
    }

    /// The units this client can ask to replace, each with the device that
    /// listed it, as listed in the last 30 s. A unit several members list
    /// appears once.
    pub fn mesh_replaceable(&self) -> Vec<MeshReplaceable> {
        let now = Instant::now();
        self.shared.read(|state| {
            let mut found: Vec<MeshReplaceable> = Vec::new();
            let mut listers: Vec<_> = state
                .devices
                .values()
                .filter_map(|d| Some((d.uid, d.mesh_replace_units.as_ref()?)))
                .filter(|(_, (_, at))| replace_list_kept(*at, now))
                .collect();
            listers.sort_by_key(|(uid, _)| *uid);
            for (lister, (list, _)) in listers {
                for unit in &list.units {
                    if found.iter().all(|f| f.unit.uid != unit.uid) {
                        found.push(MeshReplaceable {
                            lister,
                            offline_controller: list.offline_controller,
                            unit: unit.clone(),
                        });
                    }
                }
            }
            found
        })
    }

    /// Asks `lister` to replace `unit` with this client: the device takes
    /// over the unit's bays, their names and routes, and its kept settings.
    /// It answers at [`crate::EventHandler::on_mesh_replace_answered`].
    ///
    /// Asking a member to replace its offline controller makes it send the
    /// controller's kept settings to this client, at
    /// [`crate::EventHandler::on_mesh_config_received`]. Taking over is then
    /// this client's own: promoting itself to controller in place of the
    /// old one, and having the members replace it
    /// ([`Remote::send_mesh_operation`]).
    ///
    /// Refused here for what the device would refuse: `lister` has not
    /// listed `unit` in the last 30 s, this client follows a mesh controller,
    /// or its serial is a default one, or it lacks a OneIP source, OneIP sink
    /// or multiviewer feature `unit` has.
    pub fn request_mesh_replace(
        &self,
        lister: DeviceUid,
        unit: DeviceUid,
    ) -> Result<(), ControlError> {
        let features = self.shared.features();
        let default_serial = is_default_serial(&self.shared.serial);
        self.shared.command(move |state| {
            let now = Instant::now();
            let d = device_of(state, lister)?;
            if !state.mesh_controller.is_zero() {
                return Err(ControlError::InvalidRequest(
                    "a unit in a mesh replaces no other",
                ));
            }
            if default_serial {
                return Err(ControlError::InvalidRequest(
                    "a unit on a default serial replaces no other",
                ));
            }
            let listed = d
                .mesh_replace_units
                .as_ref()
                .filter(|(_, at)| replace_list_kept(*at, now))
                .and_then(|(list, _)| list.units.iter().find(|u| u.uid == unit))
                .ok_or(ControlError::NotReported(
                    "the unit among those the device can have replaced",
                ))?;
            if !features.has(listed.features & REPLACE_FEATURES) {
                return Err(ControlError::Unsupported(
                    "this client lacks a feature of the unit",
                ));
            }
            Ok(Command::new(
                Addressee::direct(d, d.heard_path(now)),
                op::MESH_REPLACE,
                build_mesh_replace_target(replace_kind::REQUEST, unit, MeshReplaceResult::REPLACED),
            )
            .then(move |state, _| state.mesh_replace_pending = Some((lister, unit))))
        })
    }

    /// Answers a unit's query with the units this client, as controller,
    /// can have replaced, or with its offline controller, as a member.
    ///
    /// A controller lists a unit while it has bays on the controller, is not
    /// the controller, and is offline. At most 32 units are listed, and a
    /// name is at most 16 bytes. An empty list is sent all the same, so the
    /// unit knows this client has none.
    pub fn send_mesh_replaceable(
        &self,
        to: DeviceUid,
        offline_controller: bool,
        units: &[MeshReplaceUnit],
    ) -> Result<(), ControlError> {
        if units.len() > MESH_REPLACE_UNITS_MAX {
            return Err(ControlError::InvalidRequest(
                "a controller lists at most 32 units",
            ));
        }
        let long = |name: &str| name.len() > MESH_REPLACE_NAME_MAX;
        if units
            .iter()
            .any(|u| long(&u.input_name) || long(&u.output_name))
        {
            return Err(ControlError::InvalidRequest(
                "a replaceable unit's bay name is at most 16 bytes",
            ));
        }
        let frames = build_mesh_replace_units(units, offline_controller);
        self.shared.commands(move |state| {
            let d = device_of(state, to)?;
            let addressee = Addressee::direct(d, d.heard_path(Instant::now()));
            Ok(frames
                .into_iter()
                .map(|p| Command::new(addressee.clone(), op::MESH_REPLACE, p))
                .collect())
        })
    }

    /// Answers a unit's request to replace `unit` with it.
    pub fn send_mesh_replace_result(
        &self,
        to: DeviceUid,
        unit: DeviceUid,
        result: MeshReplaceResult,
    ) -> Result<(), ControlError> {
        self.shared.command(move |state| {
            let d = device_of(state, to)?;
            Ok(Command::new(
                Addressee::direct(d, d.heard_path(Instant::now())),
                op::MESH_REPLACE,
                build_mesh_replace_target(replace_kind::RESULT, unit, result),
            ))
        })
    }

    // ---- the settings a mesh keeps of its units ----

    /// The length and crc32 of a unit's kept settings, as it last announced
    /// them, `None` until it has.
    pub fn mesh_config_digest(&self, uid: DeviceUid) -> Option<MeshConfigDigest> {
        self.shared
            .read(|state| state.device(uid)?.mesh_config_digest)
    }

    /// The kept settings of `subject`, as a device last sent them whole to
    /// this client, `None` until one has. Ask for them with
    /// [`Remote::request_mesh_config`].
    pub fn mesh_config(&self, subject: DeviceUid) -> Option<MeshConfig> {
        self.shared
            .read(|state| state.mesh_configs.get(&subject).cloned())
    }

    /// Asks `holder` for the kept settings of `subject`: its own when
    /// `subject` is `holder`, otherwise the copy it keeps. A controller keeps
    /// one of every member, and a member one of its controller. They arrive
    /// at [`crate::EventHandler::on_mesh_config_received`], and
    /// [`Remote::mesh_config`] reads them. A device keeping no copy answers
    /// nothing.
    pub fn request_mesh_config(
        &self,
        holder: DeviceUid,
        subject: DeviceUid,
    ) -> Result<(), ControlError> {
        self.shared.command(move |state| {
            let d = device_of(state, holder)?;
            Ok(Command::new(
                Addressee::direct(d, d.heard_path(Instant::now())),
                op::MESH_CONFIG,
                build_mesh_config_request(subject),
            ))
        })
    }

    /// Announces the length and crc32 of this client's own kept settings to
    /// every device. A unit announces its own every 60 s while it is in a
    /// mesh, and 2 s after one of them changes. Its controller, or a member
    /// whose controller this client is, then asks for them when they differ
    /// from its copy.
    ///
    /// Refused while this client follows no mesh controller, as a unit in no
    /// mesh announces none.
    pub fn send_mesh_config_hello(&self, settings: &[MeshSetting]) -> Result<(), ControlError> {
        let encoded = encoded(settings)?;
        self.shared.command(move |state| {
            if state.mesh_controller.is_zero() {
                return Err(ControlError::InvalidRequest(
                    "a unit in no mesh announces no kept settings",
                ));
            }
            Ok(Command::new(
                Addressee::Broadcast,
                op::MESH_CONFIG,
                build_mesh_config_hello(state.uid, &encoded),
            ))
        })
    }

    /// Sends `to` the kept settings of `subject`, as the answer to its
    /// request: this client's own when `subject` is this client, otherwise
    /// the copy it keeps.
    pub fn send_mesh_config(
        &self,
        to: DeviceUid,
        subject: DeviceUid,
        settings: &[MeshSetting],
    ) -> Result<(), ControlError> {
        let encoded = encoded(settings)?;
        self.shared.commands(move |state| {
            let d = device_of(state, to)?;
            let flags = if subject == state.uid {
                0
            } else {
                MESH_CONFIG_FLAG_COPY
            };
            let addressee = Addressee::direct(d, d.heard_path(Instant::now()));
            Ok(
                build_mesh_config_pages(config_kind::DUMP, flags, subject, &encoded)
                    .into_iter()
                    .map(|p| Command::new(addressee.clone(), op::MESH_CONFIG, p))
                    .collect(),
            )
        })
    }

    /// Has a member of the mesh this client controls take `settings` as its
    /// kept settings: it sets every one named, resets every other kept one
    /// to its default, and answers with the settings it then has. A member
    /// whose settings changed restarts 3 s later.
    ///
    /// Refused unless the member has reported this client as its controller,
    /// the one sender a unit takes them from.
    pub fn send_mesh_config_set(
        &self,
        member: DeviceUid,
        settings: &[MeshSetting],
    ) -> Result<(), ControlError> {
        let encoded = encoded(settings)?;
        self.shared.commands(move |state| {
            let d = device_of(state, member)?;
            match d.reported_mesh_master() {
                None => return Err(ControlError::NotReported("the member's controller")),
                Some(master) if master != state.uid => {
                    return Err(ControlError::InvalidRequest(
                        "a unit takes kept settings only from its controller",
                    ))
                }
                Some(_) => {}
            }
            let addressee = Addressee::direct(d, d.heard_path(Instant::now()));
            Ok(
                build_mesh_config_pages(config_kind::SET, 0, member, &encoded)
                    .into_iter()
                    .map(|p| Command::new(addressee.clone(), op::MESH_CONFIG, p))
                    .collect(),
            )
        })
    }
}
