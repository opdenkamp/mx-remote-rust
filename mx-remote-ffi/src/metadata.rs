// Author: Lars Op den Kamp (lars@opdenkamp-it.nl)
// Copyright (c) 2026 Op den Kamp IT Solutions

//! The port metadata a mesh keeps: fields an installer recorded about the
//! device on each port, which every unit holds.
//!
//! A change to what these read is announced through `on_device_update` of
//! the device that sent it, or of this client when it set it itself.

use std::ffi::c_char;

use mx_remote::{
    MeshPort, PortMetadata, PortMetadataField, PortMetadataStore, PORT_METADATA_FIELDS_MAX,
    PORT_METADATA_KEY_MAX, PORT_METADATA_MAX, PORT_METADATA_VALUE_MAX,
};

use crate::abi::{fail, from_control, guard, mxr_result_t, mxr_uid_t, put_str};
use crate::info::copy_into;
use crate::remote::{mxr_remote_t, with};

/// The most fields a port's metadata holds.
pub const MXR_PORT_METADATA_FIELDS_MAX: usize = 16;
/// The most bytes a port's metadata takes as it is carried: each field its
/// key, a NUL, its value and a NUL.
pub const MXR_PORT_METADATA_MAX: usize = 1024;
/// Bytes in a key of `mxr_port_metadata_field_t`, its NUL included.
pub const MXR_PORT_METADATA_KEY_LEN: usize = 32;
/// Bytes in a value of `mxr_port_metadata_field_t`, its NUL included.
pub const MXR_PORT_METADATA_VALUE_LEN: usize = 256;

const _: () = assert!(MXR_PORT_METADATA_FIELDS_MAX == PORT_METADATA_FIELDS_MAX);
const _: () = assert!(MXR_PORT_METADATA_MAX == PORT_METADATA_MAX);
const _: () = assert!(MXR_PORT_METADATA_KEY_LEN == PORT_METADATA_KEY_MAX + 1);
const _: () = assert!(MXR_PORT_METADATA_VALUE_LEN == PORT_METADATA_VALUE_MAX + 1);

/// One field of a port's metadata, such as `device_type` set to
/// `Sony PlayStation 4`.
///
/// A key is 1 to 31 characters of `A-Z`, `a-z`, `0-9`, `_` and `-`. A value
/// is 1 to 255 bytes of UTF-8, with no control character, `"` or `\`.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct mxr_port_metadata_field_t {
    /// The key, NUL-terminated.
    pub key: [c_char; MXR_PORT_METADATA_KEY_LEN],
    /// The value, NUL-terminated.
    pub value: [c_char; MXR_PORT_METADATA_VALUE_LEN],
}

impl From<&PortMetadataField> for mxr_port_metadata_field_t {
    fn from(f: &PortMetadataField) -> Self {
        let mut out = Self {
            key: [0; MXR_PORT_METADATA_KEY_LEN],
            value: [0; MXR_PORT_METADATA_VALUE_LEN],
        };
        put_str(&mut out.key, &f.key);
        put_str(&mut out.value, &f.value);
        out
    }
}

/// One field of the port metadata this client holds, with the port it is
/// of: a unit's input or its output. A port's fields are consecutive rows,
/// in order.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct mxr_port_metadata_row_t {
    /// The unit.
    pub unit: mxr_uid_t,
    /// The unit's input, or else its output.
    pub input: bool,
    /// The field.
    pub field: mxr_port_metadata_field_t,
}

