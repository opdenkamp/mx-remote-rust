// Author: Lars Op den Kamp (lars@opdenkamp-it.nl)
// Copyright (c) 2026 Op den Kamp IT Solutions

//! Replacing a unit of a mesh, and the settings a mesh keeps of its units.
//!
//! A change to what these read is announced through `on_device_update` of
//! the device that sent it.

use std::ffi::c_char;

use mx_remote::{
    ControllerHello, ControllerRole, DeviceFeature, MeshBay, MeshReplaceResult, MeshReplaceUnit,
    MeshReplaceable, MeshSetting, CONTROLLER_PARTS_MAX, MESH_BAYS_MAX, MESH_CONFIG_MAX,
    MESH_REPLACE_NAME_MAX, MESH_REPLACE_UNITS_MAX,
};

use crate::abi::{fail, from_control, guard, mxr_result_t, mxr_uid_t, put_str};
use crate::info::{copy_into, null_out};
use crate::remote::{mxr_remote_t, with};

/// The most units a controller lists as replaceable.
pub const MXR_MESH_REPLACE_UNITS_MAX: usize = 32;
/// Bytes in a bay name of `mxr_mesh_replace_unit_t`, its NUL included.
pub const MXR_MESH_REPLACE_NAME_LEN: usize = 17;
/// Bytes in a name or value of `mxr_mesh_setting_t`, its NUL included.
pub const MXR_MESH_SETTING_LEN: usize = 256;
/// The most bytes a unit's kept settings take as they are carried: each
/// setting a byte of name length, the name, a byte of value length and the
/// value.
pub const MXR_MESH_CONFIG_MAX: usize = 1984;

const _: () = assert!(MXR_MESH_REPLACE_UNITS_MAX == MESH_REPLACE_UNITS_MAX);
const _: () = assert!(MXR_MESH_REPLACE_NAME_LEN == MESH_REPLACE_NAME_MAX + 1);
const _: () = assert!(MXR_MESH_SETTING_LEN == u8::MAX as usize + 1);
const _: () = assert!(MXR_MESH_CONFIG_MAX == MESH_CONFIG_MAX);

/// A unit a controller can have replaced: it has bays on the controller, it
/// is not the controller, and it is offline.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct mxr_mesh_replace_unit_t {
    /// The unit.
    pub uid: mxr_uid_t,
    /// Its `MXR_FEATURE_*` OneIP source, OneIP sink and multiviewer bits,
    /// which a unit replacing it must have too.
    pub features: u32,
    /// Its first input bay on the controller, 0 when it has none.
    pub input: u16,
    /// Its first output bay on the controller, 0 when it has none.
    pub output: u16,
    /// The name of that input.
    pub input_name: [c_char; MXR_MESH_REPLACE_NAME_LEN],
    /// The name of that output.
    pub output_name: [c_char; MXR_MESH_REPLACE_NAME_LEN],
}

impl From<&MeshReplaceUnit> for mxr_mesh_replace_unit_t {
    fn from(u: &MeshReplaceUnit) -> Self {
        let mut out = Self {
            uid: u.uid.into(),
            features: u.features.bits(),
            input: u.input,
            output: u.output,
            input_name: [0; MXR_MESH_REPLACE_NAME_LEN],
            output_name: [0; MXR_MESH_REPLACE_NAME_LEN],
        };
        put_str(&mut out.input_name, &u.input_name);
        put_str(&mut out.output_name, &u.output_name);
        out
    }
}

/// A unit this client can ask to replace, and who to ask.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct mxr_mesh_replaceable_t {
    /// The device that listed it, which `mxr_request_mesh_replace()` asks.
    pub lister: mxr_uid_t,
    /// The unit is the lister's own controller, which is offline.
    pub offline_controller: bool,
    /// The unit.
    pub unit: mxr_mesh_replace_unit_t,
}

impl From<&MeshReplaceable> for mxr_mesh_replaceable_t {
    fn from(r: &MeshReplaceable) -> Self {
        Self {
            lister: r.lister.into(),
            offline_controller: r.offline_controller,
            unit: (&r.unit).into(),
        }
    }
}

/// One setting a unit keeps for its mesh: an application configuration name,
/// such as `led.status`, and its value as text.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct mxr_mesh_setting_t {
    /// The name, NUL-terminated.
    pub name: [c_char; MXR_MESH_SETTING_LEN],
    /// The value, NUL-terminated.
    pub value: [c_char; MXR_MESH_SETTING_LEN],
}

