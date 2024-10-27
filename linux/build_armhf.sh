#!/bin/bash
set -euo pipefail

# Cleanup.
function cleanup {
	rm -rf tango_linux_workdir
}
trap cleanup EXIT
cleanup

# Grab a copy of appimagetool.
wget https://github.com/AppImage/AppImageKit/releases/download/continuous/appimagetool-armhf.AppImage
chmod a+x appimagetool-armhf.AppImage

# Build Linux binaries.
target_arch="armv7"
cargo build --bin tango --target="${target_arch}-unknown-linux-gnueabihf" --no-default-features --features=sdl2-audio,wgpu,cpal --release

# Assemble AppImage stuff.
mkdir -p "tango_linux_workdir/${target_arch}/bin"
cp tango/src/icon.png tango_linux_workdir/tango.png
cp linux/AppRun tango_linux_workdir/AppRun
cp linux/tango.desktop tango_linux_workdir/tango.desktop
cp "target/${target_arch}-unknown-linux-gnu/release/tango" "tango_linux_workdir/${target_arch}/bin/tango"

# Bundle ffmpeg.
ffmpeg_version="6.0"

wget "https://github.com/eugeneware/ffmpeg-static/releases/download/b${ffmpeg_version}/ffmpeg-linux-arm" -O "tango_linux_workdir/${target_arch}/bin/ffmpeg"
chmod a+x "tango_linux_workdir/${target_arch}/bin/ffmpeg"

ln -s 

# Build AppImage.
mkdir -p dist
# Workaround for running 32bit OS on 64bit hardware
cd tango_linux_workdir
ln -s armv7 aarch64
ln -s armv7 armv7l
ln -s armv7 armhf
$SHELL
./appimagetool-armhf.AppImage tango_linux_workdir "dist/tango-${target_arch}-linux.AppImage"
rm -rf tango_linux_workdir
