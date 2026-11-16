#ifndef AppVersion
  #define AppVersion "1.4.0"
#endif
#ifndef ReleaseDir
  #define ReleaseDir "..\dist"
#endif

[Setup]
AppId={{12DEC791-494D-46DB-BDD4-4E29E6665211}
AppName=Fortnite Chest Ledger
AppVersion={#AppVersion}
AppSupportURL=https://github.com/IsidroTrevino/fortnite-chest-ledger/issues
AppUpdatesURL=https://github.com/IsidroTrevino/fortnite-chest-ledger/releases/latest
DefaultDirName={localappdata}\Programs\Fortnite Chest Ledger
DefaultGroupName=Fortnite Chest Ledger
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
ArchitecturesAllowed=x64compatible
MinVersion=10.0
UninstallDisplayIcon={app}\TreasureLedger.exe
CloseApplications=yes
CloseApplicationsFilter=TreasureLedger.exe
RestartApplications=no
WizardStyle=modern
SetupIconFile=..\assets\treasure-chest.ico
#ifdef SignProfile
SignTool={#SignProfile}
SignedUninstaller=yes
#endif
Compression=lzma2/normal
SolidCompression=yes
OutputDir={#ReleaseDir}
OutputBaseFilename=FortniteChestLedger-Setup

[Tasks]
Name: "desktopicon"; Description: "Create a desktop shortcut"

[Files]
Source: "{#ReleaseDir}\TreasureLedger.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#ReleaseDir}\LICENSE-ChestCounter.txt"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#ReleaseDir}\LICENSE-FortniteReplayDecompressor.txt"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#ReleaseDir}\LICENSE-dotnet.txt"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#ReleaseDir}\ThirdPartyNotices-dotnet.txt"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{userprograms}\Fortnite Chest Ledger"; Filename: "{app}\TreasureLedger.exe"
Name: "{userdesktop}\Fortnite Chest Ledger"; Filename: "{app}\TreasureLedger.exe"; Tasks: desktopicon

[Run]
Filename: "{app}\TreasureLedger.exe"; Description: "Open Fortnite Chest Ledger"; Flags: postinstall nowait skipifsilent

[UninstallRun]
Filename: "{app}\TreasureLedger.exe"; Parameters: "--disable-startup"; Flags: runhidden; RunOnceId: "DisableFortniteChestLedgerStartup"
