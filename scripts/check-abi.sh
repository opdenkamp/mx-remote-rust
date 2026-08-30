#!/bin/sh
# Fails if the archive exports an entry point the header does not declare.
#
# cbindgen parses the Rust source and expands nothing, so an entry point that
# comes out of a macro is in the library and absent from the header. Callers
# then link against a symbol the header never declared, and every other check
# in the build passes.
#
# Usage: check-abi.sh [path to libmx_remote_ffi.a]

set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
archive=${1:-$root/target/debug/libmx_remote_ffi.a}
header=$root/include/mx_remote.h

if [ ! -f "$archive" ]; then
    echo "$archive is missing: run cargo build -p mx-remote-ffi first" >&2
    exit 1
fi

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

# Exported text symbols, with the leading underscore some platforms add. An
# archive this size carries object files with no symbols at all, and nm says
# so on stderr for each one.
nm -g --defined-only "$archive" 2>/dev/null |
    sed -n 's/^.* [TtWw] _\{0,1\}\(mxr_[A-Za-z0-9_]*\)$/\1/p' |
    sort -u >"$work/exported"

# A declaration is a line that starts at column one and ends in an open paren
# or a parameter list; the name is the last identifier before the paren.
sed -n 's/^[A-Za-z_][A-Za-z0-9_ *]*[ *]\(mxr_[a-z0-9_]*\)(.*/\1/p' "$header" |
    sort -u >"$work/declared"

comm -23 "$work/exported" "$work/declared" >"$work/missing"
if [ -s "$work/missing" ]; then
    echo "exported but not declared in include/mx_remote.h:" >&2
    sed 's/^/  /' "$work/missing" >&2
    exit 1
fi

echo "$(wc -l <"$work/exported") entry points, all declared"
