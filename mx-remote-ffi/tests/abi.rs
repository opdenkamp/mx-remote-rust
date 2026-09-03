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

use mx_remote::{V2ipDecoderDetail, V2ipDecoderFormat, V2ipDecoderReason, V2ipDecoderReport};
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

/// One named bit, as either crate spells it.
struct Bit {
    name: String,
    value: u64,
}

/// Reads `path` relative to the workspace root, failing rather than skipping.
///
/// A test that shrugs at a missing file reports every constant correct when it
/// found none, which is the one answer it must not be able to give.
fn workspace_source(path: &str) -> String {
    let full = format!("{}/../{path}", env!("CARGO_MANIFEST_DIR"));
    let text =
        std::fs::read_to_string(&full).unwrap_or_else(|e| panic!("{full} could not be read: {e}"));
    // The blocks are located by patterns written with "\n", so a checkout that
    // stores line endings the other way would report a missing block rather
    // than a constant that disagrees - a pass this test must not be able to
    // give for a reason that has nothing to do with the constants.
    text.replace("\r\n", "\n")
}

/// Evaluates the expressions these constants are written with: a decimal or
/// hexadecimal literal, or a shift of one. Anything else fails rather than
/// being skipped over.
fn value_of(expr: &str, whose: &str) -> u64 {
    let expr = expr.trim();
    if let Some(hex) = expr.strip_prefix("0x") {
        return u64::from_str_radix(hex, 16)
            .unwrap_or_else(|_| panic!("{whose}: {expr} is not a hexadecimal literal"));
    }
    if let Some((lhs, rhs)) = expr.split_once("<<") {
        let base: u64 = lhs
            .trim()
            .parse()
            .unwrap_or_else(|_| panic!("{whose}: {expr}"));
        let shift: u32 = rhs
            .trim()
            .parse()
            .unwrap_or_else(|_| panic!("{whose}: {expr}"));
        return base << shift;
    }
    expr.parse()
        .unwrap_or_else(|_| panic!("{whose}: {expr} is neither a literal nor a shift"))
}

/// The named constants of one `bitmask!` or `wire_enum!` block.
fn core_bits(source: &str, block: &str) -> Vec<Bit> {
    let start = source
        .find(&format!("\n    {block} {{"))
        .or_else(|| source.find(&format!("\n    {block}: ")))
        .unwrap_or_else(|| panic!("{block} is not in the core source under that name"));
    let body = &source[start..];
    let end = body
        .find("\n    }\n}")
        .unwrap_or_else(|| panic!("{block} has no end"));
    read_bits(&body[..end], block, |line| {
        let (name, expr) = line.strip_suffix(';')?.split_once(" = ")?;
        Some((name.to_owned(), expr.to_owned()))
    })
}

/// The named constants of an `impl` block that declares them one by one.
fn core_consts(source: &str, ty: &str) -> Vec<Bit> {
    read_bits(source, ty, |line| {
        let rest = line.strip_prefix("pub const ")?;
        let (name, rest) = rest.split_once(": Self = Self(")?;
        Some((name.to_owned(), rest.strip_suffix(");")?.to_owned()))
    })
}

/// The constants of `bits.rs` whose name starts with `prefix`, and not with
/// any of `not`.
fn header_bits(source: &str, prefix: &str, not: &[&str]) -> Vec<Bit> {
    read_bits(source, prefix, |line| {
        let rest = line.strip_prefix("pub const ")?;
        let (name, rest) = rest.split_once(": ")?;
        let expr = rest.split_once(" = ")?.1.strip_suffix(';')?;
        Some((name.to_owned(), expr.to_owned()))
    })
    .into_iter()
    .filter(|b| b.name.starts_with(prefix) && !not.iter().any(|n| b.name.starts_with(n)))
    .collect()
}

fn read_bits(
    source: &str,
    whose: &str,
    parse: impl Fn(&str) -> Option<(String, String)>,
) -> Vec<Bit> {
    source
        .lines()
        .map(str::trim)
        .filter_map(&parse)
        .map(|(name, expr)| Bit {
            value: value_of(&expr, whose),
            name,
        })
        .collect()
}

