// Author: Lars Op den Kamp (lars@opdenkamp-it.nl)
// Copyright (c) 2026 Op den Kamp IT Solutions

//! What the boundary does, exercised through the boundary.
//!
//! The protocol is the core crate's to test and is tested there. What is only
//! testable here is the translation: that a caller's mistake becomes a code
//! rather than a crash, that a value survives the trip out and back, and that
//! a failure says why.
//!
//! No test opens a socket, and every client is given an identifier so that
//! none reaches the file where one would otherwise be stored.

use std::ffi::{c_char, CStr, CString};
use std::ptr;

use mx_remote_ffi::*;

/// A distinguishable identifier, from one byte.
fn uid_n(n: u8) -> mxr_uid_t {
    let mut bytes = [0u8; 16];
    bytes[0] = n;
    bytes[15] = 0xa5;
    mxr_uid_t { bytes }
}

/// A client that has an identifier of its own and no socket.
fn client(name: &CStr, uid: &CStr) -> *mut mxr_remote_t {
    let config = mxr_config_t {
        target_ip: ptr::null(),
        port: 0,
        broadcast: false,
        local_ip: ptr::null(),
        interface: ptr::null(),
        name: name.as_ptr(),
        uid: uid.as_ptr(),
        uid_path: ptr::null(),
    };
    // SAFETY: every pointer in the config is a live NUL-terminated string, and
    // a null callback table asks for no callbacks.
    let handle = unsafe { mxr_remote_new(&config, ptr::null(), ptr::null_mut()) };
    assert!(
        !handle.is_null(),
        "the client was not created: {}",
        last_error()
    );
    handle
}

/// The message behind the last failure on this thread.
fn last_error() -> String {
    // SAFETY: mxr_last_error never returns null, and the string it points at
    // lives until the next failure on this thread.
    unsafe { CStr::from_ptr(mxr_last_error()) }
        .to_string_lossy()
        .into_owned()
}

/// Reads a fixed-width field the way C would.
fn field(bytes: &[c_char]) -> String {
    let text: Vec<u8> = bytes
        .iter()
        .take_while(|b| **b != 0)
        .map(|b| *b as u8)
        .collect();
    String::from_utf8_lossy(&text).into_owned()
}

#[test]
fn an_identifier_survives_the_trip_out_to_text_and_back() {
    let uid = uid_n(7);
    let mut text = [0 as c_char; MXR_UID_STRING_LEN];
    // SAFETY: the buffer is MXR_UID_STRING_LEN bytes, which is what is asked for.
    let rc = unsafe { mxr_uid_to_string(uid, text.as_mut_ptr(), text.len()) };
    assert_eq!(rc, mxr_result_t::MXR_OK);

    let mut back = mxr_uid_t::default();
    // SAFETY: text is NUL-terminated by the call above, and back is writable.
    let rc = unsafe { mxr_uid_from_string(text.as_ptr(), &mut back) };
    assert_eq!(rc, mxr_result_t::MXR_OK);
    assert_eq!(
        back,
        uid,
        "{} did not read back as it was written",
        field(&text)
    );
}

#[test]
fn a_buffer_too_short_for_an_identifier_is_refused_rather_than_filled() {
    let mut text = [0 as c_char; MXR_UID_STRING_LEN - 1];
    // SAFETY: the buffer is as long as the length passed with it.
    let rc = unsafe { mxr_uid_to_string(uid_n(1), text.as_mut_ptr(), text.len()) };
    assert_eq!(rc, mxr_result_t::MXR_ERR_INVALID_ARGUMENT);
    assert_eq!(text[0], 0, "a refused call still wrote to the buffer");
}

#[test]
fn text_that_is_not_an_identifier_is_refused_rather_than_read_as_zero() {
    let text = CString::new("not a uid").expect("no NUL");
    let mut out = uid_n(3);
    // SAFETY: both pointers are live, and the text is NUL-terminated.
    let rc = unsafe { mxr_uid_from_string(text.as_ptr(), &mut out) };
    assert_eq!(rc, mxr_result_t::MXR_ERR_INVALID_ARGUMENT);
    assert_eq!(out, uid_n(3), "a refused parse still wrote an identifier");
    assert!(
        last_error().contains("not a uid"),
        "the failure did not say what could not be read: {}",
        last_error()
    );
}

