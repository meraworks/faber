[Setup]
AppId=Faber
AppName=Faber
AppVersion={#Version}
DefaultDirName={localappdata}\Programs\Faber
PrivilegesRequired=lowest
UninstallDisplayIcon={app}\faber-viewer.exe
SetupIconFile=..\assets\icon.ico
OutputDir=..\target\installers
OutputBaseFilename=faber-{#Version}-{#Arch}-setup
Compression=lzma2/fast
WizardStyle=modern
[Files]
Source: "{#Bin}\faber.exe"; DestDir: "{app}"
Source: "{#Bin}\faber-viewer.exe"; DestDir: "{app}"
Source: "{#Bin}\*.dll"; DestDir: "{app}"
[Icons]
Name: "{userprograms}\Faber"; Filename: "{app}\faber-viewer.exe"; WorkingDir: "{app}"
