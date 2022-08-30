#!/bin/bash
set -euo pipefail

# Cleanup.
rm -rf Tango.app

# Create directory structure.
mkdir Tango.app{,/Contents{,/{MacOS,Resources}}}

# Generate an appropriate Info.plist.
"$(dirname "${BASH_SOURCE[0]}")/generate_info_plist.py" >Tango.app/Contents/Info.plist

# Build macOS binaries.
cargo build --bin tango --target=aarch64-apple-darwin --release
cargo build --bin tango --target=x86_64-apple-darwin --release
lipo -create target/{aarch64-apple-darwin,x86_64-apple-darwin}/release/tango -output Tango.app/Contents/MacOS/tango
