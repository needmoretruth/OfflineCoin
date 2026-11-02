; The Windows installer. Runs for one person with no administrator prompt.
; Wallets and chains live in the person's own folders and no file here names them, so the uninstaller
; leaves them alone. Compiled by package.sh, which gives the paths and version in the environment.
#define Version GetEnv("OLC_VERSION")
#define Source GetEnv("OLC_SOURCE")
#define Icon GetEnv("OLC_ICON")
#define Output GetEnv("OLC_OUTPUT")

[Setup]
AppId={{6F2B1D0E-4C57-4E0B-9B7E-0C3A5A1E7D21}
AppName=OLC Core
AppVersion={#Version}
AppPublisher=OfflineCoin
AppPublisherURL=https://github.com/needmoretruth/OfflineCoin
PrivilegesRequired=lowest
DefaultDirName={autopf}\OLC Core
DisableProgramGroupPage=yes
DisableDirPage=yes
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
OutputDir={#Output}
OutputBaseFilename=OLC-Core-windows-x86_64-setup
SetupIconFile={#Icon}
UninstallDisplayIcon={app}\olc-core.ico
Compression=lzma2/max
SolidCompression=yes
WizardStyle=modern
ShowLanguageDialog=no

[Languages]
Name: "en"; MessagesFile: "compiler:Default.isl"
Name: "ko"; MessagesFile: "compiler:Languages\Korean.isl"

[Files]
Source: "{#Source}\olc-core.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#Source}\espflash.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#Icon}"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\OLC Core"; Filename: "{app}\olc-core.exe"; IconFilename: "{app}\olc-core.ico"
Name: "{autodesktop}\OLC Core"; Filename: "{app}\olc-core.exe"; IconFilename: "{app}\olc-core.ico"

[Run]
Filename: "{app}\olc-core.exe"; Description: "{cm:LaunchProgram,OLC Core}"; Flags: nowait postinstall skipifsilent
