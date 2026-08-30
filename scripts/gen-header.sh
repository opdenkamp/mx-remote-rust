#!/bin/sh
# Regenerates include/mx_remote.h from the mx-remote-ffi crate.
#
# The result is checked in, and CI runs this and fails on a diff: third parties
# read the header, and one that lags the library is worse than none.
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
exec cbindgen \
    --config "$root/mx-remote-ffi/cbindgen.toml" \
    --crate mx-remote-ffi \
    --output "$root/include/mx_remote.h"