/// Every bit the core crate names reaches the header, at the same value.
///
/// The header's copies are written out as literals because cbindgen puts a
/// constant's expression into the header verbatim and cannot evaluate a call,
/// so the two lists are genuinely separate and drift is the risk. Both
/// directions are checked: a core bit with no name in the header would leave a
/// C caller writing the number itself, and a header bit with no core bit
/// behind it names something that no longer exists.
///
/// Each list is also held to a minimum length, so a rename that leaves a
/// pattern matching nothing fails here instead of reporting every constant
/// correct.
#[test]
fn every_core_bit_reaches_the_header_at_its_own_value() {
    let enums = workspace_source("mx-remote/src/wire/enums.rs");
    let audio = workspace_source("mx-remote/src/types/audio.rs");
    let header = workspace_source("mx-remote-ffi/src/bits.rs");

    let lists: [(&str, Vec<Bit>, &[&str], usize); 15] = [
        ("MXR_FEATURE_", core_bits(&enums, "DeviceFeature"), &[], 27),
        (
            "MXR_BAY_",
            core_bits(&enums, "BayFeatures"),
            &["MXR_BAY_STATUS_"],
            17,
        ),
        ("MXR_BAY_STATUS_", core_bits(&enums, "BayStatus"), &[], 18),
        ("MXR_KEY_", core_bits(&enums, "RcKey"), &[], 48),
        ("MXR_AUDIO_", core_consts(&audio, "AudioFeatures"), &[], 15),
        (
            "MXR_MV_VIEW_MODE_",
            core_bits(&enums, "MultiviewerViewMode"),
            &[],
            9,
        ),
        (
            "MXR_MV_PIP_POSITION_",
            core_bits(&enums, "MultiviewerPipPosition"),
            &[],
            5,
        ),
        (
            "MXR_MV_PIP_SIZE_",
            core_bits(&enums, "MultiviewerPipSize"),
            &[],
            4,
        ),
        (
            "MXR_MV_OUTPUT_",
            core_bits(&enums, "MultiviewerOutputMode"),
            &[],
            15,
        ),
        (
            "MXR_MV_HDCP_",
            core_bits(&enums, "MultiviewerHdcpMode"),
            &[],
            4,
        ),
        // The EDID names already carry their own word, so they take the bare
        // multiviewer prefix and every other multiviewer list is excluded
        // rather than the names being spelled MXR_MV_EDID_EDID_*.
        (
            "MXR_MV_",
            core_bits(&enums, "MultiviewerEdidTemplate"),
            &[
                "MXR_MV_VIEW_MODE_",
                "MXR_MV_PIP_",
                "MXR_MV_OUTPUT_",
                "MXR_MV_HDCP_",
                "MXR_MV_ITC_",
                "MXR_MV_ASPECT_",
                "MXR_MV_BOOL_",
                "MXR_MV_SOURCE_",
            ],
            20,
        ),
        (
            "MXR_MV_ITC_",
            core_bits(&enums, "MultiviewerItcMode"),
            &[],
            3,
        ),
        (
            "MXR_MV_ASPECT_",
            core_bits(&enums, "MultiviewerAspectRatio"),
            &[],
            3,
        ),
        ("MXR_MV_BOOL_", core_bits(&enums, "MultiviewerBool"), &[], 3),
        (
            "MXR_MV_SOURCE_",
            core_bits(&enums, "MultiviewerSource"),
            &[],
            5,
        ),
    ];

    for (prefix, core, not, minimum) in lists {
        assert!(
            core.len() >= minimum,
            "{prefix}: found {} constants in the core crate, expected at least {minimum}; \
             the pattern that reads them has stopped matching",
            core.len()
        );
        let mine = header_bits(&header, prefix, not);
        assert_eq!(
            mine.len(),
            core.len(),
            "{prefix}: the header names {} of the core crate's {}",
            mine.len(),
            core.len()
        );
        for bit in &core {
            let name = format!("{prefix}{}", bit.name);
            let found = mine
                .iter()
                .find(|b| b.name == name)
                .unwrap_or_else(|| panic!("{name} is in the core crate and not in the header"));
            assert_eq!(found.value, bit.value, "{name} differs between the two");
        }
    }
}

