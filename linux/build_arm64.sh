#!/bin/bash
set -euo pipefail

# Cleanup.
function cleanup {
	rm -rf tango_linux_workdir
}
trap cleanup EXIT
cleanup

# Grab a copy of appimagetool.
wget -c https://github.com/AppImage/AppImageKit/releases/download/continuous/appimagetool-aarch64.AppImage
chmod a+x appimagetool-aarch64.AppImage

# Build Linux binaries.
target_arch="aarch64"
cargo build --bin tango --target="${target_arch}-unknown-linux-gnu" --no-default-features --features=sdl2-audio,wgpu,cpal --release

# Assemble AppImage stuff.
mkdir -p "tango_linux_workdir/${target_arch}/bin"
cp tango/src/icon.png tango_linux_workdir/tango.png
cp linux/AppRun tango_linux_workdir/AppRun
cp linux/tango.desktop tango_linux_workdir/tango.desktop
cp "target/${target_arch}-unknown-linux-gnu/release/tango" "tango_linux_workdir/${target_arch}/bin/tango"

# Bundle ffmpeg.
ffmpeg_version="6.0"

wget -c "https://github.com/eugeneware/ffmpeg-static/releases/download/b${ffmpeg_version}/ffmpeg-linux-arm64" -O "tango_linux_workdir/${target_arch}/bin/ffmpeg"
chmod a+x "tango_linux_workdir/${target_arch}/bin/ffmpeg"

# Build AppImage.
mkdir -p dist
./appimagetool-aarch64.AppImage tango_linux_workdir "dist/tango-${target_arch}-linux.AppImage"
rm -rf tango_linux_workdir
