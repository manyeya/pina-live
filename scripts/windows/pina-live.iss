; The Windows installer: Pina Live, what it needs beside it, a Start menu entry and (if asked) a
; desktop one, in Pina Live's own look (wizard.bmp and small.bmp, made with crates/app/icon). Per
; user, so it needs no administrator.
;
; Built by .github/workflows/windows.yml:
;   iscc /DVersion=<x.y.z> /DSource=<folder with "Pina Live.exe" and its DLLs> scripts\windows\pina-live.iss

#ifndef Version
  #define Version "0.0.0"
#endif
#ifndef Source
  #define Source "..\..\target\release"
#endif

[Setup]
AppId={{F7B01B57-4907-4564-94EB-BA39AB1809C5}
AppName=Pina Live
AppVersion={#Version}
AppPublisher=Manyeya Khutso Siema
AppCopyright=Copyright (c) 2026 Manyeya Khutso Siema. MIT License.
AppPublisherURL=https://manyeya.github.io/pina-studio/live/
AppSupportURL=https://github.com/manyeya/pina-live/issues
AppUpdatesURL=https://github.com/manyeya/pina-live/releases
VersionInfoCompany=Manyeya Khutso Siema
VersionInfoCopyright=Copyright (c) 2026 Manyeya Khutso Siema
VersionInfoDescription=Pina Live installer
VersionInfoProductName=Pina Live
VersionInfoVersion={#Version}
; The MIT License, shown before installing.
LicenseFile={#Source}\LICENSE.txt
DefaultDirName={autopf}\Pina Live
DefaultGroupName=Pina Live
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
OutputDir=..\..\dist
OutputBaseFilename=PinaLive-{#Version}-windows-x64-setup
SetupIconFile=..\..\crates\app\icon\live.ico
UninstallDisplayIcon={app}\live.ico
WizardStyle=modern
WizardImageFile=wizard.bmp,wizard@2x.bmp
WizardSmallImageFile=small.bmp,small@2x.bmp
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
Compression=lzma2
SolidCompression=yes

[Tasks]
Name: "desktopicon"; Description: "A shortcut on the desktop"; Flags: unchecked

[Files]
Source: "{#Source}\Pina Live.exe"; DestDir: "{app}"; Flags: ignoreversion
; DirectML, newer than Windows' own, for ONNX Runtime (stem separation); and the C runtime's DLLs,
; Visual C++'s redistributable, so it runs on a PC without it installed.
Source: "{#Source}\*.dll"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\..\crates\app\icon\live.ico"; DestDir: "{app}"; Flags: ignoreversion
; Its licence, and every open-source component's.
Source: "{#Source}\LICENSE.txt"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#Source}\THIRD-PARTY-NOTICES.md"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\Pina Live"; Filename: "{app}\Pina Live.exe"; IconFilename: "{app}\live.ico"
Name: "{autodesktop}\Pina Live"; Filename: "{app}\Pina Live.exe"; IconFilename: "{app}\live.ico"; Tasks: desktopicon

[Run]
Filename: "{app}\Pina Live.exe"; Description: "Open Pina Live"; Flags: nowait postinstall skipifsilent
