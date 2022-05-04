#!/bin/bash
set -euo pipefail
set -x

TANGO_CORE_VERSION="3.0.0"
TANGO_CORE_PLATFORM="x86_64-pc-windows-gnu"

tempdir="$(mktemp -d)"
trap 'rm -rf -- "$tempdir"' EXIT

curl -L -o "${tempdir}/tango-core.zip" "https://github.com/tangobattle/tango-core/releases/download/v${TANGO_CORE_VERSION}/tango-core-v${TANGO_CORE_VERSION}-${TANGO_CORE_PLATFORM}.zip"
rm -rf bin
mkdir bin || true
pushd bin
unzip "${tempdir}/tango-core.zip"
popd

curl -L -o "${tempdir}/ffmpeg.7z" "https://www.gyan.dev/ffmpeg/builds/ffmpeg-git-essentials.7z"
mkdir "${tempdir}/ffmpeg"
pushd "${tempdir}/ffmpeg"
7z x ../ffmpeg.7z
popd
cp "${tempdir}"/ffmpeg/ffmpeg-*-essentials_build/bin/ffmpeg.exe bin

curl -L -o "${tempdir}/drmingw.7z" "https://github.com/jrfonseca/drmingw/releases/download/0.9.5/drmingw-0.9.5-win64.7z"
mkdir "${tempdir}/drmingw"
pushd "${tempdir}/drmingw"
7z x ../drmingw.7z
popd
cp "${tempdir}"/drmingw/drmingw-*-win64/bin/* bin