impl From<&MeshSetting> for mxr_mesh_setting_t {
    fn from(s: &MeshSetting) -> Self {
        let mut out = Self {
            name: [0; MXR_MESH_SETTING_LEN],
            value: [0; MXR_MESH_SETTING_LEN],
        };
        put_str(&mut out.name, &s.name);
        put_str(&mut out.value, &s.value);
        out
    }
}

/// The length and crc32 of the settings a unit keeps for its mesh, as it
/// announces them.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct mxr_mesh_config_digest_t {
    /// The settings' length in bytes, as they are carried.
    pub length: u16,
    /// The crc32 of those bytes.
    pub crc32: u32,
    /// The unit lost its settings and announces defaults.
    pub lost: bool,
}

/// Who sent a unit's kept settings; `mxr_mesh_config_settings()` reads the
/// settings themselves.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct mxr_mesh_config_t {
    /// The device that sent them.
    pub holder: mxr_uid_t,
    /// They are a copy `holder` keeps of the unit, not its own.
    pub copy: bool,
    /// The unit lost its settings, and these are its defaults.
    pub lost: bool,
    /// How many settings there are.
    pub count: usize,
}

/// Reads a fixed-width field up to its first NUL, or whole without one.
fn field_str(field: &[c_char]) -> Result<String, mxr_result_t> {
    let bytes: Vec<u8> = field
        .iter()
        .map(|&c| c as u8)
        .take_while(|&b| b != 0)
        .collect();
    String::from_utf8(bytes).map_err(|_| {
        fail(
            mxr_result_t::MXR_ERR_INVALID_ARGUMENT,
            "string is not valid UTF-8",
        )
    })
}

/// Borrows a caller's array of `count` elements, which may be null when
/// `count` is zero.
///
/// # Safety
///
/// `items` is null or points at `count` initialised elements.
unsafe fn items<'a, T>(items: *const T, count: usize, what: &str) -> Result<&'a [T], mxr_result_t> {
    match (items.is_null(), count) {
        (_, 0) => Ok(&[]),
        (true, _) => Err(fail(
            mxr_result_t::MXR_ERR_INVALID_ARGUMENT,
            &format!("{what} is null with a count above zero"),
        )),
        // SAFETY: the caller guarantees count elements at items.
        (false, _) => Ok(unsafe { std::slice::from_raw_parts(items, count) }),
    }
}

/// Reads a caller's settings.
///
/// # Safety
///
/// `settings` is null or points at `count` settings.
unsafe fn settings_of(
    settings: *const mxr_mesh_setting_t,
    count: usize,
) -> Result<Vec<MeshSetting>, mxr_result_t> {
    // SAFETY: the caller guarantees count settings at settings.
    unsafe { items(settings, count, "settings") }?
        .iter()
        .map(|s| Ok(MeshSetting::new(field_str(&s.name)?, field_str(&s.value)?)))
        .collect()
}

// ---- replacing a unit of a mesh ----

/// Asks every device which units this client can replace.
///
/// Every controller answers with its offline units, and every member whose
/// controller is offline with that controller; `mxr_mesh_replaceable()` then
/// reads them. A unit lists none until its controller has been up 120 s, and
/// a device without the mesh module answers nothing. A call within 2 s of
/// the last sends nothing, as the answers to that one are still arriving.
///
/// # Safety
///
/// `remote` is null or a live handle from `mxr_remote_new()`.
#[no_mangle]
pub unsafe extern "C" fn mxr_query_mesh_replaceable(remote: *const mxr_remote_t) -> mxr_result_t {
    // SAFETY: the caller guarantees a live handle or null.
    let handle = unsafe { remote.as_ref() };
    with(handle, |r| from_control(r.remote.query_mesh_replaceable()))
}

