#!/usr/bin/env bash
set -euo pipefail

cleanup() {
    rm -rf tango_linux_workdir
}
trap cleanup EXIT
cleanup

# Grab a copy of appimagetool.
wget https://github.com/AppImage/AppImageKit/releases/download/continuous/appimagetool-x86_64.AppImage
chmod a+x appimagetool-x86_64.AppImage

# Build Linux binaries.
target_arch="x86_64"
cargo build --bin tango --features gamesupport-all --target="${target_arch}-unknown-linux-gnu" --profile release-dist

# Assemble AppImage stuff.
mkdir -p "tango_linux_workdir/${target_arch}/bin"
cp tango/src/icon.png tango_linux_workdir/tango.png
cp linux/AppRun tango_linux_workdir/AppRun
cp linux/tango.desktop tango_linux_workdir/tango.desktop
cp "target/${target_arch}-unknown-linux-gnu/release-dist/tango" "tango_linux_workdir/${target_arch}/bin/tango"

# Split the DWARF (release-dist builds with debug = 1) out of the
# shipped binary into a sidecar release asset — the ELF analogue of
# the Windows .pdb — so users don't download line tables with every
# update but crash-log module+offset frames still resolve offline.
mkdir -p dist
objcopy --only-keep-debug "tango_linux_workdir/${target_arch}/bin/tango" "dist/tango-${target_arch}-linux.debug"
objcopy --strip-debug --add-gnu-debuglink="dist/tango-${target_arch}-linux.debug" "tango_linux_workdir/${target_arch}/bin/tango"

# Bundle ffmpeg.
ffmpeg_version="8.1.2"

wget "https://github.com/tangobattle/ffmpeg-build/releases/download/ffmpeg-${ffmpeg_version}/ffmpeg-linux-x86_64" -O "tango_linux_workdir/${target_arch}/bin/ffmpeg"
chmod a+x "tango_linux_workdir/${target_arch}/bin/ffmpeg"

# Build AppImage. appimagetool defaults to gzip squashfs; xz roughly
# halves the download for a modest first-access decompression cost
# (blocks land in the page cache after that), and the AppImageKit
# runtime decompresses it natively.
./appimagetool-x86_64.AppImage --comp xz tango_linux_workdir "dist/tango-${target_arch}-linux.AppImage"
rm -rf tango_linux_workdir
