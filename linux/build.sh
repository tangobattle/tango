#!/bin/bash
set -euo pipefail

# Create a basic retry wrapper
# Use this for anything that makes external network requests
# This way a single erroneuous network failure doesn't fail the whole build
retry() {
	for i in {1..10}; do
		if [ $i -gt 1 ]; then
			echo "Retrying... (attempt $i/10)"
		fi

		$@ || {
			continue
		}

		if [[ "$?" == "0" ]]; then
			return 0
		fi

		if [ $i -eq 10 ]; then
			echo "The operation failed after 10 attempts"
			return 1
		fi
	done
}

# Define the arch we're building for
# In the future, to build for aarch64, we can re-run all these commands and just change the arch here
# We would just need to set up an aarch64 sysroot first
export LINUX_ARCH="x86_64"

# The wget-accessible URL for the static ffmpeg binary
# Be sure to change to arm64 in the event we want an aarch64 build
export STATIC_FFMPEG_URL="https://github.com/eugeneware/ffmpeg-static/releases/download/b5.0.1/linux-x64"

# Define a packaging directory
export LINUX_PACKAGING="linux/packaging"

# Define the ./bin directory inside the AppImage for this arch
export APPIMAGE_BIN_DIR="${LINUX_PACKAGING}/${LINUX_ARCH}/bin"

# Build tango
cargo build --bin tango --target="${LINUX_ARCH}-unknown-linux-gnu" --no-default-features --features=wgpu,cpal --release

# Create AppImage packaging directory and a bin folder inside it
mkdir -p "${APPIMAGE_BIN_DIR}"

# Copy tango icon into packaging directory
cp tango/src/icon.png "${LINUX_PACKAGING}/tango.png"

# Copy AppRun into packaging directory and make executable
cp linux/AppRun "${LINUX_PACKAGING}/AppRun"
chmod 755 "${LINUX_PACKAGING}/AppRun"

# Copy .desktop file into packaging directory and make executable
cp linux/tango.desktop "${LINUX_PACKAGING}/tango.desktop"
chmod 755 "${LINUX_PACKAGING}/tango.desktop"

# Download ffmpeg binary and make executable
retry wget "${STATIC_FFMPEG_URL}" -O "${APPIMAGE_BIN_DIR}/ffmpeg"
chmod 755 "${APPIMAGE_BIN_DIR}/ffmpeg"

# Copy tango binary and make executable
cp "target/${LINUX_ARCH}-unknown-linux-gnu/release/tango" "${APPIMAGE_BIN_DIR}/tango"
chmod 755 "${APPIMAGE_BIN_DIR}/tango"

# Download appimagetool
# We don't need to change the arch here for cross compiling, GitHub Actions is x86_64
retry wget https://github.com/AppImage/AppImageKit/releases/download/continuous/appimagetool-x86_64.AppImage
chmod +x appimagetool-x86_64.AppImage

# Package tango into an AppImage
mkdir -p ./dist
./appimagetool-x86_64.AppImage "${LINUX_PACKAGING}" "./dist/tango-${LINUX_ARCH}-linux.AppImage"
