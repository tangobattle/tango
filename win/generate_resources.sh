#!/bin/bash
set -euo pipefail

# Cleanup.
function cleanup {
    rm -rf Tango.iconset
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
