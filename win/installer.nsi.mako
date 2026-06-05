<%
import os
import semver
import toml

with open(os.path.join(os.path.dirname(__file__), "..", "tango", "Cargo.toml")) as f:
    cargo_toml = toml.load(f)


version = semver.Version.parse(cargo_toml["package"]["version"])

%>!define NAME "Tango"
!define REGPATH_UNINSTSUBKEY "Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\<%text>$</%text>{NAME}"

LoadLanguageFile "<%text>$</%text>{NSISDIR}\Contrib\Language files\English.nlf"
LoadLanguageFile "<%text>$</%text>{NSISDIR}\Contrib\Language files\Japanese.nlf"
LoadLanguageFile "<%text>$</%text>{NSISDIR}\Contrib\Language files\TradChinese.nlf"
LoadLanguageFile "<%text>$</%text>{NSISDIR}\Contrib\Language files\SimpChinese.nlf"

Name "<%text>$</%text>{NAME}"
Icon "icon.ico"
OutFile "installer.exe"

VIProductVersion "${version.major}.${version.minor}.${version.patch}.0"
VIAddVersionKey "ProductName" "<%text>$</%text>{NAME}"
VIAddVersionKey "FileVersion" "${version.major}.${version.minor}.${version.patch}.0"
VIAddVersionKey "FileDescription" "Tango Installer"
VIAddVersionKey "LegalCopyright" "© Copyright The Tango Developers"

SetCompressor /solid /final zlib
Unicode true
RequestExecutionLevel user
; No installer UI at all. First-install and auto-update (updater.rs
; spawns this exe detached, with no /S flag) both apply with zero
; visible window; the app itself is launched by .onInstSuccess, so the
; user still gets feedback. Manual uninstall stays interactive so the
; "delete config?" prompt is preserved.
SilentInstall silent
AutoCloseWindow true
ShowInstDetails nevershow
ShowUninstDetails nevershow
BrandingText " "

InstallDir ""
InstallDirRegKey HKCU "<%text>$</%text>{REGPATH_UNINSTSUBKEY}" "UninstallString"

!include LogicLib.nsh
!include WinCore.nsh
!include FileFunc.nsh

Function .onInit
    SetShellVarContext Current

    <%text>$</%text>{If} $INSTDIR == ""
        GetKnownFolderPath $INSTDIR <%text>$</%text>{FOLDERID_UserProgramFiles}
        StrCmp $INSTDIR "" 0 +2
        StrCpy $INSTDIR "$LocalAppData\\Programs"
        StrCpy $INSTDIR "$INSTDIR\\$(^Name)"
    <%text>$</%text>{EndIf}

    ; Clean up a prior install before laying down new files, so any
    ; component dropped between versions doesn't linger. Run the old
    ; uninstaller silently and synchronously: /S suppresses its UI (and
    ; the "delete config?" prompt, so settings survive an upgrade), and
    ; _?=$INSTDIR makes it run in place so ExecWait actually blocks
    ; until it finishes instead of returning while a self-copy in $TEMP
    ; races our file writes. Skipped on a fresh install.
    IfFileExists "$INSTDIR\\uninstall.exe" 0 +2
        ExecWait '"$INSTDIR\\uninstall.exe" /S _?=$INSTDIR'
FunctionEnd

Function un.onInit
    SetShellVarContext Current
FunctionEnd

LangString MessageDeleteConfig <%text>$</%text>{LANG_ENGLISH} "Would you also like to delete configuration settings?"
LangString MessageDeleteConfig <%text>$</%text>{LANG_JAPANESE} "コンフィギュレーション設定も削除しますか？"
LangString MessageDeleteConfig <%text>$</%text>{LANG_SIMPCHINESE} "您是否也想删除配置设置？"
LangString MessageDeleteConfig <%text>$</%text>{LANG_TRADCHINESE} "您是否也想刪除配置設置？"

Function un.onGUIInit
    MessageBox MB_YESNO "$(MessageDeleteConfig)" /SD IDNO IDYES true IDNO false
    true:
        Delete "$APPDATA\\Tango\\config\\config.json"
    false:
FunctionEnd

Function .onInstSuccess
    Exec "$INSTDIR\\tango.exe"
FunctionEnd

Section
    SetDetailsPrint none
    SetOutPath $INSTDIR
    File "libEGL.dll"
    File "libGLESv2.dll"
    File "ffmpeg.exe"
    File "tango.exe"
    WriteUninstaller "$INSTDIR\\uninstall.exe"
    WriteRegStr HKCU "<%text>$</%text>{REGPATH_UNINSTSUBKEY}" "DisplayName" "<%text>$</%text>{NAME}"
    WriteRegStr HKCU "<%text>$</%text>{REGPATH_UNINSTSUBKEY}" "DisplayIcon" "$INSTDIR\\tango.exe,0"
    WriteRegStr HKCU "<%text>$</%text>{REGPATH_UNINSTSUBKEY}" "Publisher" "The Tango Developers"
    WriteRegStr HKCU "<%text>$</%text>{REGPATH_UNINSTSUBKEY}" "InstallLocation" "$INSTDIR"

    IntFmt $0 "0x%08X" "{version.major}"
    WriteRegDWORD HKCU "<%text>$</%text>{REGPATH_UNINSTSUBKEY}" "VersionMajor" "$0"

    IntFmt $0 "0x%08X" "{version.minor}"
    WriteRegDWORD HKCU "<%text>$</%text>{REGPATH_UNINSTSUBKEY}" "VersionMinor" "$0"

    WriteRegStr HKCU "<%text>$</%text>{REGPATH_UNINSTSUBKEY}" "DisplayVersion" "{version}"
    WriteRegStr HKCU "<%text>$</%text>{REGPATH_UNINSTSUBKEY}" "UninstallString" '"$INSTDIR\\uninstall.exe"'
    WriteRegStr HKCU "<%text>$</%text>{REGPATH_UNINSTSUBKEY}" "QuietUninstallString" '"$INSTDIR\\uninstall.exe" /S'

    <%text>$</%text>{GetSize} "$INSTDIR" "/S=0K" $0 $1 $2
    IntFmt $0 "0x%08X" $0
    WriteRegDWORD HKCU "<%text>$</%text>{REGPATH_UNINSTSUBKEY}" "EstimatedSize" "$0"

    WriteRegDWORD HKCU "<%text>$</%text>{REGPATH_UNINSTSUBKEY}" "NoModify" 1
    WriteRegDWORD HKCU "<%text>$</%text>{REGPATH_UNINSTSUBKEY}" "NoRepair" 1
    CreateShortcut "$SMPROGRAMS\\Tango.lnk" "$INSTDIR\\tango.exe"
    CreateShortcut "$DESKTOP\\Tango.lnk" "$INSTDIR\\tango.exe"
SectionEnd

Section "uninstall"
    SetDetailsPrint none
    Delete "$DESKTOP\\Tango.lnk"
    Delete "$SMPROGRAMS\\Tango.lnk"
    Delete "$INSTDIR\\libEGL.dll"
    Delete "$INSTDIR\\libGLESv2.dll"
    Delete "$INSTDIR\\ffmpeg.exe"
    Delete "$INSTDIR\\tango.exe"
    Delete "$INSTDIR\\uninstall.exe"
    RMDir $INSTDIR
    DeleteRegKey HKCU "<%text>$</%text>{REGPATH_UNINSTSUBKEY}"
SectionEnd