/// The unset signal type is not a format, whichever way a sender says so.
#[test]
fn the_signal_type_accessors_read_a_packed_word() {
    // svd 16, colour space 1, bit-depth index 2.
    let word = 16 | (1 << 8) | (2 << 13);
    assert_eq!(mxr_signal_type_svd(word), 16);
    assert_eq!(mxr_signal_type_colour_space(word), 1);
    assert_eq!(mxr_signal_type_bpp_index(word), 2);
    // The wire carries an index into a table of depths, not a depth.
    assert_eq!(mxr_signal_type_bpp(word), 10);
    assert!(mxr_signal_type_is_set(word));

    // A bay with nothing configured, said both ways.
    assert!(!mxr_signal_type_is_set(5 << 13));
    assert!(!mxr_signal_type_is_set(0));
    assert_eq!(mxr_signal_type_bpp(5 << 13), 0);

    // Bits 16-19 and 22-23 of a status word are fields, not flags.
    let status = MXR_BAY_STATUS_SIGNAL_DETECTED | (3 << 16) | (2 << 22);
    assert_eq!(mxr_bay_status_rc_type(status), 3);
    assert_eq!(mxr_bay_status_hdcp(status), 2);
}

/// Every entry point added for a C consumer rejects a null handle rather than
/// dereferencing one, and says why.
#[test]
fn the_new_entry_points_refuse_a_null_handle() {
    let route = mxr_v2ip_route_t {
        video: mxr_stream_addr_t {
            ip: ptr::null(),
            port: 0,
        },
        audio: mxr_stream_addr_t {
            ip: ptr::null(),
            port: 0,
        },
        anc: mxr_stream_addr_t {
            ip: ptr::null(),
            port: 0,
        },
    };
    let mut edid = [0u8; MXR_EDID_LEN];
    let mut audio = std::mem::MaybeUninit::<mxr_audio_details_t>::zeroed();
    let mut frames = 0u64;

    // SAFETY: a null handle is what is under test; every other argument is
    // valid, so an argument error can only be the handle.
    let calls = unsafe {
        [
            mxr_select_source_addr(ptr::null(), mxr_bay_uid_t::default(), &route, ptr::null()),
            mxr_send_key(ptr::null(), mxr_bay_uid_t::default(), MXR_KEY_PLAY),
            mxr_request_edid(ptr::null(), uid_n(1), true),
            mxr_request_signal_status(ptr::null(), uid_n(1)),
            mxr_bay_audio_details(ptr::null(), mxr_bay_uid_t::default(), audio.as_mut_ptr()),
            mxr_device_edid(ptr::null(), uid_n(1), true, edid.as_mut_ptr(), edid.len()),
            mxr_frames_received(ptr::null(), &mut frames),
        ]
    };
    for (n, rc) in calls.into_iter().enumerate() {
        assert_eq!(rc, mxr_result_t::MXR_ERR_INVALID_ARGUMENT, "call {n}");
    }
    assert!(!last_error().is_empty(), "the failure said nothing");
}

/// What the new reads answer on a client that has heard from nothing.
///
/// "Not found" and "not reported" are different answers and a caller acts on
/// them differently, so a client with no devices must not give either of them
/// where the other belongs.
#[test]
fn the_new_reads_separate_an_unknown_device_from_an_unreported_value() {
    let remote = client(c"abi-new-reads", c"00000021.00000000.00000000.000000a5");

    let mut frames = 1234u64;
    // SAFETY: a live handle and a writable u64.
    assert_eq!(
        unsafe { mxr_frames_received(remote, &mut frames) },
        mxr_result_t::MXR_OK
    );
    assert_eq!(frames, 0, "a client with no socket has heard nothing");

    let mut edid = [0u8; MXR_EDID_LEN];
    // SAFETY: a live handle and a buffer of exactly the documented size.
    let rc = unsafe { mxr_device_edid(remote, uid_n(9), true, edid.as_mut_ptr(), edid.len()) };
    assert_eq!(rc, mxr_result_t::MXR_ERR_NOT_REPORTED);

    // A buffer too short is the caller's mistake, not a missing report.
    // SAFETY: a live handle, and a capacity that matches the shortened slice.
    let rc = unsafe { mxr_device_edid(remote, uid_n(9), true, edid.as_mut_ptr(), 8) };
    assert_eq!(rc, mxr_result_t::MXR_ERR_INVALID_ARGUMENT);

    let mut audio = std::mem::MaybeUninit::<mxr_audio_details_t>::zeroed();
    // SAFETY: a live handle and a writable struct.
    let rc = unsafe { mxr_bay_audio_details(remote, mxr_bay_uid_t::default(), audio.as_mut_ptr()) };
    assert_eq!(rc, mxr_result_t::MXR_ERR_NOT_FOUND, "no such bay");

    // SAFETY: created above and not yet freed.
    unsafe { mxr_remote_free(remote) };
}

