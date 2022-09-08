#!/usr/bin/env python3
import os
import semver
import toml

with open(os.path.join(os.path.dirname(__file__), "..", "tango", "Cargo.toml")) as f:
    cargo_toml = toml.load(f)


version = semver.Version.parse(cargo_toml["package"]["version"])

print(
    f"""\
<?xml version="1.0" encoding="UTF-8"?>
<Wix xmlns="http://schemas.microsoft.com/wix/2006/wi">
    <Product
        Id="*"
        Language="1033"
        Manufacturer="Tango"
        Name="Tango"
        Version="{version.major}.{version.minor}.{version.patch}.0">
        <Package Id="B2BBB3AD-3965-485B-9634-33323BCAA49F" InstallerVersion="200" />
        <Media Id="1" Cabinet="product.cab" EmbedCab="yes" />

        <Directory Id="TARGETDIR" Name="SourceDir">
            <Directory Id="DesktopFolder" Name="Desktop">
                <Component Id="ApplicationShortcutDesktop" Guid="*">
                    <Shortcut
                        Id="ApplicationDesktopShortcut"
                        Name="Tango"
                        Target="[INSTALLFOLDER]Tango.exe"
                        WorkingDirectory="INSTALLFOLDER" />
                    <RemoveFolder Id="DesktopFolder" On="uninstall" />
                    <RegistryValue
                        Root="HKCU"
                        Key="Software\Tango\Tango"
                        Name="installed"
                        Type="integer"
                        Value="1"
                        KeyPath="yes" />
                </Component>
            </Directory>

            <Directory Id="ProgramFiles64Folder">
                <Directory Id="INSTALLFOLDER" Name="Tango">
                    <Component Id="tango.exe" Guid="*">
                        <File Source="tango.exe" KeyPath="yes" />
                    </Component>
                    <Component Id="libstdc++-6.dll" Guid="*">
                        <File Source="libstdc++-6.dll" KeyPath="yes" />
                    </Component>
                    <Component Id="libEGL.dll" Guid="*">
                        <File Source="libEGL.dll" KeyPath="yes" />
                    </Component>
                    <Component Id="libGLESv2.dll" Guid="*">
                        <File Source="libGLESv2.dll" KeyPath="yes" />
                    </Component>
                    <Component Id="libgcc_s_seh-1.dll" Guid="*">
                        <File Source="libgcc_s_seh-1.dll" KeyPath="yes" />
                    </Component>
                    <Component Id="libwinpthread-1.dll" Guid="*">
                        <File Source="libwinpthread-1.dll" KeyPath="yes" />
                    </Component>
                    <Component Id="ffmpeg.exe" Guid="*">
                        <File Source="ffmpeg.exe" KeyPath="yes" />
                    </Component>
                </Directory>
            </Directory>
        </Directory>

        <Feature Id="Tango" Level="1">
            <ComponentRef Id="tango.exe" />
            <ComponentRef Id="libstdc++-6.dll" />
            <ComponentRef Id="libEGL.dll" />
            <ComponentRef Id="libGLESv2.dll" />
            <ComponentRef Id="libgcc_s_seh-1.dll" />
            <ComponentRef Id="libwinpthread-1.dll" />
            <ComponentRef Id="ffmpeg.exe" />
            <ComponentRef Id="ApplicationShortcutDesktop" />
        </Feature>
    </Product>
</Wix>
"""
)