/// Writes the units this client can ask to replace, as listed in the last
/// 30 s, and returns how many there are. A unit several members list appears
/// once.
///
/// # Safety
///
/// `remote` is null or a live handle, and `out` is null or points at `cap`
/// writable [`mxr_mesh_replaceable_t`].
#[no_mangle]
pub unsafe extern "C" fn mxr_mesh_replaceable(
    remote: *const mxr_remote_t,
    out: *mut mxr_mesh_replaceable_t,
    cap: usize,
) -> usize {
    guard(0, || {
        // SAFETY: the caller guarantees a live handle or null.
        let Some(r) = (unsafe { remote.as_ref() }) else {
            return crate::subsystems::no_handle();
        };
        let found: Vec<mxr_mesh_replaceable_t> =
            r.remote.mesh_replaceable().iter().map(Into::into).collect();
        // SAFETY: the caller guarantees cap writable elements at out.
        unsafe { copy_into(&found, out, cap) }
    })
}

/// Asks `lister` to replace `unit` with this client: the device takes over
/// the unit's bays, their names and routes, and its kept settings. The answer
/// is not reported through C.
///
/// Asking a member to replace its offline controller makes it send the
/// controller's kept settings, which `mxr_mesh_config()` then reads. Taking
/// over is then this client's own: promoting itself to controller in place
/// of the old one, and having the members replace it
/// (`mxr_send_mesh_operation()`).
///
/// `MXR_ERR_NOT_REPORTED` when `lister` has not listed `unit` in the last
/// 30 s, `MXR_ERR_INVALID_ARGUMENT` while this client follows a mesh
/// controller or is on a default serial, and `MXR_ERR_UNSUPPORTED` when it
/// lacks a OneIP source, OneIP sink or multiviewer feature `unit` has.
///
/// # Safety
///
/// `remote` is null or a live handle from `mxr_remote_new()`.
#[no_mangle]
pub unsafe extern "C" fn mxr_request_mesh_replace(
    remote: *const mxr_remote_t,
    lister: mxr_uid_t,
    unit: mxr_uid_t,
) -> mxr_result_t {
    // SAFETY: the caller guarantees a live handle or null.
    let handle = unsafe { remote.as_ref() };
    with(handle, |r| {
        from_control(r.remote.request_mesh_replace(lister.into(), unit.into()))
    })
}

/// Answers a unit's query with the units this client, as controller, can have
/// replaced, or with its offline controller, as a member. An empty list is
/// sent all the same.
///
/// `MXR_ERR_INVALID_ARGUMENT` for more than `MXR_MESH_REPLACE_UNITS_MAX`
/// units, or a name without a NUL in its field.
///
/// # Safety
///
/// `remote` is null or a live handle from `mxr_remote_new()`, and `units` is
/// null or points at `count` units.
#[no_mangle]
pub unsafe extern "C" fn mxr_send_mesh_replaceable(
    remote: *const mxr_remote_t,
    to: mxr_uid_t,
    offline_controller: bool,
    units: *const mxr_mesh_replace_unit_t,
    count: usize,
) -> mxr_result_t {
    // SAFETY: the caller guarantees a live handle or null.
    let handle = unsafe { remote.as_ref() };
    with(handle, |r| {
        // SAFETY: the caller guarantees count units at units.
        let units = match unsafe { items(units, count, "units") } {
            Ok(units) => units,
            Err(code) => return code,
        };
        let units: Result<Vec<MeshReplaceUnit>, mxr_result_t> = units
            .iter()
            .map(|u| {
                Ok(MeshReplaceUnit {
                    uid: u.uid.into(),
                    features: DeviceFeature::from_bits(u.features),
                    input: u.input,
                    output: u.output,
                    input_name: field_str(&u.input_name)?,
                    output_name: field_str(&u.output_name)?,
                })
            })
            .collect();
        match units {
            Ok(units) => from_control(r.remote.send_mesh_replaceable(
                to.into(),
                offline_controller,
                &units,
            )),
            Err(code) => code,
        }
    })
}

/// Answers a unit's request to replace `unit` with it; `result` is
/// `MXR_MESH_REPLACE_*`.
///
/// # Safety
///
/// `remote` is null or a live handle from `mxr_remote_new()`.
#[no_mangle]
pub unsafe extern "C" fn mxr_send_mesh_replace_result(
    remote: *const mxr_remote_t,
    to: mxr_uid_t,
    unit: mxr_uid_t,
    result: u8,
) -> mxr_result_t {
    // SAFETY: the caller guarantees a live handle or null.
    let handle = unsafe { remote.as_ref() };
    with(handle, |r| {
        from_control(r.remote.send_mesh_replace_result(
            to.into(),
            unit.into(),
            MeshReplaceResult::from_wire(result),
        ))
    })
}

