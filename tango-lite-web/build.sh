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

# A shared-memory module, because the DS games' engine is one: melonDS
# and its libc++ come out of the *threads* wasi sysroot, and wasm-ld
# accepts them only into a link where every object carries the atomics
# feature — the Rust half included, which is what the target-feature
# flags and the std rebuild (-Zbuild-std, hence nightly) are for.
# --import-memory because wasm-bindgen's threads transform insists the
# memory arrive from JS; the TLS exports are what that transform calls
# to set a spawned thread's TLS block up, and __heap_base/__data_end
# are where it injects the thread-id slot — older rustc exported those
# two by default, newer nightlies don't. A page instantiating this
# must be served cross-origin-isolated (COOP/COEP — see serve.py and
# the _headers file), which is what makes a *shared* WebAssembly.Memory
# constructible at all.
export RUSTFLAGS="${RUSTFLAGS:-} \
    -Ctarget-feature=+atomics,+bulk-memory,+mutable-globals \
    -Clink-arg=--shared-memory \
    -Clink-arg=--import-memory \
    -Clink-arg=--max-memory=2147483648 \
    -Clink-arg=-zstack-size=8388608 \
    -Clink-arg=--export=__wasm_init_tls \
    -Clink-arg=--export=__tls_size \
    -Clink-arg=--export=__tls_align \
    -Clink-arg=--export=__tls_base \
    -Clink-arg=--export=__heap_base \
    -Clink-arg=--export=__data_end"

# The one cc-built C dependency outside the emulator sys crates (zstd,
# via tango-patch) has to carry the atomics feature too, or wasm-ld
# refuses it into the shared-memory link. The sys crates handle their
# own flags; this reaches the ones that just use `cc` with defaults.
export CFLAGS_wasm32_unknown_unknown="${CFLAGS_wasm32_unknown_unknown:-} -matomics -mbulk-memory -mmutable-globals"

cargo +nightly build \
    --package tango-lite-web \
    --target wasm32-unknown-unknown \
    --profile "$profile" \
    --features "$features" \
    -Zbuild-std=std,panic_abort

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
# the unoptimised module is ~14MB, mostly mgba. Needs binaryen 118+:
# rustc emits reference-types instructions by default since 1.82, and
# only binaryen 118's standardized-features default covers them —
# older wasm-opt defaults to MVP and dies parsing the module
# ("invalid code after misc prefix"), which is also why apt's
# binaryen 117 won't do.
if command -v wasm-opt >/dev/null 2>&1; then
    # The threads flags mirror the target features above: the module
    # uses shared memory and atomics, and wasm-opt refuses (or worse,
    # mangles) what it hasn't been told to expect.
    wasm-opt -Oz \
        --enable-threads --enable-bulk-memory --enable-mutable-globals \
        -o "$out/tango-lite-web_bg.wasm" "$out/tango-lite-web_bg.wasm"
else
    echo "note: wasm-opt not found; shipping the unoptimised module" >&2
fi

# The app shell, its icons, and the two files that make it
# installable. `sw.js` has to sit at the root of what it serves —
# a worker's scope can't reach above its own directory. `_headers` is
# for Cloudflare Pages: the COOP/COEP pair that makes the page
# cross-origin-isolated, without which the shared wasm memory can't be
# constructed (serve.py sends the same two locally).
cp index.html style.css "$out/"
cp assets/favicon.svg assets/apple-touch-icon.png assets/icon-192.png \
    assets/icon-512.png assets/icon-512-maskable.png \
    assets/manifest.webmanifest assets/sw.js assets/_headers "$out/"

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
