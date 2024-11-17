#!/bin/bash
set -euo pipefail

# Cleanup.
function cleanup {
    rm -rf Tango.iconset tango_win_workdir
}
trap cleanup EXIT
cleanup

# Generate an appropriate .rc file.
tools/mako_generate.py "$(dirname "${BASH_SOURCE[0]}")/resource.rc.mako" >tango/resource.rc

# Create icon.
mkdir Tango.iconset
convert -resize 16x16 tango/src/icon.png -depth 32 Tango.iconset/icon_16x16.png
convert -resize 32x32 tango/src/icon.png -depth 32 Tango.iconset/icon_32x32.png
convert -resize 128x128 tango/src/icon.png -depth 32 Tango.iconset/icon_128x128.png
convert -resize 256x256 tango/src/icon.png -depth 32 Tango.iconset/icon_256x256.png
convert Tango.iconset/*.png tango/icon.ico
rm -rf Tango.iconset

# Build Windows binaries.
cargo build --bin tango --release --target i686-pc-windows-gnu

# Build installer.
mkdir tango_win_workdir
tools/mako_generate.py "$(dirname "${BASH_SOURCE[0]}")/installer_i686.nsi.mako" >tango_win_workdir/installer.nsi

pushd tango_win_workdir

cp ../tango/icon.ico .
cp ../target/i686-pc-windows-gnu/release/tango.exe .
cp {/usr/i686-w64-mingw32/lib/libwinpthread-1.dll,/usr/lib/gcc/i686-w64-mingw32/10-posix/{libgcc_s_dw2-1.dll,libstdc++-6.dll}} .

# Chrome 109.0.5414.120 installer is used here, since it's the last version that supports Windows 7 and 8
chrome_109_url="https://dl.google.com/release2/chrome/acihtkcueyye3ymoj2afvv7ulzxa_109.0.5414.120/109.0.5414.120_chrome_installer.exe"
wget "${chrome_109_url}"
7z x 109.0.5414.120_chrome_installer.exe
7z e -aoa chrome.7z {Chrome-bin/109.0.5414.120/libEGL.dll,Chrome-bin/109.0.5414.120/libGLESv2.dll}

ffmpeg_version="6.0"
wget -O ffmpeg.exe "https://github.com/eugeneware/ffmpeg-static/releases/download/b${ffmpeg_version}/ffmpeg-win32-ia32"

makensis installer.nsi
popd

mkdir -p dist
mv tango_win_workdir/installer.exe "dist/tango-i686-windows.exe"
rm -rf tango_win_workdir