#[test]
fn the_empty_identifier_is_the_one_the_protocol_spells_absence_with() {
    assert!(mxr_uid_is_zero(mxr_uid_t::default()));
    assert!(!mxr_uid_is_zero(uid_n(1)));
}

/// A null handle must reach a result code, never a dereference.
///
/// Every entry point takes one, and they divide into three shapes by what they
/// have to return when there is nothing to work with: a code, a count, or
/// nothing at all. One of each is enough to show the shape is handled; what
/// would not be caught is a single function that forgot, which is why the
/// check lives in one place they all call.
#[test]
fn a_null_handle_is_an_argument_error_rather_than_a_crash() {
    let mut info = std::mem::MaybeUninit::<mxr_device_info_t>::zeroed();
    // SAFETY: a null handle is what is under test; the output is writable.
    let rc = unsafe { mxr_device(ptr::null(), uid_n(1), info.as_mut_ptr()) };
    assert_eq!(rc, mxr_result_t::MXR_ERR_INVALID_ARGUMENT);
    assert!(!last_error().is_empty(), "the failure said nothing");

    // SAFETY: a null handle, and a null buffer with a zero capacity to match.
    let count = unsafe { mxr_devices(ptr::null(), ptr::null_mut(), 0) };
    assert_eq!(count, 0);

    // SAFETY: a null handle is what is under test.
    let rc = unsafe { mxr_power_on(ptr::null(), mxr_bay_uid_t::default()) };
    assert_eq!(rc, mxr_result_t::MXR_ERR_INVALID_ARGUMENT);

    // SAFETY: closing nothing must be a no-op, not a dereference.
    unsafe { mxr_remote_close(ptr::null()) };
    // SAFETY: freeing null is documented as ignored.
    unsafe { mxr_remote_free(ptr::null_mut()) };
}

#[test]
fn a_client_reports_the_identifier_and_name_it_was_given() {
    let name = CString::new("test client").expect("no NUL");
    let uid_text = CString::new("00000007.00000000.00000000.a5000000").expect("no NUL");
    let handle = client(&name, &uid_text);

    let mut uid = mxr_uid_t::default();
    // SAFETY: the handle is live and the output is writable.
    let rc = unsafe { mxr_remote_uid(handle, &mut uid) };
    assert_eq!(rc, mxr_result_t::MXR_OK);
    assert_eq!(uid, uid_n(7), "the configured identifier was not kept");

    let mut buf = [0 as c_char; 32];
    // SAFETY: the handle is live, and the buffer is as long as it is said to be.
    let rc = unsafe { mxr_remote_name(handle, buf.as_mut_ptr(), buf.len()) };
    assert_eq!(rc, mxr_result_t::MXR_OK);
    assert_eq!(field(&buf), "test client");

    // SAFETY: the handle came from mxr_remote_new and is freed once.
    unsafe { mxr_remote_free(handle) };
}

#[test]
fn a_client_that_was_never_started_has_no_target() {
    let name = CString::new("unstarted").expect("no NUL");
    let uid_text = CString::new("00000001.00000000.00000000.00000000").expect("no NUL");
    let handle = client(&name, &uid_text);

    let mut port = 0u16;
    // SAFETY: the handle is live; a null address buffer skips that output.
    let rc = unsafe { mxr_remote_target(handle, ptr::null_mut(), 0, &mut port) };
    assert_eq!(rc, mxr_result_t::MXR_ERR_NOT_CONNECTED);

    // SAFETY: the handle came from mxr_remote_new and is freed once.
    unsafe { mxr_remote_free(handle) };
}

#[test]
fn a_command_for_a_device_never_heard_from_is_not_found() {
    let name = CString::new("no devices").expect("no NUL");
    let uid_text = CString::new("00000002.00000000.00000000.00000000").expect("no NUL");
    let handle = client(&name, &uid_text);

    // Not MXR_ERR_NOT_CONNECTED: the gate refuses this before the socket is
    // ever reached, so an unstarted client and a started one answer alike.
    // SAFETY: the handle is live.
    let rc = unsafe { mxr_reboot(handle, uid_n(9)) };
    assert_eq!(rc, mxr_result_t::MXR_ERR_NOT_FOUND);
    assert!(
        last_error().contains("no device"),
        "the failure did not say what was missing: {}",
        last_error()
    );

    // SAFETY: the handle came from mxr_remote_new and is freed once.
    unsafe { mxr_remote_free(handle) };
}

