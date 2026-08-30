// Author: Lars Op den Kamp (lars@opdenkamp-it.nl)
// Copyright (c) 2026 Op den Kamp IT Solutions

//! The client handle: making one, running it, and asking it what it has found.

use std::ffi::c_char;
use std::ffi::c_void;
use std::net::Ipv4Addr;
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::Arc;

use mx_remote::{Config, DeviceUid, EventHandler, Remote};

use crate::abi::{
    fail, from_io, from_send, guard, mxr_bay_uid_t, mxr_result_t, mxr_uid_t, opt_str, put_str,
    req_str,
};
use crate::events::{mxr_callbacks_t, Bridge};

/// Bytes an IPv4 address needs when written as text, the terminator included.
pub const MXR_IP_STRING_LEN: usize = 16;

/// A running client. Opaque: everything about it is reached through the
/// functions below.
pub struct mxr_remote_t {
    pub(crate) remote: Remote,
}

/// How a client finds the network.
///
/// Zeroing the whole struct asks for the default: multicast discovery on
/// whichever interface the host picks, which is the right answer on a machine
/// with one network and an arbitrary one on any other.
#[repr(C)]
pub struct mxr_config_t {
    /// Where to send. Null means the multicast group, or the interface's
    /// broadcast address when `broadcast` is set.
    pub target_ip: *const c_char,
    /// UDP port. Zero means the default for the selected mode.
    pub port: u16,
    /// Use broadcast rather than multicast.
    pub broadcast: bool,
    /// Selects the interface by address, as text. Null lets the host choose.
    ///
    /// It decides both which interface frames leave by and which one they are
    /// accepted on. Getting it wrong on a multi-homed host fails one-sidedly:
    /// devices are still discovered, because their broadcasts arrive by any
    /// route, while every request this client sends leaves by the wrong
    /// interface and is never answered.
    pub local_ip: *const c_char,
    /// Selects the interface by name, taking precedence over `local_ip`.
    ///
    /// An interface with no address of its own - a tagged VLAN - can be named
    /// only this way, and only on Linux.
    pub interface: *const c_char,
    /// The name this client advertises to devices. Null means a default.
    pub name: *const c_char,
    /// This client's identifier, in the form `mxr_uid_to_string()` writes.
    ///
    /// Null loads it from `uid_path`, generating and storing one on first run.
    /// It must be stable across restarts, or every peer counts each run as a
    /// new client.
    ///
    /// `mxr_uid_to_string()`: crate::mxr_uid_to_string
    pub uid: *const c_char,
    /// Where the identifier is kept. Null means `.mxr-uid` in the user's home
    /// directory.
    pub uid_path: *const c_char,
}

/// Reads an address argument, where null means "not set".
unsafe fn opt_ip(ptr: *const c_char, what: &str) -> Result<Option<Ipv4Addr>, mxr_result_t> {
    // SAFETY: the caller guarantees a NUL-terminated string or null.
    let text = match unsafe { opt_str(ptr) }? {
        Some(s) => s,
        None => return Ok(None),
    };
    match Ipv4Addr::from_str(text) {
        Ok(ip) => Ok(Some(ip)),
        Err(_) => Err(fail(
            mxr_result_t::MXR_ERR_INVALID_ARGUMENT,
            &format!("{what} is not an IPv4 address: {text:?}"),
        )),
    }
}

/// Builds the core crate's configuration from the caller's.
///
/// # Safety
///
/// Every non-null pointer in `c` is a NUL-terminated string.
unsafe fn to_config(c: &mxr_config_t) -> Result<Config, mxr_result_t> {
    // SAFETY: the caller guarantees NUL-terminated strings or null.
    let (target_ip, local_ip) = unsafe {
        (
            opt_ip(c.target_ip, "target_ip")?,
            opt_ip(c.local_ip, "local_ip")?,
        )
    };
    // SAFETY: as above.
    let (interface, name, uid_text, uid_path) = unsafe {
        (
            opt_str(c.interface)?,
            opt_str(c.name)?,
            opt_str(c.uid)?,
            opt_str(c.uid_path)?,
        )
    };
    let uid = match uid_text {
        Some(text) => match DeviceUid::from_str(text) {
            Ok(uid) => Some(uid),
            Err(e) => return Err(fail(mxr_result_t::MXR_ERR_INVALID_ARGUMENT, &e.to_string())),
        },
        None => None,
    };

    // Config is non_exhaustive, so it is filled in rather than built: a field
    // added upstream keeps whatever default it is given there.
    let mut config = Config::default();
    config.target_ip = target_ip;
    // Zero is not a port a socket can be sent to, so it is how the caller says
    // nothing rather than a value to pass on.
    config.port = (c.port != 0).then_some(c.port);
    config.local_ip = local_ip;
    config.interface = interface.map(str::to_owned);
    config.broadcast = c.broadcast;
    config.name = name.map(str::to_owned);
    config.uid = uid;
    config.uid_path = uid_path.map(PathBuf::from);
    Ok(config)
}

