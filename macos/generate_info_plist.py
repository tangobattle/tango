#!/usr/bin/env python3
import toml
import semver

with open("tango/Cargo.toml") as f:
    cargo_toml = toml.load(f)


version = semver.Version.parse(cargo_toml["package"]["version"])

TEMPLATE = f"""\
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
	<dict>
		<key>CFBundleDevelopmentRegion</key>
		<string>en</string>
		<key>CFBundleExecutable</key>
		<string>Tango</string>
		<key>CFBundleIdentifier</key>
		<string>com.tangobattle.Tango</string>
		<key>CFBundleInfoDictionaryVersion</key>
		<string>6.0</string>
		<key>CFBundleName</key>
		<string>Tango</string>
		<key>CFBundleIconFile</key>
		<string>tango.icns</string>
		<key>CFBundlePackageType</key>
		<string>APPL</string>
		<key>CFBundleShortVersionString</key>
		<string>{version.major}.{version.minor}.{version.patch}</string>
		<key>CFBundleSupportedPlatforms</key>
		<array>
			<string>MacOSX</string>
		</array>
		<key>CFBundleVersion</key>
		<string>1</string>
		<key>LSMinimumSystemVersion</key>
		<string>10.15.7</string>
		<key>NSHumanReadableCopyright</key>
		<string></string>
	</dict>
</plist>
"""

print(TEMPLATE)
