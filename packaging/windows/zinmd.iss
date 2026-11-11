; The Windows installer, built with Inno Setup by scripts/package-windows.ps1.
;
; It installs for the person running it, without asking for an administrator,
; and offers to open Markdown files with ZinMD. Removing ZinMD takes all of
; that away again.

#ifndef AppVersion
  #define AppVersion "0.0.0"
#endif
#ifndef NumericVersion
  #define NumericVersion "0.0.0"
#endif
#ifndef OutputName
  #define OutputName "ZinMD-setup"
#endif
#ifndef Binary
  #define Binary "..\..\target\dist\zinmd.exe"
#endif
#ifndef Arch
  #define Arch "x64compatible"
#endif

#define Root "..\.."
#define ProgId "ZinMD.Markdown"

[Setup]
; Never change this: it is how an update finds the copy it replaces.
AppId={{7B0B6F0E-5C0B-4E0B-9B5E-2D3A4C8E9A41}
AppName=ZinMD
AppVersion={#AppVersion}
AppVerName=ZinMD {#AppVersion}
AppPublisher=ZinMD
AppPublisherURL=https://github.com/iRbouh/ZinMD
AppSupportURL=https://github.com/iRbouh/ZinMD/issues
VersionInfoVersion={#NumericVersion}
DefaultDirName={autopf}\ZinMD
DisableProgramGroupPage=yes
DisableDirPage=auto
; No administrator needed; one can still be chosen, to install for everyone.
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog
ArchitecturesAllowed={#Arch}
ArchitecturesInstallIn64BitMode={#Arch}
ChangesAssociations=yes
OutputDir={#Root}\dist
OutputBaseFilename={#OutputName}
SetupIconFile={#Root}\assets\ZinMD.ico
UninstallDisplayIcon={app}\ZinMD.exe
UninstallDisplayName=ZinMD
Compression=lzma2/max
SolidCompression=yes
WizardStyle=modern
CloseApplications=yes

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "associate"; Description: "Open Markdown files (.md) with ZinMD"; GroupDescription: "Files:"
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked

[Files]
Source: "{#Binary}"; DestDir: "{app}"; DestName: "ZinMD.exe"; Flags: ignoreversion
Source: "{#Root}\assets\ZinMD-document.ico"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#Root}\LICENSE"; DestDir: "{app}"; DestName: "LICENSE.txt"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\ZinMD"; Filename: "{app}\ZinMD.exe"
Name: "{autodesktop}\ZinMD"; Filename: "{app}\ZinMD.exe"; Tasks: desktopicon

[Registry]
; What a Markdown file is, once it is ours: its name, its icon, how to open it.
Root: HKA; Subkey: "Software\Classes\{#ProgId}"; ValueType: string; ValueName: ""; ValueData: "Markdown document"; Flags: uninsdeletekey
Root: HKA; Subkey: "Software\Classes\{#ProgId}\DefaultIcon"; ValueType: string; ValueName: ""; ValueData: "{app}\ZinMD-document.ico"
Root: HKA; Subkey: "Software\Classes\{#ProgId}\shell\open\command"; ValueType: string; ValueName: ""; ValueData: """{app}\ZinMD.exe"" ""%1"""

; ZinMD in the "Open with" list of every kind of Markdown file. Always there.
Root: HKA; Subkey: "Software\Classes\.md\OpenWithProgids"; ValueType: string; ValueName: "{#ProgId}"; ValueData: ""; Flags: uninsdeletevalue
Root: HKA; Subkey: "Software\Classes\.markdown\OpenWithProgids"; ValueType: string; ValueName: "{#ProgId}"; ValueData: ""; Flags: uninsdeletevalue
Root: HKA; Subkey: "Software\Classes\.mdx\OpenWithProgids"; ValueType: string; ValueName: "{#ProgId}"; ValueData: ""; Flags: uninsdeletevalue
Root: HKA; Subkey: "Software\Classes\.mdown\OpenWithProgids"; ValueType: string; ValueName: "{#ProgId}"; ValueData: ""; Flags: uninsdeletevalue
Root: HKA; Subkey: "Software\Classes\.mkd\OpenWithProgids"; ValueType: string; ValueName: "{#ProgId}"; ValueData: ""; Flags: uninsdeletevalue

; The app itself, as Windows lists it among the apps that can open a file.
Root: HKA; Subkey: "Software\Classes\Applications\ZinMD.exe"; ValueType: string; ValueName: "FriendlyAppName"; ValueData: "ZinMD"; Flags: uninsdeletekey
Root: HKA; Subkey: "Software\Classes\Applications\ZinMD.exe\shell\open\command"; ValueType: string; ValueName: ""; ValueData: """{app}\ZinMD.exe"" ""%1"""
Root: HKA; Subkey: "Software\Classes\Applications\ZinMD.exe\SupportedTypes"; ValueType: string; ValueName: ".md"; ValueData: ""
Root: HKA; Subkey: "Software\Classes\Applications\ZinMD.exe\SupportedTypes"; ValueType: string; ValueName: ".markdown"; ValueData: ""
Root: HKA; Subkey: "Software\Classes\Applications\ZinMD.exe\SupportedTypes"; ValueType: string; ValueName: ".mdx"; ValueData: ""
Root: HKA; Subkey: "Software\Classes\Applications\ZinMD.exe\SupportedTypes"; ValueType: string; ValueName: ".mdown"; ValueData: ""
Root: HKA; Subkey: "Software\Classes\Applications\ZinMD.exe\SupportedTypes"; ValueType: string; ValueName: ".mkd"; ValueData: ""

; With the box ticked, a double click on a Markdown file opens ZinMD, unless
; the person has already chosen another app themselves: Windows keeps to that.
Root: HKA; Subkey: "Software\Classes\.md"; ValueType: string; ValueName: ""; ValueData: "{#ProgId}"; Flags: uninsdeletevalue; Tasks: associate
Root: HKA; Subkey: "Software\Classes\.markdown"; ValueType: string; ValueName: ""; ValueData: "{#ProgId}"; Flags: uninsdeletevalue; Tasks: associate
Root: HKA; Subkey: "Software\Classes\.mdx"; ValueType: string; ValueName: ""; ValueData: "{#ProgId}"; Flags: uninsdeletevalue; Tasks: associate
Root: HKA; Subkey: "Software\Classes\.mdown"; ValueType: string; ValueName: ""; ValueData: "{#ProgId}"; Flags: uninsdeletevalue; Tasks: associate
Root: HKA; Subkey: "Software\Classes\.mkd"; ValueType: string; ValueName: ""; ValueData: "{#ProgId}"; Flags: uninsdeletevalue; Tasks: associate

[Run]
Filename: "{app}\ZinMD.exe"; Description: "{cm:LaunchProgram,ZinMD}"; Flags: nowait postinstall skipifsilent