/// Creates a client, without opening a socket yet.
///
/// `config` may be null for the defaults. `callbacks` may be null, and so may
/// any member of it: an event with no function pointer is dropped. `userdata`
/// is passed back to every callback and is never examined.
///
/// Returns null on failure, with the reason in
/// `mxr_last_error()`. The client must be released with
/// `mxr_remote_free()`.
///
/// # Safety
///
/// `config` and `callbacks` are null or point at initialised structs that
/// outlive the call, and every string in them is NUL-terminated. `userdata`
/// must remain valid, and safe to use from the library's own threads, until
/// `mxr_remote_free()` returns.
#[no_mangle]
pub unsafe extern "C" fn mxr_remote_new(
    config: *const mxr_config_t,
    callbacks: *const mxr_callbacks_t,
    userdata: *mut c_void,
) -> *mut mxr_remote_t {
    guard(std::ptr::null_mut(), || {
        // SAFETY: the caller guarantees an initialised struct or null.
        let config = match unsafe { config.as_ref() } {
            // SAFETY: its string members carry the same guarantee.
            Some(c) => match unsafe { to_config(c) } {
                Ok(c) => c,
                Err(_) => return std::ptr::null_mut(),
            },
            None => Config::default(),
        };
        // SAFETY: the caller guarantees an initialised table or null, and
        // guarantees userdata outlives the client.
        let handler: Arc<dyn EventHandler> = match unsafe { callbacks.as_ref() } {
            Some(table) => Arc::new(Bridge::new(table, userdata)),
            None => Arc::new(()),
        };
        match Remote::new(config, handler) {
            Ok(remote) => Box::into_raw(Box::new(mxr_remote_t { remote })),
            Err(e) => {
                fail(mxr_result_t::MXR_ERR_IO, &e.to_string());
                std::ptr::null_mut()
            }
        }
    })
}

/// Opens the socket and starts the receive and timer threads.
///
/// # Safety
///
/// `remote` is null or a handle from `mxr_remote_new()` that has not been
/// freed.
#[no_mangle]
pub unsafe extern "C" fn mxr_remote_start(remote: *const mxr_remote_t) -> mxr_result_t {
    // SAFETY: the caller guarantees a live handle or null.
    let handle = unsafe { remote.as_ref() };
    with(handle, |r| from_io(r.remote.start()))
}

/// Stops the threads and closes the socket. Idempotent.
///
/// A handle that has been closed can be freed but not restarted.
///
/// # Safety
///
/// `remote` is null or a handle from `mxr_remote_new()` that has not been
/// freed.
#[no_mangle]
pub unsafe extern "C" fn mxr_remote_close(remote: *const mxr_remote_t) {
    // SAFETY: the caller guarantees a live handle or null.
    let handle = unsafe { remote.as_ref() };
    with(handle, |r| {
        r.remote.close();
        mxr_result_t::MXR_OK
    });
}

/// Closes the client and releases it. Null is ignored.
///
/// This waits for the receive and timer threads to finish, so a callback
/// running when it is called returns before it does - which means calling it
/// from inside a callback would deadlock.
///
/// # Safety
///
/// `remote` is null or a handle from `mxr_remote_new()` that has not already
/// been freed, and no other thread is using it.
#[no_mangle]
pub unsafe extern "C" fn mxr_remote_free(remote: *mut mxr_remote_t) {
    guard((), || {
        if remote.is_null() {
            return;
        }
        // SAFETY: the caller guarantees a handle from mxr_remote_new that has
        // not been freed, so this reclaims the box that call leaked.
        drop(unsafe { Box::from_raw(remote) });
    });
}

