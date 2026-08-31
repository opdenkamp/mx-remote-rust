// Author: Lars Op den Kamp (lars@opdenkamp-it.nl)
// Copyright (c) 2026 Op den Kamp IT Solutions

//! What every entry point shares: the result code, the identifiers, and the
//! guard that keeps an unwind out of C.

use std::any::Any;
use std::cell::RefCell;
use std::ffi::{c_char, CStr, CString};
use std::io;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::str::FromStr;

use mx_remote::{BayUid, ControlError, DeviceUid, SendError};

/// Bytes a [`mxr_uid_t`] needs when written as text, the terminator included.
pub const MXR_UID_STRING_LEN: usize = 36;

/// How a call ended.
///
/// Everything but [`mxr_result_t::MXR_OK`] is negative, so `if (rc < 0)` is a
/// complete test and a new code cannot turn a failure into a success.
#[repr(i32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum mxr_result_t {
    /// The call did what was asked.
    MXR_OK = 0,
    /// A pointer was null, a buffer too small, or a string not UTF-8.
    MXR_ERR_INVALID_ARGUMENT = -1,
    /// No device, bay or source by that name has been heard from.
    ///
    /// A device reports itself when it feels like it, so this is as likely to
    /// mean "not yet" as "never": the same call may succeed later.
    MXR_ERR_NOT_FOUND = -2,
    /// The addressed device speaks a protocol older than the command needs.
    ///
    /// It would discard the frame without answering, so nothing was sent.
    MXR_ERR_PROTOCOL_TOO_OLD = -3,
    /// The client has no socket, because it was never started or was closed.
    MXR_ERR_NOT_CONNECTED = -4,
    /// The socket write failed, or the socket could not be opened.
    MXR_ERR_IO = -5,
    /// The addressee does not do what was asked of it.
    MXR_ERR_UNSUPPORTED = -6,
    /// The device has not reported something the request is assembled from.
    MXR_ERR_NOT_REPORTED = -7,
    /// A panic was caught at the boundary. The library's state is unknown.
    MXR_ERR_PANIC = -8,
}

/// A flag a device may not have reported.
///
/// Firmware sends only what it has, so "off" and "never said" are different
/// answers and a two-valued flag would have to pick one of them for both.
#[repr(i8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum mxr_tribool_t {
    /// The device has not reported this.
    MXR_UNKNOWN = -1,
    /// Reported, and false.
    MXR_FALSE = 0,
    /// Reported, and true.
    MXR_TRUE = 1,
}

impl From<Option<bool>> for mxr_tribool_t {
    fn from(value: Option<bool>) -> Self {
        match value {
            None => Self::MXR_UNKNOWN,
            Some(false) => Self::MXR_FALSE,
            Some(true) => Self::MXR_TRUE,
        }
    }
}

/// The 16-byte identifier of a device on the network.
///
/// All zero is the empty identifier, which is how the protocol says "no
/// device" - see `mxr_uid_is_zero()`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct mxr_uid_t {
    /// The raw identifier, in wire order.
    pub bytes: [u8; 16],
}

/// A single bay: the device it is on, and its port number there.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct mxr_bay_uid_t {
    /// The device the bay belongs to.
    pub device: mxr_uid_t,
    /// The bay's port number on that device.
    pub port: u16,
}

impl From<DeviceUid> for mxr_uid_t {
    fn from(uid: DeviceUid) -> Self {
        Self {
            bytes: *uid.as_bytes(),
        }
    }
}

impl From<mxr_uid_t> for DeviceUid {
    fn from(uid: mxr_uid_t) -> Self {
        DeviceUid::from_array(uid.bytes)
    }
}

impl From<BayUid> for mxr_bay_uid_t {
    fn from(bay: BayUid) -> Self {
        Self {
            device: bay.device.into(),
            port: bay.port,
        }
    }
}

impl From<mxr_bay_uid_t> for BayUid {
    fn from(bay: mxr_bay_uid_t) -> Self {
        BayUid::new(bay.device.into(), bay.port)
    }
}

/// The bay a route names, where the zero device stands for "unrouted".
///
/// The protocol already spends the zero identifier on absence, so a separate
/// present flag would give the same fact two spellings that could disagree.
pub(crate) fn bay_or_zero(bay: Option<BayUid>) -> mxr_bay_uid_t {
    bay.map(mxr_bay_uid_t::from).unwrap_or_default()
}

thread_local! {
    /// Why the last call on this thread failed. Kept per thread so a failure
    /// on the receive thread cannot overwrite one the caller is about to read.
    static LAST_ERROR: RefCell<CString> = RefCell::new(c"".to_owned());
}