// ---- the settings a mesh keeps of its units ----

/// Fills `out` with the length and crc32 of a unit's kept settings, as it
/// last announced them. `MXR_ERR_NOT_REPORTED` until it has.
///
/// # Safety
///
/// `remote` is null or a live handle, and `out` points at a writable
/// `mxr_mesh_config_digest_t`.
#[no_mangle]
pub unsafe extern "C" fn mxr_mesh_config_digest(
    remote: *const mxr_remote_t,
    uid: mxr_uid_t,
    out: *mut mxr_mesh_config_digest_t,
) -> mxr_result_t {
    // SAFETY: the caller guarantees a live handle or null.
    let handle = unsafe { remote.as_ref() };
    with(handle, |r| {
        let value = r
            .remote
            .mesh_config_digest(uid.into())
            .map(|d| mxr_mesh_config_digest_t {
                length: d.length,
                crc32: d.crc32,
                lost: d.lost,
            });
        // SAFETY: the caller guarantees a writable struct or null.
        unsafe { crate::subsystems::fill(r, uid, out, "kept settings digest", value) }
    })
}

/// Fills `out` with who last sent `subject`'s kept settings whole, and how
/// many there are. `MXR_ERR_NOT_REPORTED` until a device has; ask for them
/// with `mxr_request_mesh_config()`.
///
/// # Safety
///
/// `remote` is null or a live handle, and `out` points at a writable
/// `mxr_mesh_config_t`.
#[no_mangle]
pub unsafe extern "C" fn mxr_mesh_config(
    remote: *const mxr_remote_t,
    subject: mxr_uid_t,
    out: *mut mxr_mesh_config_t,
) -> mxr_result_t {
    // SAFETY: the caller guarantees a live handle or null.
    let handle = unsafe { remote.as_ref() };
    with(handle, |r| {
        if out.is_null() {
            return null_out("kept settings");
        }
        match r.remote.mesh_config(subject.into()) {
            Some(c) => {
                // SAFETY: the caller guarantees a writable struct, and it is
                // not null.
                unsafe {
                    *out = mxr_mesh_config_t {
                        holder: c.holder.into(),
                        copy: c.copy,
                        lost: c.lost,
                        count: c.settings.len(),
                    }
                };
                mxr_result_t::MXR_OK
            }
            None => fail(
                mxr_result_t::MXR_ERR_NOT_REPORTED,
                "no device has sent the unit's kept settings",
            ),
        }
    })
}

/// Writes `subject`'s kept settings, as a device last sent them whole, and
/// returns how many there are: none until one has.
///
/// # Safety
///
/// `remote` is null or a live handle, and `out` is null or points at `cap`
/// writable [`mxr_mesh_setting_t`].
#[no_mangle]
pub unsafe extern "C" fn mxr_mesh_config_settings(
    remote: *const mxr_remote_t,
    subject: mxr_uid_t,
    out: *mut mxr_mesh_setting_t,
    cap: usize,
) -> usize {
    guard(0, || {
        // SAFETY: the caller guarantees a live handle or null.
        let Some(r) = (unsafe { remote.as_ref() }) else {
            return crate::subsystems::no_handle();
        };
        let settings: Vec<mxr_mesh_setting_t> = r
            .remote
            .mesh_config(subject.into())
            .map(|c| c.settings.iter().map(Into::into).collect())
            .unwrap_or_default();
        // SAFETY: the caller guarantees cap writable elements at out.
        unsafe { copy_into(&settings, out, cap) }
    })
}

/// Asks `holder` for the kept settings of `subject`: its own when `subject`
/// is `holder`, otherwise the copy it keeps. A controller keeps one of every
/// member, and a member one of its controller. `mxr_mesh_config()` reads them
/// once they arrive; a device keeping no copy answers nothing.
///
/// # Safety
///
/// `remote` is null or a live handle from `mxr_remote_new()`.
#[no_mangle]
pub unsafe extern "C" fn mxr_request_mesh_config(
    remote: *const mxr_remote_t,
    holder: mxr_uid_t,
    subject: mxr_uid_t,
) -> mxr_result_t {
    // SAFETY: the caller guarantees a live handle or null.
    let handle = unsafe { remote.as_ref() };
    with(handle, |r| {
        from_control(r.remote.request_mesh_config(holder.into(), subject.into()))
    })
}