#[test]
fn a_configuration_that_cannot_be_read_yields_no_client() {
    let bad = CString::new("300.1.1.1").expect("no NUL");
    let config = mxr_config_t {
        target_ip: ptr::null(),
        port: 0,
        broadcast: false,
        local_ip: bad.as_ptr(),
        interface: ptr::null(),
        name: ptr::null(),
        uid: ptr::null(),
        uid_path: ptr::null(),
    };
    // SAFETY: every non-null pointer in the config is a live NUL-terminated
    // string, and a null callback table asks for no callbacks.
    let handle = unsafe { mxr_remote_new(&config, ptr::null(), ptr::null_mut()) };
    assert!(
        handle.is_null(),
        "a client was built from an unreadable address"
    );
    assert!(
        last_error().contains("local_ip"),
        "the failure did not name the field: {}",
        last_error()
    );
}

#[test]
fn a_list_call_reports_its_length_when_given_nowhere_to_write() {
    let name = CString::new("sizing").expect("no NUL");
    let uid_text = CString::new("00000003.00000000.00000000.00000000").expect("no NUL");
    let handle = client(&name, &uid_text);

    // SAFETY: the handle is live; a null buffer with a zero capacity asks only
    // for the count.
    let count = unsafe { mxr_devices(handle, ptr::null_mut(), 0) };
    assert_eq!(count, 0, "a client that has heard nothing listed a device");

    // SAFETY: the handle came from mxr_remote_new and is freed once.
    unsafe { mxr_remote_free(handle) };
}

#[test]
fn the_version_is_the_crate_version() {
    // SAFETY: mxr_version returns a static NUL-terminated string.
    let version = unsafe { CStr::from_ptr(mxr_version()) };
    assert_eq!(version.to_str().expect("ASCII"), env!("CARGO_PKG_VERSION"));
}

/// A subsystem read must say which of the two things went wrong.
///
/// "No such device" and "that device has not sent this" lead somewhere
/// different - the first is a wrong identifier, the second is a wait - and a
/// single code for both would leave a caller unable to tell a typo from a
/// device that has not got round to it. Only the first is reachable from here,
/// because reaching the second needs a device to have said something.
#[test]
fn a_subsystem_of_a_device_never_heard_from_is_not_found() {
    let name = CString::new("subsystems").expect("no NUL");
    let uid_text = CString::new("00000004.00000000.00000000.00000000").expect("no NUL");
    let handle = client(&name, &uid_text);

    let mut stats = std::mem::MaybeUninit::<mxr_v2ip_stats_t>::zeroed();
    // SAFETY: the handle is live and the output is writable.
    let rc = unsafe { mxr_v2ip_stats(handle, uid_n(4), stats.as_mut_ptr()) };
    assert_eq!(rc, mxr_result_t::MXR_ERR_NOT_FOUND);
    assert!(
        last_error().contains("no device"),
        "the failure did not say the device was missing: {}",
        last_error()
    );

    // SAFETY: the handle is live; a null buffer with a zero capacity asks only
    // for the count.
    let count = unsafe { mxr_audio_endpoints(handle, uid_n(4), ptr::null_mut(), 0) };
    assert_eq!(count, 0);

    // SAFETY: the handle came from mxr_remote_new and is freed once.
    unsafe { mxr_remote_free(handle) };
}

#[test]
fn a_subsystem_read_without_somewhere_to_write_is_an_argument_error() {
    let name = CString::new("no output").expect("no NUL");
    let uid_text = CString::new("00000005.00000000.00000000.00000000").expect("no NUL");
    let handle = client(&name, &uid_text);

    // The null output is caught before the lookup, so this is an argument
    // error rather than the not-found the same call would otherwise give.
    // SAFETY: the handle is live; a null output is what is under test.
    let rc = unsafe { mxr_multiviewer_status(handle, uid_n(5), ptr::null_mut()) };
    assert_eq!(rc, mxr_result_t::MXR_ERR_INVALID_ARGUMENT);

    // SAFETY: the handle came from mxr_remote_new and is freed once.
    unsafe { mxr_remote_free(handle) };
}
