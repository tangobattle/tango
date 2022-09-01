#!/bin/bash
set -euo pipefail

# Cleanup.
function cleanup {
    rm -rf Tango.iconset Tango.app
}
trap cleanup EXIT
cleanup

# Create directory structure.
mkdir Tango.app{,/Contents{,/{MacOS,Resources}}}

# Generate an appropriate Info.plist.
"$(dirname "${BASH_SOURCE[0]}")/macos/generate_info_plist.py" >Tango.app/Contents/Info.plist

# Create icon.
mkdir Tango.iconset
sips -z 16 16 tango/src/icon.png --out Tango.iconset/icon_16x16.png
sips -z 32 32 tango/src/icon.png --out Tango.iconset/icon_16x16@2x.png
sips -z 32 32 tango/src/icon.png --out Tango.iconset/icon_32x32.png
sips -z 64 64 tango/src/icon.png --out Tango.iconset/icon_32x32@2x.png
sips -z 128 128 tango/src/icon.png --out Tango.iconset/icon_128x128.png
sips -z 256 256 tango/src/icon.png --out Tango.iconset/icon_128x128@2x.png
sips -z 256 256 tango/src/icon.png --out Tango.iconset/icon_256x256.png
sips -z 512 512 tango/src/icon.png --out Tango.iconset/icon_256x256@2x.png
sips -z 512 512 tango/src/icon.png --out Tango.iconset/icon_512x512.png
sips -z 1024 1024 tango/src/icon.png --out Tango.iconset/icon_512x512@2x.png
iconutil -c icns Tango.iconset --output Tango.app/Contents/Resources/Tango.icns
rm -rf Tango.iconset

# Build macOS binaries.
cargo build --bin tango --target=aarch64-apple-darwin --release
cargo build --bin tango --target=x86_64-apple-darwin --release
lipo -create target/{aarch64-apple-darwin,x86_64-apple-darwin}/release/tango -output Tango.app/Contents/MacOS/tango

# Build zip.
zip -r "target/tango-$(python3 -c "import toml; print(toml.load(open('tango/Cargo.toml'))['package']['version'])", end='')-macos.zip" Tango.app