/// Records why a call failed, for `mxr_last_error()`.
pub(crate) fn set_last_error(message: &str) {
    // A NUL inside the text would cut the message short in C, so a message
    // that cannot be carried whole is replaced rather than truncated.
    let text = CString::new(message).unwrap_or_else(|_| c"error text contains a NUL".to_owned());
    LAST_ERROR.with(|slot| *slot.borrow_mut() = text);
}

/// Runs `body`, turning a panic into `fallback`.
///
/// Unwinding into C is undefined behaviour, so this sits inside every entry
/// point. It is the only thing here that can absorb a bug rather than report
/// it, which is why the panic message is kept: without it the caller would
/// have a result code and no way to find out what happened.
pub(crate) fn guard<T>(fallback: T, body: impl FnOnce() -> T) -> T {
    match catch_unwind(AssertUnwindSafe(body)) {
        Ok(value) => value,
        Err(payload) => {
            set_last_error(&format!("panic: {}", panic_text(&payload)));
            fallback
        }
    }
}

/// The message a panic carried, for the two payload types `panic!` produces.
fn panic_text(payload: &Box<dyn Any + Send>) -> &str {
    if let Some(s) = payload.downcast_ref::<&str>() {
        return s;
    }
    if let Some(s) = payload.downcast_ref::<String>() {
        return s;
    }
    "no message"
}

/// Reports a failure and returns its result code.
pub(crate) fn fail(code: mxr_result_t, message: &str) -> mxr_result_t {
    set_last_error(message);
    code
}

/// Turns a control failure into a result code, keeping its message.
pub(crate) fn from_control(result: Result<(), ControlError>) -> mxr_result_t {
    let error = match result {
        Ok(()) => return mxr_result_t::MXR_OK,
        Err(e) => e,
    };
    let code = match &error {
        ControlError::UnknownDevice(_)
        | ControlError::UnknownBay(_)
        | ControlError::UnknownSource(_) => mxr_result_t::MXR_ERR_NOT_FOUND,
        ControlError::Unsupported(_) => mxr_result_t::MXR_ERR_UNSUPPORTED,
        ControlError::NotReported(_) => mxr_result_t::MXR_ERR_NOT_REPORTED,
        ControlError::Send(e) => send_code(e),
        // ControlError is non_exhaustive: an unnamed variant is a failure
        // whose kind this build has no code for, never a success.
        _ => mxr_result_t::MXR_ERR_UNSUPPORTED,
    };
    fail(code, &error.to_string())
}

/// Turns a send failure into a result code, keeping its message.
pub(crate) fn from_send(result: Result<(), SendError>) -> mxr_result_t {
    match result {
        Ok(()) => mxr_result_t::MXR_OK,
        Err(e) => fail(send_code(&e), &e.to_string()),
    }
}

fn send_code(error: &SendError) -> mxr_result_t {
    match error {
        SendError::ProtocolTooOld { .. } => mxr_result_t::MXR_ERR_PROTOCOL_TOO_OLD,
        SendError::NotConnected => mxr_result_t::MXR_ERR_NOT_CONNECTED,
        SendError::Io(_) => mxr_result_t::MXR_ERR_IO,
        SendError::UnknownOpcode { .. } => mxr_result_t::MXR_ERR_UNSUPPORTED,
        _ => mxr_result_t::MXR_ERR_IO,
    }
}

/// Turns an I/O failure into a result code, keeping its message.
pub(crate) fn from_io(result: io::Result<()>) -> mxr_result_t {
    match result {
        Ok(()) => mxr_result_t::MXR_OK,
        Err(e) => fail(mxr_result_t::MXR_ERR_IO, &e.to_string()),
    }
}

/// Copies `text` into a fixed-width field, NUL-terminated and NUL-padded.
///
/// A field too narrow for the value truncates it on a character boundary
/// rather than failing the call: these are display names, and a caller reading
/// a device list would rather have a shortened name than no device.
pub(crate) fn put_str(dst: &mut [c_char], text: &str) {
    let room = dst.len().saturating_sub(1);
    let mut end = room.min(text.len());
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    let taken = text.as_bytes().get(..end).unwrap_or_default();
    for (slot, byte) in dst.iter_mut().zip(taken) {
        *slot = *byte as c_char;
    }
    for slot in dst.iter_mut().skip(end) {
        *slot = 0;
    }
}