/// Runs `body` on a handle, rejecting null and catching a panic.
///
/// It takes a reference rather than the raw pointer so that the one unsafe
/// step - deciding that the caller's pointer is a live handle - stays at the
/// entry point, where the caller's guarantee is written down.
pub(crate) fn with(
    remote: Option<&mxr_remote_t>,
    body: impl FnOnce(&mxr_remote_t) -> mxr_result_t,
) -> mxr_result_t {
    guard(mxr_result_t::MXR_ERR_PANIC, || match remote {
        Some(r) => body(r),
        None => fail(
            mxr_result_t::MXR_ERR_INVALID_ARGUMENT,
            "the client handle is null",
        ),
    })
}

/// Copies a list into a caller's array and reports how long the list is.
///
/// The return value is the full length whether or not it fitted, so a caller
/// can size a buffer by calling once with `cap` zero. `out` may be null only
/// when `cap` is zero.
///
/// # Safety
///
/// `out` is null or points at `cap` writable elements.
unsafe fn copy_out<T: Copy, U: Copy + Into<T>>(items: &[U], out: *mut T, cap: usize) -> usize {
    if !out.is_null() {
        // SAFETY: the caller guarantees cap writable elements at out.
        let dst = unsafe { std::slice::from_raw_parts_mut(out, cap) };
        for (slot, item) in dst.iter_mut().zip(items) {
            *slot = (*item).into();
        }
    }
    items.len()
}

/// Writes this client's own identifier.
///
/// # Safety
///
/// `remote` is null or a live handle, and `out` points at a writable
/// [`mxr_uid_t`].
#[no_mangle]
pub unsafe extern "C" fn mxr_remote_uid(
    remote: *const mxr_remote_t,
    out: *mut mxr_uid_t,
) -> mxr_result_t {
    // SAFETY: the caller guarantees a live handle or null.
    let handle = unsafe { remote.as_ref() };
    with(handle, |r| {
        if out.is_null() {
            return fail(
                mxr_result_t::MXR_ERR_INVALID_ARGUMENT,
                "uid output pointer is null",
            );
        }
        // SAFETY: checked non-null just above.
        unsafe { *out = r.remote.uid().into() };
        mxr_result_t::MXR_OK
    })
}

/// Writes the name this client advertises.
///
/// # Safety
///
/// `remote` is null or a live handle, and `out` points at `cap` writable bytes.
#[no_mangle]
pub unsafe extern "C" fn mxr_remote_name(
    remote: *const mxr_remote_t,
    out: *mut c_char,
    cap: usize,
) -> mxr_result_t {
    // SAFETY: the caller guarantees a live handle or null.
    let handle = unsafe { remote.as_ref() };
    with(handle, |r| {
        if out.is_null() || cap == 0 {
            return fail(
                mxr_result_t::MXR_ERR_INVALID_ARGUMENT,
                "name buffer is null or empty",
            );
        }
        // SAFETY: the caller guarantees cap writable bytes at out.
        put_str(
            unsafe { std::slice::from_raw_parts_mut(out, cap) },
            r.remote.name(),
        );
        mxr_result_t::MXR_OK
    })
}

/// Writes the address this client sends to.
///
/// Fails with `MXR_ERR_NOT_CONNECTED` before `mxr_remote_start()`. `ip` needs
/// [`MXR_IP_STRING_LEN`] bytes; either output may be null to skip it.
///
/// # Safety
///
/// `remote` is null or a live handle, `ip` is null or points at `cap` writable
/// bytes, and `port` is null or points at a writable `uint16_t`.
#[no_mangle]
pub unsafe extern "C" fn mxr_remote_target(
    remote: *const mxr_remote_t,
    ip: *mut c_char,
    cap: usize,
    port: *mut u16,
) -> mxr_result_t {
    // SAFETY: the caller guarantees a live handle or null.
    let handle = unsafe { remote.as_ref() };
    with(handle, |r| {
        let target = match r.remote.target() {
            Some(t) => t,
            None => {
                return fail(
                    mxr_result_t::MXR_ERR_NOT_CONNECTED,
                    "the client has no socket",
                )
            }
        };
        if !ip.is_null() {
            if cap < MXR_IP_STRING_LEN {
                return fail(
                    mxr_result_t::MXR_ERR_INVALID_ARGUMENT,
                    "address buffer is shorter than MXR_IP_STRING_LEN",
                );
            }
            // SAFETY: the caller guarantees cap writable bytes at ip.
            let dst = unsafe { std::slice::from_raw_parts_mut(ip, cap) };
            put_str(dst, &target.ip().to_string());
        }
        if !port.is_null() {
            // SAFETY: the caller guarantees a writable uint16_t at port.
            unsafe { *port = target.port() };
        }
        mxr_result_t::MXR_OK
    })
}

