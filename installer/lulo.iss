; Lulo installer (Inno Setup 6).
;
; Per-user install: no admin rights and no UAC prompt. Files go to
; %LOCALAPPDATA%\Lulo, the same folder the double-click setup of lulo-hook.exe
; uses, so installing over a zip install keeps the hooks pointing at the
; same path.
;
; Build:  iscc installer\lulo.iss /DAppVersion=0.1.0
; Signed: iscc installer\lulo.iss /DAppVersion=0.1.0 /DSign "/Slulo=<command> $f"
;         (installer\build.ps1 and the release workflow pass the command.)

#ifndef AppVersion
  #define AppVersion "0.1.0"
#endif
#ifndef BinDir
  #define BinDir "..\target\release"
#endif

[Setup]
; Never change AppId: Windows uses it to recognise upgrades and the uninstaller.
AppId={{FD466F9B-8291-4D0B-9E46-06027F50CE63}
AppName=Lulo
AppVersion={#AppVersion}
AppVerName=Lulo {#AppVersion}
AppPublisher=Lulo
AppPublisherURL=https://github.com/festupinans/Lulo
AppSupportURL=https://github.com/festupinans/Lulo/issues
VersionInfoVersion={#AppVersion}
VersionInfoDescription=Instalador de Lulo
DefaultDirName={localappdata}\Lulo
DisableDirPage=yes
DisableProgramGroupPage=yes
DisableReadyPage=yes
PrivilegesRequired=lowest
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
MinVersion=10.0
OutputDir=..\target\installer
OutputBaseFilename=Lulo-Setup-{#AppVersion}
Compression=lzma2/max
SolidCompression=yes
WizardStyle=modern
WizardSizePercent=100
ShowLanguageDialog=no
UninstallDisplayName=Lulo
UninstallDisplayIcon={app}\lulo-widget.exe
; The widget is closed by [Code] below, so the Restart Manager page never shows.
CloseApplications=no
#ifdef Sign
SignTool=lulo
SignedUninstaller=yes
#endif

[Languages]
Name: "es"; MessagesFile: "compiler:Languages\Spanish.isl"
Name: "en"; MessagesFile: "compiler:Default.isl"

[CustomMessages]
es.AutoStart=Iniciar Lulo con Windows
es.InstallingHooks=Añadiendo los hooks de Lulo a Claude Code...
es.LaunchWidget=Abrir Lulo ahora
es.HooksFailed=No se pudieron añadir los hooks a ~/.claude/settings.json (código %1).%n%nPara ver el error, abre una terminal y ejecuta:%n"%2" install
en.AutoStart=Start Lulo with Windows
en.InstallingHooks=Adding Lulo's hooks to Claude Code...
en.LaunchWidget=Open Lulo now
en.HooksFailed=Could not add the hooks to ~/.claude/settings.json (code %1).%n%nTo see the error, open a terminal and run:%n"%2" install

[Messages]
es.FinishedLabel=Lulo quedó instalado. Reinicia las sesiones de Claude Code que tengas abiertas para que aparezcan en la media luna.
en.FinishedLabel=Lulo is installed. Restart any open Claude Code sessions so they show up in the half-moon.

[Tasks]
Name: "autostart"; Description: "{cm:AutoStart}"

[Files]
Source: "{#BinDir}\lulo-hook.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#BinDir}\lulo-widget.exe"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\Lulo"; Filename: "{app}\lulo-widget.exe"

[Registry]
; Same value the widget's "Iniciar con Windows" menu item reads and writes.
Root: HKCU; Subkey: "Software\Microsoft\Windows\CurrentVersion\Run"; ValueType: string; ValueName: "Lulo"; ValueData: """{app}\lulo-widget.exe"""; Tasks: autostart

[Run]
Filename: "{app}\lulo-widget.exe"; Description: "{cm:LaunchWidget}"; Flags: nowait postinstall skipifsilent

[UninstallRun]
Filename: "{app}\lulo-hook.exe"; Parameters: "uninstall"; Flags: runhidden; RunOnceId: "RemoveHooks"

[UninstallDelete]
Type: files; Name: "{app}\widget.json"
Type: filesandordirs; Name: "{localappdata}\claude-status"

[Code]
const
  RunKey = 'Software\Microsoft\Windows\CurrentVersion\Run';

procedure CloseWidget();
var
  ResultCode: Integer;
begin
  // The widget has no window to ask politely, and it keeps no unsaved state.
  Exec(ExpandConstant('{sys}\taskkill.exe'), '/F /IM lulo-widget.exe', '', SW_HIDE,
    ewWaitUntilTerminated, ResultCode);
end;

function PrepareToInstall(var NeedsRestart: Boolean): String;
begin
  CloseWidget();
  Result := '';
end;

procedure CurStepChanged(CurStep: TSetupStep);
var
  Hook: String;
  ResultCode: Integer;
begin
  if CurStep <> ssPostInstall then
    exit;
  WizardForm.StatusLabel.Caption := CustomMessage('InstallingHooks');
  Hook := ExpandConstant('{app}\lulo-hook.exe');
  if not Exec(Hook, 'install', '', SW_HIDE, ewWaitUntilTerminated, ResultCode) then
    ResultCode := -1;
  if ResultCode <> 0 then
    SuppressibleMsgBox(FmtMessage(CustomMessage('HooksFailed'), [IntToStr(ResultCode), Hook]),
      mbError, MB_OK, IDOK);
end;

procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
begin
  if CurUninstallStep = usUninstall then
  begin
    CloseWidget();
    // Also covers autostart turned on from the widget's own menu.
    RegDeleteValue(HKCU, RunKey, 'Lulo');
  end;
end;