/// Reads a caller's string, treating null as absent.
///
/// # Safety
///
/// `ptr` is null or points at a NUL-terminated string that outlives the call.
pub(crate) unsafe fn opt_str<'a>(ptr: *const c_char) -> Result<Option<&'a str>, mxr_result_t> {
    if ptr.is_null() {
        return Ok(None);
    }
    // SAFETY: the caller guarantees a NUL-terminated string.
    match unsafe { CStr::from_ptr(ptr) }.to_str() {
        Ok(s) => Ok(Some(s)),
        Err(_) => Err(fail(
            mxr_result_t::MXR_ERR_INVALID_ARGUMENT,
            "string is not valid UTF-8",
        )),
    }
}

/// Reads a caller's string, where absence is not allowed.
///
/// # Safety
///
/// `ptr` is null or points at a NUL-terminated string that outlives the call.
pub(crate) unsafe fn req_str<'a>(ptr: *const c_char) -> Result<&'a str, mxr_result_t> {
    // SAFETY: same contract as opt_str, which this only narrows.
    match unsafe { opt_str(ptr) }? {
        Some(s) => Ok(s),
        None => Err(fail(
            mxr_result_t::MXR_ERR_INVALID_ARGUMENT,
            "a required string argument was null",
        )),
    }
}

/// This library's version, as `MAJOR.MINOR.PATCH`.
///
/// The returned pointer is static and always valid.
#[no_mangle]
pub extern "C" fn mxr_version() -> *const c_char {
    // A literal, so there is nothing to fail and nothing to guard.
    concat!(env!("CARGO_PKG_VERSION"), "\0").as_ptr() as *const c_char
}

/// Why the last call on this thread failed, or an empty string.
///
/// The text is owned by the library and is replaced by the next failure on
/// this thread, so a caller that keeps it copies it first. It describes the
/// failure; the result code classifies it, and only the code should be
/// branched on.
///
/// Never returns null.
#[no_mangle]
pub extern "C" fn mxr_last_error() -> *const c_char {
    // Borrowing to take the pointer and then dropping the borrow is what the
    // caller does anyway: the string lives in the thread-local, not the guard.
    LAST_ERROR.with(|slot| slot.borrow().as_ptr())
}

/// Reports whether `uid` is the empty identifier.
///
/// The protocol uses it wherever a device could be named and is not, so this
/// is the test for "no device" rather than a comparison against a constant.
#[no_mangle]
pub extern "C" fn mxr_uid_is_zero(uid: mxr_uid_t) -> bool {
    uid.bytes == [0; 16]
}

/// Writes `uid` as dotted hex into `out`, which needs
/// [`MXR_UID_STRING_LEN`] bytes.
///
/// # Safety
///
/// `out` points at `cap` writable bytes.
#[no_mangle]
pub unsafe extern "C" fn mxr_uid_to_string(
    uid: mxr_uid_t,
    out: *mut c_char,
    cap: usize,
) -> mxr_result_t {
    guard(mxr_result_t::MXR_ERR_PANIC, || {
        if out.is_null() || cap < MXR_UID_STRING_LEN {
            return fail(
                mxr_result_t::MXR_ERR_INVALID_ARGUMENT,
                "uid buffer is null or shorter than MXR_UID_STRING_LEN",
            );
        }
        // SAFETY: the caller guarantees cap writable bytes at out.
        let dst = unsafe { std::slice::from_raw_parts_mut(out, cap) };
        put_str(dst, &DeviceUid::from(uid).to_string());
        mxr_result_t::MXR_OK
    })
}

/// Reads the dotted-hex form `mxr_uid_to_string()` writes.
///
/// # Safety
///
/// `text` points at a NUL-terminated string and `out` at a writable
/// [`mxr_uid_t`].
#[no_mangle]
pub unsafe extern "C" fn mxr_uid_from_string(
    text: *const c_char,
    out: *mut mxr_uid_t,
) -> mxr_result_t {
    guard(mxr_result_t::MXR_ERR_PANIC, || {
        if out.is_null() {
            return fail(
                mxr_result_t::MXR_ERR_INVALID_ARGUMENT,
                "uid output pointer is null",
            );
        }
        // SAFETY: the caller guarantees a NUL-terminated string or null.
        let text = match unsafe { req_str(text) } {
            Ok(s) => s,
            Err(code) => return code,
        };
        match DeviceUid::from_str(text) {
            Ok(uid) => {
                // SAFETY: checked non-null above, and mxr_uid_t is plain bytes.
                unsafe { *out = uid.into() };
                mxr_result_t::MXR_OK
            }
            Err(e) => fail(mxr_result_t::MXR_ERR_INVALID_ARGUMENT, &e.to_string()),
        }
    })
}