/// Announces the length and crc32 of this client's own kept settings to every
/// device, as a unit in a mesh does every 60 s.
///
/// `MXR_ERR_INVALID_ARGUMENT` while this client follows no mesh controller,
/// and for settings no unit takes: a name or value of more than 255 bytes, or
/// more than `MXR_MESH_CONFIG_MAX` bytes together.
///
/// # Safety
///
/// `remote` is null or a live handle from `mxr_remote_new()`, and `settings`
/// is null or points at `count` settings.
#[no_mangle]
pub unsafe extern "C" fn mxr_send_mesh_config_hello(
    remote: *const mxr_remote_t,
    settings: *const mxr_mesh_setting_t,
    count: usize,
) -> mxr_result_t {
    // SAFETY: the caller guarantees a live handle or null.
    let handle = unsafe { remote.as_ref() };
    with(handle, |r| {
        // SAFETY: the caller guarantees count settings at settings.
        match unsafe { settings_of(settings, count) } {
            Ok(settings) => from_control(r.remote.send_mesh_config_hello(&settings)),
            Err(code) => code,
        }
    })
}

/// Sends `to` the kept settings of `subject`, as the answer to its request:
/// this client's own when `subject` is this client, otherwise the copy it
/// keeps.
///
/// `MXR_ERR_INVALID_ARGUMENT` for settings no unit takes, as
/// `mxr_send_mesh_config_hello()`.
///
/// # Safety
///
/// `remote` is null or a live handle from `mxr_remote_new()`, and `settings`
/// is null or points at `count` settings.
#[no_mangle]
pub unsafe extern "C" fn mxr_send_mesh_config(
    remote: *const mxr_remote_t,
    to: mxr_uid_t,
    subject: mxr_uid_t,
    settings: *const mxr_mesh_setting_t,
    count: usize,
) -> mxr_result_t {
    // SAFETY: the caller guarantees a live handle or null.
    let handle = unsafe { remote.as_ref() };
    with(handle, |r| {
        // SAFETY: the caller guarantees count settings at settings.
        match unsafe { settings_of(settings, count) } {
            Ok(settings) => from_control(r.remote.send_mesh_config(
                to.into(),
                subject.into(),
                &settings,
            )),
            Err(code) => code,
        }
    })
}

/// Has a member of the mesh this client controls take `settings` as its kept
/// settings: it sets every one named, resets every other kept one to its
/// default, and answers with the settings it then has. A member whose
/// settings changed restarts 3 s later.
///
/// `MXR_ERR_NOT_REPORTED` until the member has reported its controller,
/// `MXR_ERR_INVALID_ARGUMENT` when that is not this client, and for settings
/// no unit takes, as `mxr_send_mesh_config_hello()`.
///
/// # Safety
///
/// `remote` is null or a live handle from `mxr_remote_new()`, and `settings`
/// is null or points at `count` settings.
#[no_mangle]
pub unsafe extern "C" fn mxr_send_mesh_config_set(
    remote: *const mxr_remote_t,
    member: mxr_uid_t,
    settings: *const mxr_mesh_setting_t,
    count: usize,
) -> mxr_result_t {
    // SAFETY: the caller guarantees a live handle or null.
    let handle = unsafe { remote.as_ref() };
    with(handle, |r| {
        // SAFETY: the caller guarantees count settings at settings.
        match unsafe { settings_of(settings, count) } {
            Ok(settings) => from_control(r.remote.send_mesh_config_set(member.into(), &settings)),
            Err(code) => code,
        }
    })
}

/// The most bays a controller's numbering holds: up to 256 of each mode.
pub const MXR_MESH_BAYS_MAX: usize = 512;

const _: () = assert!(MXR_MESH_BAYS_MAX == MESH_BAYS_MAX);

/// One bay of a controller's numbering: the unit that owns it.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct mxr_mesh_bay_t {
    /// The unit that owns the bay.
    pub unit: mxr_uid_t,
    /// The unit's `MXR_FEATURE_*` OneIP source, OneIP sink and multiviewer
    /// bits as the sender knows them, 0 when it doesn't.
    pub features: u32,
    /// The bay number the controller gives it; 0 is the controller's own.
    pub bay: u16,
    /// An input, or else an output.
    pub input: bool,
}

