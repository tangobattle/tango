#!/bin/bash
set -euo pipefail
set -x

# Can't download from johnvansickle.com since GitHub Actions is blocked (?)
# Use the mirror provided by npm package ffmpeg-static
mkdir -p bin
wget https://github.com/eugeneware/ffmpeg-static/releases/download/b5.0.1/linux-x64 -O bin/ffmpeg
chmod 755 bin/ffmpeg
