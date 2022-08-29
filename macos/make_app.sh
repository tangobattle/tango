#!/bin/bash
set -euo pipefail
rm -rf Tango.app
mkdir Tango.app{,/Contents{,/{MacOS,Resources}}}
./generate_info_plist.py >Tango.app/Contents/Info.plist
