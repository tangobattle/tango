#!/bin/bash
set -euo pipefail
set -x

tempdir="$(mktemp -d)"
trap 'rm -rf -- "$tempdir"' EXIT

curl -L -o "${tempdir}/sdl2.zip" "https://github.com/libsdl-org/SDL/releases/download/release-2.0.22/SDL2-2.0.22-win32-x64.zip"
mkdir "${tempdir}/sdl2"
pushd "${tempdir}/sdl2"
unzip ../sdl2.zip
popd
cp "${tempdir}"/sdl2/SDL2.dll bin

curl -L -o "${tempdir}/sdl2_ttf.zip" "https://github.com/libsdl-org/SDL_ttf/releases/download/release-2.0.18/SDL2_ttf-2.0.18-win32-x64.zip"
mkdir "${tempdir}/sdl2_ttf"
pushd "${tempdir}/sdl2_ttf"
unzip ../sdl2_ttf.zip
popd
cp "${tempdir}"/sdl2_ttf/SDL2_ttf.dll bin

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