impl From<&MeshBay> for mxr_mesh_bay_t {
    fn from(b: &MeshBay) -> Self {
        Self {
            unit: b.unit.into(),
            features: b.features.bits(),
            bay: b.bay,
            input: b.input,
        }
    }
}

/// Marks the setup of mesh member `member` alone completed, or not, as its
/// controller does: `mxr_set_mesh_setup_completed()` marks every unit that
/// hears it, as its frame names no unit.
///
/// A member takes it only from its own controller, and only one whose mesh
/// module knows it; any other unit drops it unread. Nothing reports it taken
/// except the member's next hello, which carries
/// `MXR_FEATURE_SETUP_COMPLETED` as marked.
///
/// A controller marking its mesh's setup completed marks the members of that
/// moment, and sends this to one that missed it: offline at the time, or
/// still booting without its controller known or its module loaded, so it is
/// sent again until the hello shows it. A replacement takes the mark of the
/// unit it replaced. A member added later is marked only with its own setup.
/// Which members carry a mark is the caller's to keep.
///
/// # Safety
///
/// `remote` is null or a live handle from `mxr_remote_new()`.
#[no_mangle]
pub unsafe extern "C" fn mxr_set_member_setup_completed(
    remote: *const mxr_remote_t,
    member: mxr_uid_t,
    completed: bool,
) -> mxr_result_t {
    // SAFETY: the caller guarantees a live handle or null.
    let handle = unsafe { remote.as_ref() };
    with(handle, |r| {
        from_control(
            r.remote
                .set_member_setup_completed(member.into(), completed),
        )
    })
}

/// Asks `holder` for the numbering of controller `subject`: which unit owns
/// each of its bays. A controller answers for itself; a member with the copy
/// it keeps of its controller's, also once that controller is offline.
/// `mxr_mesh_bays()` reads it once it arrives; a device keeping none answers
/// nothing.
///
/// # Safety
///
/// `remote` is null or a live handle from `mxr_remote_new()`.
#[no_mangle]
pub unsafe extern "C" fn mxr_request_mesh_bays(
    remote: *const mxr_remote_t,
    holder: mxr_uid_t,
    subject: mxr_uid_t,
) -> mxr_result_t {
    // SAFETY: the caller guarantees a live handle or null.
    let handle = unsafe { remote.as_ref() };
    with(handle, |r| {
        from_control(r.remote.request_mesh_bays(holder.into(), subject.into()))
    })
}

/// Writes controller `subject`'s numbering, as a device last sent it whole,
/// and returns how many bays it holds: none until one has.
///
/// # Safety
///
/// `remote` is null or a live handle, and `out` is null or points at `cap`
/// writable [`mxr_mesh_bay_t`].
#[no_mangle]
pub unsafe extern "C" fn mxr_mesh_bays(
    remote: *const mxr_remote_t,
    subject: mxr_uid_t,
    out: *mut mxr_mesh_bay_t,
    cap: usize,
) -> usize {
    guard(0, || {
        // SAFETY: the caller guarantees a live handle or null.
        let Some(r) = (unsafe { remote.as_ref() }) else {
            return crate::subsystems::no_handle();
        };
        let bays: Vec<mxr_mesh_bay_t> = r
            .remote
            .mesh_bays(subject.into())
            .map(|b| b.bays.iter().map(Into::into).collect())
            .unwrap_or_default();
        // SAFETY: the caller guarantees cap writable elements at out.
        unsafe { copy_into(&bays, out, cap) }
    })
}

/// Sends `to` the numbering of controller `subject`: this client's own when
/// `subject` is this client, otherwise the copy it keeps.
/// `MXR_ERR_INVALID_ARGUMENT` for more than `MXR_MESH_BAYS_MAX` bays.
///
/// # Safety
///
/// `remote` is null or a live handle from `mxr_remote_new()`, and `bays` is
/// null or points at `count` bays.
#[no_mangle]
pub unsafe extern "C" fn mxr_send_mesh_bays(
    remote: *const mxr_remote_t,
    to: mxr_uid_t,
    subject: mxr_uid_t,
    bays: *const mxr_mesh_bay_t,
    count: usize,
) -> mxr_result_t {
    // SAFETY: the caller guarantees a live handle or null.
    let handle = unsafe { remote.as_ref() };
    with(handle, |r| {
        // SAFETY: the caller guarantees count bays at bays.
        let bays = match unsafe { items(bays, count, "bays") } {
            Ok(bays) => bays,
            Err(code) => return code,
        };
        let bays: Vec<MeshBay> = bays
            .iter()
            .map(|b| MeshBay {
                input: b.input,
                bay: b.bay,
                features: DeviceFeature::from_bits(b.features),
                unit: b.unit.into(),
            })
            .collect();
        from_control(r.remote.send_mesh_bays(to.into(), subject.into(), &bays))
    })
}

