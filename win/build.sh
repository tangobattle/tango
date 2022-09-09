#!/bin/bash
set -euo pipefail

# Cleanup.
function cleanup {
    rm -rf Tango.iconset tango_wix_workdir
}
trap cleanup EXIT
cleanup

# Generate an appropriate .rc file.
"$(dirname "${BASH_SOURCE[0]}")/generate_rc.py" >tango/resource.rc

# Create icon.
mkdir Tango.iconset
convert -resize 16x16 tango/src/icon.png -depth 32 Tango.iconset/icon_16x16.png
convert -resize 32x32 tango/src/icon.png -depth 32 Tango.iconset/icon_32x32.png
convert -resize 128x128 tango/src/icon.png -depth 32 Tango.iconset/icon_128x128.png
convert -resize 256x256 tango/src/icon.png -depth 32 Tango.iconset/icon_256x256.png
convert Tango.iconset/*.png tango/icon.ico
rm -rf Tango.iconset

# Build Windows binaries.
cargo build --release --target x86_64-pc-windows-gnu

# Build MSI.
mkdir tango_wix_workdir
"$(dirname "${BASH_SOURCE[0]}")/generate_wxs.py" >tango_wix_workdir/installer.wxs
pushd tango_wix_workdir

cp ../tango/icon.ico .
cp ../target/x86_64-pc-windows-gnu/release/tango.exe .

mingw_sysroot="$(x86_64-w64-mingw32-g++ -print-sysroot)"
cp "${mingw_sysroot}/x86_64-w64-mingw32/"{bin/libwinpthread-1.dll,lib/{libgcc_s_seh-1.dll,libstdc++-6.dll}} .

ANGLE_ZIP_URL="https://github.com/google/gfbuild-angle/releases/download/github%2Fgoogle%2Fgfbuild-angle%2Ff810e998993290f049bbdad4fae975e4867100ad/gfbuild-angle-f810e998993290f049bbdad4fae975e4867100ad-Windows_x64_Release.zip"
mkdir angle
wget -O - "${ANGLE_ZIP_URL}" | bsdtar -Cangle -xvf- lib/{libEGL.dll,libGLESv2.dll}
cp angle/lib/{libEGL.dll,libGLESv2.dll} .

FFMPEG_ZIP_URL="https://github.com/BtbN/FFmpeg-Builds/releases/download/latest/ffmpeg-master-latest-win64-gpl.zip"
mkdir ffmpeg
wget -O - "${FFMPEG_ZIP_URL}" | bsdtar -Cffmpeg -xvf- ffmpeg-master-latest-win64-gpl/bin/ffmpeg.exe
cp ffmpeg/ffmpeg-master-latest-win64-gpl/bin/ffmpeg.exe .

wixl installer.wxs
popd
mv tango_wix_workdir/installer.msi "dist/tango-$(python3 -c "import toml; print(toml.load(open('tango/Cargo.toml'))['package']['version'])", end='')-x86_64-windows.msi"
rm -rf tango_wix_workdir
