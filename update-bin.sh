#!/bin/bash
set -euo pipefail
set -x

TANGO_CORE_VERSION="3.0.0-alpha.3"
TANGO_CORE_PLATFORM="x86_64-pc-windows-gnu"
FFMPEG_VERSION="2022-04-21-git-83e1a1de88"

tempdir="$(mktemp -d)"
trap 'rm -rf -- "$tempdir"' EXIT

curl -L -o "${tempdir}/tango-core.zip" "https://github.com/tangobattle/tango-core/releases/download/v${TANGO_CORE_VERSION}/tango-core-v${TANGO_CORE_VERSION}-${TANGO_CORE_PLATFORM}.zip"
rm -rf bin
mkdir bin || true
pushd bin
unzip "${tempdir}/tango-core.zip"
popd

curl -L -o "${tempdir}/ffmpeg.7z" https://www.gyan.dev/ffmpeg/builds/packages/ffmpeg-${FFMPEG_VERSION}-essentials_build.7z
mkdir "${tempdir}/ffmpeg"
pushd "${tempdir}/ffmpeg"
7z x ../ffmpeg.7z
popd

cp "${tempdir}/ffmpeg/ffmpeg-${FFMPEG_VERSION}-essentials_build/bin/ffmpeg.exe" bin
