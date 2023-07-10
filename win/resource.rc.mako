<%
import os
import semver
import toml

with open(os.path.join(os.path.dirname(__file__), "..", "tango", "Cargo.toml")) as f:
    cargo_toml = toml.load(f)


version = semver.Version.parse(cargo_toml["package"]["version"])

%>#include "winver.h"

1 ICON "icon.ico"

VS_VERSION_INFO VERSIONINFO
    FILEVERSION    ${version.major},${version.minor},${version.patch},0
    PRODUCTVERSION ${version.major},${version.minor},${version.patch},0
BEGIN
    BLOCK "StringFileInfo"
    BEGIN
        BLOCK "040904b0"
        BEGIN
            VALUE "FileDescription", "Tango\\0"
            VALUE "ProductVersion", "${version.major}.${version.minor}.${version.patch}.0\\0"
            VALUE "FileVersion", "${version.major}.${version.minor}.${version.patch}.0\\0"
            VALUE "OriginalFilename", "tango.exe\\0"
            VALUE "Info", "https://tango.n1gp.net\\0"
        END
    END
    BLOCK "VarFileInfo"
    BEGIN
        VALUE "Translation", 0x0, 1200
    END
END
