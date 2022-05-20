#!/bin/bash
set -euo pipefail
set -x

tempdir="$(mktemp -d)"
trap 'rm -rf -- "$tempdir"' EXIT

mkdir -p external/sdl2 || true

curl -L -o "${tempdir}/sdl2.zip" "https://github.com/libsdl-org/SDL/releases/download/release-2.0.22/SDL2-2.0.22-win32-x64.zip"
mkdir "${tempdir}/sdl2"
pushd "${tempdir}/sdl2"
unzip ../sdl2.zip
popd
cp "${tempdir}"/sdl2/SDL2.dll external/sdl2

curl -L -o "${tempdir}/sdl2_ttf.zip" "https://github.com/libsdl-org/SDL_ttf/releases/download/release-2.0.18/SDL2_ttf-2.0.18-win32-x64.zip"
mkdir "${tempdir}/sdl2_ttf"
pushd "${tempdir}/sdl2_ttf"
unzip ../sdl2_ttf.zip
popd
cp "${tempdir}"/sdl2_ttf/SDL2_ttf.dll external/sdl2
