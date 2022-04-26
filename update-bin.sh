#!/bin/bash
set -euo pipefail
set -x
TANGO_CORE_VERSION="3.0.0-alpha.1"
TANGO_CORE_PLATFORM="x86_64-pc-windows-gnu"
curl -L -o tango-core.zip "https://github.com/tangobattle/tango-core/releases/download/v${TANGO_CORE_VERSION}/tango-core-v${TANGO_CORE_VERSION}-${TANGO_CORE_PLATFORM}.zip"
rm -rf bin
mkdir bin || true
cd bin
unzip ../tango-core.zip
