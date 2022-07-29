#!/bin/bash
set -euo pipefail
set -x

tempdir="$(mktemp -d)"
trap 'rm -rf -- "$tempdir"' EXIT

curl -L -o "${tempdir}/ffmpeg.7z" "https://www.gyan.dev/ffmpeg/builds/ffmpeg-git-essentials.7z"
mkdir "${tempdir}/ffmpeg"
pushd "${tempdir}/ffmpeg"
7z x ../ffmpeg.7z
popd
cp "${tempdir}"/ffmpeg/ffmpeg-*-essentials_build/bin/ffmpeg.exe bin

curl -L -o "${tempdir}/drmingw.7z" "https://github.com/tangobattle/drmingw/releases/download/0.9.5-w/drmingw-0.9.5-win64.7z"
mkdir "${tempdir}/drmingw"
pushd "${tempdir}/drmingw"
7z x ../drmingw.7z
popd
cp "${tempdir}"/drmingw/drmingw-*-win64/bin/* bin
