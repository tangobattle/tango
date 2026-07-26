#!/usr/bin/env bash
# Build the browser app into dist/.
#
# Plain cargo + wasm-bindgen rather than `dx`: the Dioxus CLI's value is
# hot reload and asset processing, and this crate uses neither (the
# stylesheet and the audio worklet are a copied file and an
# include_str!). One fewer tool to have installed.
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$here"

profile="${PROFILE:-release}"
features="${FEATURES:-gamesupport-all}"
out="$here/dist"

cargo build \
    --package tango-lite-web \
    --target wasm32-unknown-unknown \
    --profile "$profile" \
    --features "$features"

# `release` is the only profile whose directory isn't its own name.
profile_dir="$profile"
[ "$profile" = "dev" ] && profile_dir=debug

rm -rf "$out"
wasm-bindgen \
    --target web \
    --no-typescript \
    --out-dir "$out" \
    "$here/../target/wasm32-unknown-unknown/$profile_dir/tango-lite-web.wasm"

# Optional, but it makes a real difference to what a phone downloads —
# the unoptimised module is ~14MB, mostly mgba.
if command -v wasm-opt >/dev/null 2>&1; then
    wasm-opt -Oz --enable-bulk-memory \
        -o "$out/tango-lite-web_bg.wasm" "$out/tango-lite-web_bg.wasm"
else
    echo "note: wasm-opt not found; shipping the unoptimised module" >&2
fi

cp index.html style.css "$out/"

echo "built $out"
echo "serve it with any static file server, e.g.:  python3 -m http.server -d $out 8080"