/// A route with an address that is not one is refused before anything is sent.
#[test]
fn a_route_with_an_unparseable_address_is_an_argument_error() {
    let remote = client(c"abi-bad-route", c"00000022.00000000.00000000.000000a5");
    let bad = CString::new("not an address").expect("no NUL");
    let route = mxr_v2ip_route_t {
        video: mxr_stream_addr_t {
            ip: bad.as_ptr(),
            port: 0,
        },
        audio: mxr_stream_addr_t {
            ip: ptr::null(),
            port: 0,
        },
        anc: mxr_stream_addr_t {
            ip: ptr::null(),
            port: 0,
        },
    };
    // SAFETY: a live handle, and a route whose one address is a live string.
    let rc =
        unsafe { mxr_select_source_addr(remote, mxr_bay_uid_t::default(), &route, ptr::null()) };
    assert_eq!(rc, mxr_result_t::MXR_ERR_INVALID_ARGUMENT);
    assert!(
        last_error().contains("not an address"),
        "the failure did not name the address it could not read: {}",
        last_error()
    );

    // A null route pointer is the other way to get this wrong.
    // SAFETY: a live handle and a deliberately null route.
    let rc = unsafe {
        mxr_select_source_addr(remote, mxr_bay_uid_t::default(), ptr::null(), ptr::null())
    };
    assert_eq!(rc, mxr_result_t::MXR_ERR_INVALID_ARGUMENT);

    // SAFETY: created above and not yet freed.
    unsafe { mxr_remote_free(remote) };
}

/// Only one of the three decoder states carries a reading, and the other two
/// carry nothing rather than a stale copy of one.
///
/// A C caller has no `Option` to stop it reading the geometry regardless, so
/// what stops it reporting a 4K picture from a sink whose decoder has never
/// answered is that the fields beside `detail` are zero.
#[test]
fn only_an_answered_decoder_carries_a_reading() {
    let answered: mxr_v2ip_decoder_t = V2ipDecoderDetail::Answered(V2ipDecoderReport {
        reason: V2ipDecoderReason::PTP_UNLOCKED,
        blocking: true,
        width: 3840,
        height: 2160,
        format: V2ipDecoderFormat::YCBCR_420,
        updates: 600,
        flags: 1 << 8,
        blocked_count: 100_009,
    })
    .into();
    assert_eq!(
        answered.detail,
        mxr_v2ip_decoder_detail_t::MXR_V2IP_DECODER_ANSWERED
    );
    assert_eq!(answered.reason, 8);
    assert!(answered.blocking);
    assert_eq!((answered.width, answered.height), (3840, 2160));
    assert_eq!(answered.format, 3);
    assert_eq!(answered.updates, 600);
    assert_eq!(answered.flags, 1 << 8);
    assert_eq!(answered.blocked_count, 100_009);

    for (detail, expected) in [
        (
            V2ipDecoderDetail::Absent,
            mxr_v2ip_decoder_detail_t::MXR_V2IP_DECODER_ABSENT,
        ),
        (
            V2ipDecoderDetail::NeverAnswered,
            mxr_v2ip_decoder_detail_t::MXR_V2IP_DECODER_NEVER_ANSWERED,
        ),
    ] {
        let empty: mxr_v2ip_decoder_t = detail.into();
        assert_eq!(empty.detail, expected);
        assert!(!empty.blocking);
        assert_eq!(
            (
                u32::from(empty.reason),
                u32::from(empty.width),
                u32::from(empty.height),
                u32::from(empty.format),
                u32::from(empty.updates),
                empty.flags,
                empty.blocked_count,
            ),
            (0, 0, 0, 0, 0, 0, 0),
            "{expected:?} carried a reading it does not have"
        );
    }
}
