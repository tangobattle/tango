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

# The app shell, its icons, and the two files that make it
# installable. `sw.js` has to sit at the root of what it serves —
# a worker's scope can't reach above its own directory.
cp index.html style.css "$out/"
cp assets/favicon.svg assets/apple-touch-icon.png assets/icon-192.png \
    assets/icon-512.png assets/icon-512-maskable.png \
    assets/manifest.webmanifest assets/sw.js "$out/"

# Fill in the service worker's shell list and cache version from what
# is actually in dist -- see the note in assets/sw.js.
python3 - "$out" <<'STAMP'
import hashlib, json, pathlib, sys

dist = pathlib.Path(sys.argv[1])
worker = dist / "sw.js"
files = sorted(p for p in dist.rglob("*") if p.is_file() and p != worker)
shell = ["./"] + [f"./{p.relative_to(dist).as_posix()}" for p in files]
digest = hashlib.sha256()
for p in files:
    digest.update(p.relative_to(dist).as_posix().encode())
    digest.update(p.read_bytes())
worker.write_text(
    worker.read_text()
    .replace("__BUILD_ID__", digest.hexdigest()[:12])
    .replace("__SHELL__", json.dumps(shell))
)
print(f"service worker: {len(shell)} shell entries, build {digest.hexdigest()[:12]}")
STAMP

echo "built $out"
echo "serve it with any static file server, e.g.:  python3 -m http.server -d $out 8080"