/// Writes every device heard from, and returns how many there are.
///
/// Returns the full count even when it exceeds `cap`, so calling with `cap`
/// zero sizes the buffer. Returns zero on a null handle.
///
/// # Safety
///
/// `remote` is null or a live handle, and `out` is null or points at `cap`
/// writable [`mxr_uid_t`].
#[no_mangle]
pub unsafe extern "C" fn mxr_devices(
    remote: *const mxr_remote_t,
    out: *mut mxr_uid_t,
    cap: usize,
) -> usize {
    guard(0, || {
        // SAFETY: the caller guarantees a live handle or null.
        let Some(r) = (unsafe { remote.as_ref() }) else {
            fail(
                mxr_result_t::MXR_ERR_INVALID_ARGUMENT,
                "the client handle is null",
            );
            return 0;
        };
        // SAFETY: the caller guarantees cap writable elements at out.
        unsafe { copy_out(&r.remote.devices(), out, cap) }
    })
}

/// Finds a device by its serial number.
///
/// # Safety
///
/// `remote` is null or a live handle, `serial` is a NUL-terminated string, and
/// `out` points at a writable [`mxr_uid_t`].
#[no_mangle]
pub unsafe extern "C" fn mxr_device_by_serial(
    remote: *const mxr_remote_t,
    serial: *const c_char,
    out: *mut mxr_uid_t,
) -> mxr_result_t {
    // SAFETY: the caller guarantees a live handle or null.
    let handle = unsafe { remote.as_ref() };
    with(handle, |r| {
        // SAFETY: the caller guarantees a NUL-terminated string.
        let serial = match unsafe { req_str(serial) } {
            Ok(s) => s,
            Err(code) => return code,
        };
        // SAFETY: the caller guarantees a writable mxr_uid_t.
        unsafe { write_uid(r.remote.device_by_serial(serial), out, "serial", serial) }
    })
}

/// Finds a device by serial number, name or identifier, in that order.
///
/// # Safety
///
/// `remote` is null or a live handle, `name` is a NUL-terminated string, and
/// `out` points at a writable [`mxr_uid_t`].
#[no_mangle]
pub unsafe extern "C" fn mxr_resolve_device(
    remote: *const mxr_remote_t,
    name: *const c_char,
    out: *mut mxr_uid_t,
) -> mxr_result_t {
    // SAFETY: the caller guarantees a live handle or null.
    let handle = unsafe { remote.as_ref() };
    with(handle, |r| {
        // SAFETY: the caller guarantees a NUL-terminated string.
        let name = match unsafe { req_str(name) } {
            Ok(s) => s,
            Err(code) => return code,
        };
        // SAFETY: the caller guarantees a writable mxr_uid_t.
        unsafe { write_uid(r.remote.resolve_device(name), out, "device", name) }
    })
}

/// Writes a device lookup's answer, or reports that it found nothing.
///
/// # Safety
///
/// `out` points at a writable [`mxr_uid_t`].
unsafe fn write_uid(
    found: Option<mx_remote::DeviceUid>,
    out: *mut mxr_uid_t,
    what: &str,
    key: &str,
) -> mxr_result_t {
    if out.is_null() {
        return fail(
            mxr_result_t::MXR_ERR_INVALID_ARGUMENT,
            "uid output pointer is null",
        );
    }
    match found {
        Some(uid) => {
            // SAFETY: checked non-null just above.
            unsafe { *out = uid.into() };
            mxr_result_t::MXR_OK
        }
        None => fail(
            mxr_result_t::MXR_ERR_NOT_FOUND,
            &format!("no device with {what} {key:?}"),
        ),
    }
}

