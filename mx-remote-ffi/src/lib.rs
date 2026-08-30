// Author: Lars Op den Kamp (lars@opdenkamp-it.nl)
// Copyright (c) 2026 Op den Kamp IT Solutions

#![deny(missing_docs)]
// Every type here is named as it appears in C, because C is the only thing
// that reads it. A header whose names were transliterated from Rust would make
// the reader translate in both directions.
#![allow(non_camel_case_types)]

//! C ABI for [`mx_remote`].
//!
//! The whole of this crate is one translation: it holds every `unsafe` in the
//! workspace, and adds no protocol logic of its own. Anything a caller can do
//! here is something the core crate already does; if a rule is not enforced in
//! `mx_remote`, it is not enforced at all.
//!
//! Three conventions run through the header, and knowing them is most of
//! knowing the API:
//!
//! - **A device is addressed by value.** There is no handle for a device or a
//!   bay: a device is a [`mxr_uid_t`], a bay is a [`mxr_bay_uid_t`], and state
//!   is read by passing one in and having a struct filled out. Nothing hands
//!   back a pointer into state that a lock protects.
//! - **Every call returns, whatever happens.** An unwind across the boundary
//!   is undefined behaviour, so each entry point catches: a panic becomes
//!   [`mxr_result_t::MXR_ERR_PANIC`] rather than a corrupted stack.
//! - **A borrowed pointer lives for one call.** Strings and arrays handed to a
//!   callback point into memory the library owns and reuses. A caller that
//!   needs one afterwards copies it.
//!
//! Every failure code is negative and [`mxr_result_t::MXR_OK`] is zero, so
//! `if (rc < 0)` is a complete test, and a code added later cannot turn a
//! failure into a success.
//!
//! # From C
//!
//! ```c
//! #include <mx_remote.h>
//!
//! /* The client cannot be its own userdata: it does not exist when the table
//!  * is handed over, so it reaches the callbacks through a struct that does. */
//! struct app { mxr_remote_t *remote; };
//!
//! static void on_device_update(void *userdata, mxr_uid_t device) {
//!     mxr_device_info_t info;
//!     if (mxr_device(((struct app *)userdata)->remote, device, &info) == MXR_OK)
//!         printf("%s %s\n", info.model, info.name);
//! }
//!
//! int main(void) {
//!     struct app app = {0};
//!     mxr_callbacks_t cb = {0};
//!     cb.on_device_update = on_device_update;
//!
//!     /* Zeroing a config asks for every default; NULL does the same. */
//!     app.remote = mxr_remote_new(NULL, &cb, &app);
//!     mxr_remote_start(app.remote);
//!     /* ... */
//!     mxr_remote_free(app.remote);
//!     return 0;
//! }
//! ```
//!
//! `include/mx_remote.h` is generated from this source by cbindgen and checked
//! into the repository. `include/mx_remote.hpp` is a hand-written header-only
//! C++ layer over it, with a move-only `mxr::Remote` that closes and joins in
//! its destructor and an `mxr::Handler` base class carrying a virtual method
//! per event.

mod abi;
mod control;
mod events;
mod info;
mod remote;
mod subsystems;

// Everything the header exposes, re-exported flat: a Rust caller reaching for
// this crate wants the same surface a C caller gets, under the same names.
pub use abi::*;
pub use control::*;
pub use events::*;
pub use info::*;
pub use remote::*;
pub use subsystems::*;
