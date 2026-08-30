# mx-remote-ffi

The C ABI over [`mx-remote`], a client for Pulse-Eight AV distribution
hardware: neo HDBaseT matrices, OneIP HDMI-over-IP units and multiviewers, and
the ProAmp8 8-zone amplifier. They all run the same MatrixOS firmware and speak
one protocol, MX Remote, over UDP multicast or broadcast.

This crate holds every `unsafe` in the workspace and adds no protocol logic of
its own. Anything a caller can do here is something `mx-remote` already does.

```bash
cargo build -p mx-remote-ffi --release
cc -Iinclude prog.c target/release/libmx_remote_ffi.a -lpthread -ldl -lm
```

The archive is the point: it links into a C or C++ program with no runtime to
initialise, takes no signal handlers from the host process, and pulls in
nothing but libc and pthreads.

```c
#include <mx_remote.h>

/* The client cannot be its own userdata: it does not exist when the table is
 * handed over, so it reaches the callbacks through a struct that does. */
struct app { mxr_remote_t *remote; };

static void on_device_update(void *userdata, mxr_uid_t device) {
    mxr_device_info_t info;
    if (mxr_device(((struct app *)userdata)->remote, device, &info) == MXR_OK)
        printf("%s %s\n", info.model, info.name);
}

int main(void) {
    struct app app = {0};
    mxr_callbacks_t cb = {0};
    cb.on_device_update = on_device_update;

    /* Zeroing a config asks for every default; NULL does the same. */
    app.remote = mxr_remote_new(NULL, &cb, &app);
    mxr_remote_start(app.remote);
    /* ... */
    mxr_remote_free(app.remote);
    return 0;
}
```

## Three conventions

Knowing them is most of knowing the API.

- **A device is addressed by value.** There is no handle for a device or a bay:
  a device is an `mxr_uid_t`, a bay is an `mxr_bay_uid_t`, and state is read by
  passing one in and having a caller-owned struct filled out. Nothing hands
  back a pointer into state that a lock protects.
- **Every call returns, whatever happens.** A panic becomes `MXR_ERR_PANIC`;
  nothing unwinds across the boundary. Every failure code is negative and
  `MXR_OK` is zero, so `if (rc < 0)` is a complete test.
- **A borrowed pointer lives for one call.** Strings and arrays handed to a
  callback point into memory the library owns and reuses. A caller that needs
  one afterwards copies it.

## The headers

`include/mx_remote.h` is generated from this crate's source by cbindgen and
checked into the repository; CI fails if regenerating it produces a diff.

`include/mx_remote.hpp` is a hand-written header-only layer over it: a
move-only `mxr::Remote` that closes and joins in its destructor, `mxr::Uid` and
`mxr::BayUid` value types, and an `mxr::Handler` base class with a virtual
method per event. Declare the handler before the client, so the destructor that
joins the receive thread runs before the handler it calls into is gone.

## Licence

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or
[MIT license](LICENSE-MIT) at your option.

[`mx-remote`]: https://crates.io/crates/mx-remote