/// Finds a bay on a device by the name the device gives its port.
///
/// # Safety
///
/// `remote` is null or a live handle, `port_name` is a NUL-terminated string,
/// and `out` points at a writable [`mxr_bay_uid_t`].
#[no_mangle]
pub unsafe extern "C" fn mxr_bay_by_name(
    remote: *const mxr_remote_t,
    device: mxr_uid_t,
    port_name: *const c_char,
    out: *mut mxr_bay_uid_t,
) -> mxr_result_t {
    // SAFETY: the caller guarantees a live handle or null.
    let handle = unsafe { remote.as_ref() };
    with(handle, |r| {
        // SAFETY: the caller guarantees a NUL-terminated string.
        let port_name = match unsafe { req_str(port_name) } {
            Ok(s) => s,
            Err(code) => return code,
        };
        let found = r.remote.bay_by_name(device.into(), port_name);
        // SAFETY: the caller guarantees a writable mxr_bay_uid_t.
        unsafe { write_bay(found, out, &format!("no bay named {port_name:?}")) }
    })
}

/// Finds the source bay advertising a multicast group.
///
/// `audio` picks which of the bay's two streams the address is matched
/// against.
///
/// # Safety
///
/// `remote` is null or a live handle, `ip` is a NUL-terminated string, and
/// `out` points at a writable [`mxr_bay_uid_t`].
#[no_mangle]
pub unsafe extern "C" fn mxr_bay_by_stream_ip(
    remote: *const mxr_remote_t,
    ip: *const c_char,
    audio: bool,
    out: *mut mxr_bay_uid_t,
) -> mxr_result_t {
    // SAFETY: the caller guarantees a live handle or null.
    let handle = unsafe { remote.as_ref() };
    with(handle, |r| {
        // SAFETY: the caller guarantees a NUL-terminated string or null.
        let ip = match unsafe { opt_ip(ip, "ip") } {
            Ok(Some(ip)) => ip,
            Ok(None) => {
                return fail(
                    mxr_result_t::MXR_ERR_INVALID_ARGUMENT,
                    "a required string argument was null",
                )
            }
            Err(code) => return code,
        };
        let found = r.remote.bay_by_stream_ip(ip, audio);
        // SAFETY: the caller guarantees a writable mxr_bay_uid_t.
        unsafe { write_bay(found, out, &format!("no bay streams to {ip}")) }
    })
}

/// Writes a bay lookup's answer, or reports that it found nothing.
///
/// # Safety
///
/// `out` points at a writable [`mxr_bay_uid_t`].
unsafe fn write_bay(
    found: Option<mx_remote::BayUid>,
    out: *mut mxr_bay_uid_t,
    message: &str,
) -> mxr_result_t {
    if out.is_null() {
        return fail(
            mxr_result_t::MXR_ERR_INVALID_ARGUMENT,
            "bay output pointer is null",
        );
    }
    match found {
        Some(bay) => {
            // SAFETY: checked non-null just above.
            unsafe { *out = bay.into() };
            mxr_result_t::MXR_OK
        }
        None => fail(mxr_result_t::MXR_ERR_NOT_FOUND, message),
    }
}

/// Reopens the socket on a different interface, or in the other mode.
///
/// `local_ip` may be null to let the host choose again.
///
/// # Safety
///
/// `remote` is null or a live handle, and `local_ip` is null or a
/// NUL-terminated string.
#[no_mangle]
pub unsafe extern "C" fn mxr_remote_update_config(
    remote: *const mxr_remote_t,
    local_ip: *const c_char,
    broadcast: bool,
) -> mxr_result_t {
    // SAFETY: the caller guarantees a live handle or null.
    let handle = unsafe { remote.as_ref() };
    with(handle, |r| {
        // SAFETY: the caller guarantees a NUL-terminated string or null.
        let ip = match unsafe { opt_ip(local_ip, "local_ip") } {
            Ok(ip) => ip,
            Err(code) => return code,
        };
        from_io(r.remote.update_config(ip, broadcast))
    })
}

/// Asks every device on the network to announce itself.
///
/// # Safety
///
/// `remote` is null or a live handle.
#[no_mangle]
pub unsafe extern "C" fn mxr_discover(remote: *const mxr_remote_t) -> mxr_result_t {
    // SAFETY: the caller guarantees a live handle or null.
    let handle = unsafe { remote.as_ref() };
    with(handle, |r| from_send(r.remote.discover()))
}