/// A NUL-terminated field, as UTF-8.
fn text(field: &[c_char]) -> Result<String, mxr_result_t> {
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

/// `count` items at `items`, an empty slice for none.
///
/// # Safety
///
/// `items` is null or points at `count` readable items.
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

fn field_of(f: &mxr_port_metadata_field_t) -> Result<PortMetadataField, mxr_result_t> {
    Ok(PortMetadataField::new(text(&f.key)?, text(&f.value)?))
}

fn port(unit: mxr_uid_t, input: bool) -> MeshPort {
    MeshPort {
        unit: unit.into(),
        input,
    }
}

/// Writes the metadata of a port, `unit`'s input or its output, as this
/// client holds it, and returns how many fields it has: 0 when it holds
/// none.
///
/// # Safety
///
/// `remote` is null or a live handle, and `out` is null or points at `cap`
/// writable [`mxr_port_metadata_field_t`].
#[no_mangle]
pub unsafe extern "C" fn mxr_port_metadata(
    remote: *const mxr_remote_t,
    unit: mxr_uid_t,
    input: bool,
    out: *mut mxr_port_metadata_field_t,
    cap: usize,
) -> usize {
    guard(0, || {
        // SAFETY: the caller guarantees a live handle or null.
        let Some(r) = (unsafe { remote.as_ref() }) else {
            return crate::subsystems::no_handle();
        };
        let fields: Vec<mxr_port_metadata_field_t> = r
            .remote
            .port_metadata(port(unit, input))
            .unwrap_or_default()
            .iter()
            .map(Into::into)
            .collect();
        // SAFETY: the caller guarantees cap writable elements at out.
        unsafe { copy_into(&fields, out, cap) }
    })
}

/// Sets the metadata of a port, `unit`'s input or its output, as the API of
/// the unit serving it does: `fields` in the order given, none removing the
/// port's metadata. When that changes what this client holds, the
/// generation goes up by one; as controller this client also sends the
/// port's metadata to every unit at once and its index 1 s later, and its
/// members take them. A unit in no mesh keeps it alone.
///
/// `MXR_ERR_INVALID_ARGUMENT` while this client follows another unit as its
/// controller, whose API it is, and for metadata no unit takes: more than
/// `MXR_PORT_METADATA_FIELDS_MAX` fields or `MXR_PORT_METADATA_MAX` bytes,
/// an invalid key or value, or a key set twice.
///
/// # Safety
///
/// `remote` is null or a live handle from `mxr_remote_new()`, and `fields`
/// is null or points at `count` fields.
#[no_mangle]
pub unsafe extern "C" fn mxr_set_port_metadata(
    remote: *const mxr_remote_t,
    unit: mxr_uid_t,
    input: bool,
    fields: *const mxr_port_metadata_field_t,
    count: usize,
) -> mxr_result_t {
    // SAFETY: the caller guarantees a live handle or null.
    let handle = unsafe { remote.as_ref() };
    with(handle, |r| {
        // SAFETY: the caller guarantees count fields at fields.
        let fields = match unsafe { items(fields, count, "fields") }
            .and_then(|f| f.iter().map(field_of).collect::<Result<Vec<_>, _>>())
        {
            Ok(fields) => fields,
            Err(code) => return code,
        };
        from_control(r.remote.set_port_metadata(port(unit, input), fields))
    })
}

/// Asks `holder` for the metadata of a port, `unit`'s input or its output.
/// Every unit answers with what it holds, and the answer is taken from
/// `holder` for 30 s; `mxr_port_metadata()` then reads it.
///
/// # Safety
///
/// `remote` is null or a live handle from `mxr_remote_new()`.
#[no_mangle]
pub unsafe extern "C" fn mxr_request_port_metadata(
    remote: *const mxr_remote_t,
    holder: mxr_uid_t,
    unit: mxr_uid_t,
    input: bool,
) -> mxr_result_t {
    // SAFETY: the caller guarantees a live handle or null.
    let handle = unsafe { remote.as_ref() };
    with(handle, |r| {
        from_control(
            r.remote
                .request_port_metadata(holder.into(), port(unit, input)),
        )
    })
}

/// The generation of the port metadata this client holds, 0 for a null
/// handle: a controller raises it by one on every change, and a unit takes
/// it from the index it last took whole. What a controller persists, with
/// `mxr_port_metadata_rows()`.
///
/// # Safety
///
/// `remote` is null or a live handle from `mxr_remote_new()`.
#[no_mangle]
pub unsafe extern "C" fn mxr_port_metadata_generation(remote: *const mxr_remote_t) -> u32 {
    guard(0, || {
        // SAFETY: the caller guarantees a live handle or null.
        let Some(r) = (unsafe { remote.as_ref() }) else {
            crate::subsystems::no_handle();
            return 0;
        };
        r.remote.port_metadata_store().generation
    })
}

/// Writes every field of the port metadata this client holds, port by port,
/// and returns how many there are.
///
/// # Safety
///
/// `remote` is null or a live handle, and `out` is null or points at `cap`
/// writable [`mxr_port_metadata_row_t`].
#[no_mangle]
pub unsafe extern "C" fn mxr_port_metadata_rows(
    remote: *const mxr_remote_t,
    out: *mut mxr_port_metadata_row_t,
    cap: usize,
) -> usize {
    guard(0, || {
        // SAFETY: the caller guarantees a live handle or null.
        let Some(r) = (unsafe { remote.as_ref() }) else {
            return crate::subsystems::no_handle();
        };
        let rows: Vec<mxr_port_metadata_row_t> = r
            .remote
            .port_metadata_store()
            .ports
            .iter()
            .flat_map(|p| {
                p.fields.iter().map(|f| mxr_port_metadata_row_t {
                    unit: p.port.unit.into(),
                    input: p.port.input,
                    field: f.into(),
                })
            })
            .collect();
        // SAFETY: the caller guarantees cap writable elements at out.
        unsafe { copy_into(&rows, out, cap) }
    })
}

/// Replaces all the port metadata this client holds with `rows` at
/// `generation`, as a unit loads what it persisted; best before
/// `mxr_remote_start()`. Consecutive rows of one port are its fields, in
/// order. Nothing is reported or sent for it, but as controller this client
/// sends its index within 1 s.
///
/// `MXR_ERR_INVALID_ARGUMENT`, holding what it held, for a port whose rows
/// are not consecutive, or for a field `mxr_set_port_metadata()` refuses
/// other than a key set twice.
///
/// # Safety
///
/// `remote` is null or a live handle from `mxr_remote_new()`, and `rows` is
/// null or points at `count` rows.
#[no_mangle]
pub unsafe extern "C" fn mxr_restore_port_metadata(
    remote: *const mxr_remote_t,
    generation: u32,
    rows: *const mxr_port_metadata_row_t,
    count: usize,
) -> mxr_result_t {
    // SAFETY: the caller guarantees a live handle or null.
    let handle = unsafe { remote.as_ref() };
    with(handle, |r| {
        // SAFETY: the caller guarantees count rows at rows.
        let rows = match unsafe { items(rows, count, "rows") } {
            Ok(rows) => rows,
            Err(code) => return code,
        };
        let mut ports: Vec<PortMetadata> = Vec::new();
        for row in rows {
            let field = match field_of(&row.field) {
                Ok(field) => field,
                Err(code) => return code,
            };
            let at = port(row.unit, row.input);
            match ports.last_mut() {
                Some(last) if last.port == at => last.fields.push(field),
                _ => ports.push(PortMetadata {
                    port: at,
                    fields: vec![field],
                }),
            }
        }
        from_control(
            r.remote
                .restore_port_metadata(&PortMetadataStore { generation, ports }),
        )
    })
}
