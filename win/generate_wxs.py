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
        UpgradeCode="E8EA30F8-D1B3-4ABC-9048-F0BAD4835738"
        Language="1033"
        Manufacturer="Tango"
        Name="Tango"
        Version="{version.major}.{version.minor}.{version.patch}.0">
        <Package Id="*" InstallerVersion="200" InstallScope="perUser" />
        <Media Id="1" Cabinet="product.cab" EmbedCab="yes" />
        <Icon Id="icon.ico" SourceFile="icon.ico" />
        <Upgrade Id="E8EA30F8-D1B3-4ABC-9048-F0BAD4835738">
            <UpgradeVersion
                OnlyDetect="no"
                Minimum="0.0.0.0" Maximum="999.999.999.999"
                Property="PREVIOUSVERSIONSINSTALLED"
                IncludeMinimum="yes" IncludeMaximum="no" />
        </Upgrade>
        <Property Id="REINSTALLMODE" Value="dmus" />
        <Property Id="PREVIOUSVERSIONSINSTALLED" />
        <Property Id="ARPPRODUCTICON" Value="icon.ico" />
        <Property Id="ALLUSERS" Value="2" />
        <Property Id="MSIINSTALLPERUSER" Value="1" />

        <Directory Id="TARGETDIR" Name="SourceDir">
            <Directory Id="ProgramFiles64Folder">
                <Directory Id="INSTALLFOLDER" Name="Tango">
                    <Component Id="tango.exe" Guid="*">
                        <File Id="tango.exe" Name="tango.exe" Source="tango.exe" KeyPath="yes">
                            <Shortcut
                                Id="DesktopShortcut"
                                Directory="DesktopFolder"
                                Name="Tango"
                                WorkingDirectory="INSTALLFOLDER"
                                Icon="tango.exe"
                                IconIndex="0"
                                Advertise="yes" />
                        </File>
                    </Component>
                    <Component Id="libstdc++-6.dll" Guid="*">
                        <File Id="libstdc++-6.dll" Name="libstdc++-6.dll" Source="libstdc++-6.dll" KeyPath="yes" />
                    </Component>
                    <Component Id="libEGL.dll" Guid="*">
                        <File Id="libEGL.dll" Name="libEGL.dll" Source="libEGL.dll" KeyPath="yes" />
                    </Component>
                    <Component Id="libGLESv2.dll" Guid="*">
                        <File Id="libGLESv2.dll" Name="libGLESv2.dll" Source="libGLESv2.dll" KeyPath="yes" />
                    </Component>
                    <Component Id="libgcc_s_seh-1.dll" Guid="*">
                        <File Id="libgcc_s_seh-1.dll" Name="libgcc_s_seh-1.dll" Source="libgcc_s_seh-1.dll" KeyPath="yes" />
                    </Component>
                    <Component Id="libwinpthread-1.dll" Guid="*">
                        <File Id="libwinpthread-1.dll" Name="libwinpthread-1.dll" Source="libwinpthread-1.dll" KeyPath="yes" />
                    </Component>
                    <Component Id="ffmpeg.exe" Guid="*">
                        <File Id="ffmpeg.exe" Name="ffmpeg.exe" Source="ffmpeg.exe" KeyPath="yes" />
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
        </Feature>

        <InstallExecuteSequence>
            <RemoveExistingProducts After="InstallInitialize" />
            <Custom Action="Launch" After="InstallFinalize">NOT Installed</Custom>
        </InstallExecuteSequence>

        <CustomAction
            Id="Launch"
            ExeCommand="tango.exe"
            FileKey="tango.exe"
            Execute="immediate"
            Impersonate="yes"
            Return="asyncNoWait" />
    </Product>
</Wix>
"""
)