/// The most parts a controller hello lists: the state, and the four overlay
/// logo slots.
pub const MXR_CONTROLLER_PARTS_MAX: usize = 5;

const _: () = assert!(MXR_CONTROLLER_PARTS_MAX == CONTROLLER_PARTS_MAX);

/// One part of a dedicated controller's state, as its hello lists it.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct mxr_controller_part_t {
    /// The part's length in bytes.
    pub length: u32,
    /// The crc32 of the whole part.
    pub crc: u32,
    /// Which part: 0 the state, 1-4 the PNG of overlay logo slot N.
    pub part: u8,
}

/// A dedicated controller's `CONTROLLER_STATE` hello.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct mxr_controller_hello_t {
    /// The primary in the sender's state, all zero for none.
    pub primary: mxr_uid_t,
    /// Raised on every change of the sender's state.
    pub generation: u32,
    /// The parts, `nb_parts` of them.
    pub parts: [mxr_controller_part_t; MXR_CONTROLLER_PARTS_MAX],
    /// `MXR_CONTROLLER_ROLE_*`.
    pub role: u8,
    /// How many of `parts` are listed.
    pub nb_parts: u8,
}

impl From<&ControllerHello> for mxr_controller_hello_t {
    fn from(h: &ControllerHello) -> Self {
        let mut out = Self {
            primary: h.primary.into(),
            generation: h.generation,
            role: h.role.to_wire(),
            ..Self::default()
        };
        for (slot, part) in out.parts.iter_mut().zip(&h.parts) {
            *slot = mxr_controller_part_t {
                length: part.length,
                crc: part.crc,
                part: part.part,
            };
            out.nb_parts += 1;
        }
        out
    }
}

/// Fills `out` with the `CONTROLLER_STATE` hello device `uid` last sent.
/// `MXR_ERR_NOT_REPORTED` until it has sent one; a device that has is a
/// dedicated controller.
///
/// # Safety
///
/// `remote` is null or a live handle, and `out` points at a writable
/// `mxr_controller_hello_t`.
#[no_mangle]
pub unsafe extern "C" fn mxr_controller_hello(
    remote: *const mxr_remote_t,
    uid: mxr_uid_t,
    out: *mut mxr_controller_hello_t,
) -> mxr_result_t {
    // SAFETY: the caller guarantees a live handle or null.
    let handle = unsafe { remote.as_ref() };
    with(handle, |r| {
        let value = r
            .remote
            .controller_hello(uid.into())
            .map(|h| mxr_controller_hello_t::from(&h));
        // SAFETY: the caller guarantees a writable struct or null.
        unsafe { crate::subsystems::fill(r, uid, out, "controller hello", value) }
    })
}

/// Sets the role (`MXR_CONTROLLER_ROLE_*`) and the primary this client's
/// `CONTROLLER_STATE` hello carries as a dedicated controller
/// (`dedicated_controller` in its config), and sends it at once when they
/// changed. `MXR_ERR_INVALID_ARGUMENT` on a client that isn't one.
///
/// # Safety
///
/// `remote` is null or a live handle from `mxr_remote_new()`.
#[no_mangle]
pub unsafe extern "C" fn mxr_set_controller_role(
    remote: *const mxr_remote_t,
    role: u8,
    primary: mxr_uid_t,
) -> mxr_result_t {
    // SAFETY: the caller guarantees a live handle or null.
    let handle = unsafe { remote.as_ref() };
    with(handle, |r| {
        from_control(
            r.remote
                .set_controller_role(ControllerRole::from_wire(role), primary.into()),
        )
    })
}

/// One part of this client's state, as `mxr_set_controller_parts()` takes it.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct mxr_controller_part_data_t {
    /// The bytes, `len` of them; null for none.
    pub bytes: *const u8,
    /// How many bytes `bytes` points at.
    pub len: usize,
    /// Which part: 0 the state, 1-4 the PNG of overlay logo slot N.
    pub part: u8,
}

