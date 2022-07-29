#!/bin/bash
set -euo pipefail
set -x

tempdir="$(mktemp -d)"
trap 'rm -rf -- "$tempdir"' EXIT

curl -L -o "${tempdir}/ffmpeg.7z" "https://evermeet.cx/ffmpeg/get"
mkdir "${tempdir}/ffmpeg"
pushd "${tempdir}/ffmpeg"
7z x ../ffmpeg.7z
popd
cp "${tempdir}"/ffmpeg/ffmpeg bin