/// Sets the parts of this client's state as a dedicated controller, `count`
/// of them: its hello then lists each part's length and crc32 under
/// `generation`, and goes at once when either changed. A request from
/// another dedicated controller of this client's mesh, or the one
/// `mxr_allow_controller_state()` names, is answered from them. The library
/// keeps its own copies. `MXR_ERR_INVALID_ARGUMENT` on a client that isn't a
/// dedicated controller, for more than `MXR_CONTROLLER_PARTS_MAX` parts, or a
/// part listed twice.
///
/// # Safety
///
/// `remote` is null or a live handle from `mxr_remote_new()`. `parts` is null
/// with `count` 0, or points at `count` entries, each `bytes` null with `len`
/// 0 or pointing at `len` readable bytes.
#[no_mangle]
pub unsafe extern "C" fn mxr_set_controller_parts(
    remote: *const mxr_remote_t,
    generation: u32,
    parts: *const mxr_controller_part_data_t,
    count: usize,
) -> mxr_result_t {
    // SAFETY: the caller guarantees a live handle or null.
    let handle = unsafe { remote.as_ref() };
    with(handle, |r| {
        if count > MXR_CONTROLLER_PARTS_MAX {
            return fail(
                mxr_result_t::MXR_ERR_INVALID_ARGUMENT,
                "more parts than MXR_CONTROLLER_PARTS_MAX",
            );
        }
        if count > 0 && parts.is_null() {
            return fail(mxr_result_t::MXR_ERR_INVALID_ARGUMENT, "parts is null");
        }
        let entries = if count == 0 {
            &[][..]
        } else {
            // SAFETY: the caller guarantees `count` readable entries.
            unsafe { std::slice::from_raw_parts(parts, count) }
        };
        let mut owned = Vec::with_capacity(count);
        for entry in entries {
            let bytes = if entry.len == 0 {
                Vec::new()
            } else if entry.bytes.is_null() {
                return fail(
                    mxr_result_t::MXR_ERR_INVALID_ARGUMENT,
                    "part bytes are null",
                );
            } else {
                // SAFETY: the caller guarantees `len` readable bytes.
                unsafe { std::slice::from_raw_parts(entry.bytes, entry.len) }.to_vec()
            };
            owned.push((entry.part, bytes));
        }
        from_control(r.remote.set_controller_parts(generation, owned))
    })
}

/// Also takes `CONTROLLER_STATE` requests and data from `uid`, a dedicated
/// controller of no mesh of this client's: one this client is replacing a
/// router of its mesh with. The all-zero uid takes them only from this
/// client's mesh again.
///
/// # Safety
///
/// `remote` is null or a live handle from `mxr_remote_new()`.
#[no_mangle]
pub unsafe extern "C" fn mxr_allow_controller_state(
    remote: *const mxr_remote_t,
    uid: mxr_uid_t,
) -> mxr_result_t {
    // SAFETY: the caller guarantees a live handle or null.
    let handle = unsafe { remote.as_ref() };
    with(handle, |r| {
        let uid: mx_remote::DeviceUid = uid.into();
        r.remote
            .allow_controller_state((!uid.is_zero()).then_some(uid));
        mxr_result_t::MXR_OK
    })
}

/// Pulls `part` of device `from`'s state `generation`, which its hello lists
/// with `length` and `crc`, in place of a pull of the same part under way. It
/// arrives at `on_controller_part_received`, or the pull ends at
/// `on_controller_part_failed`. `MXR_ERR_UNKNOWN_DEVICE` for a device not
/// online.
///
/// # Safety
///
/// `remote` is null or a live handle from `mxr_remote_new()`.
#[no_mangle]
pub unsafe extern "C" fn mxr_pull_controller_part(
    remote: *const mxr_remote_t,
    from: mxr_uid_t,
    part: u8,
    generation: u32,
    length: u32,
    crc: u32,
) -> mxr_result_t {
    // SAFETY: the caller guarantees a live handle or null.
    let handle = unsafe { remote.as_ref() };
    with(handle, |r| {
        from_control(
            r.remote
                .pull_controller_part(from.into(), part, generation, length, crc),
        )
    })
}
