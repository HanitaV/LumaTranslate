#ifndef AppVersion
  #define AppVersion "1.0.0"
#endif

#ifndef SourceDir
  #define SourceDir "..\dist\LumaTranslate-win-x64"
#endif

#ifndef OutputDir
  #define OutputDir "..\dist"
#endif

#ifndef OutputBaseFilename
  #define OutputBaseFilename "LumaTranslate-Setup-x64"
#endif

[Setup]
AppId=LumaTranslate
AppName=LumaTranslate
AppVersion={#AppVersion}
AppVerName=LumaTranslate {#AppVersion}
AppPublisher=LumaTranslate
AppPublisherURL=https://github.com/HanitaV/LumaTranslate
AppSupportURL=https://github.com/HanitaV/LumaTranslate/issues
AppUpdatesURL=https://github.com/HanitaV/LumaTranslate/releases
DefaultDirName={localappdata}\Programs\LumaTranslate
DefaultGroupName=LumaTranslate
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
OutputDir={#OutputDir}
OutputBaseFilename={#OutputBaseFilename}
SetupIconFile=..\apps\LumaTranslate.WinUI\Assets\AppIcon.ico
UninstallDisplayIcon={app}\LumaTranslate.exe
UninstallDisplayName=LumaTranslate
LicenseFile=..\LICENSE
Compression=lzma2/fast
SolidCompression=yes
WizardStyle=modern
CloseApplications=force
RestartApplications=no
UsePreviousAppDir=yes

[Messages]
SetupWindowTitle=Cài đặt - %1
WelcomeLabel1=Chào mừng bạn đến với trình cài đặt [name]
WelcomeLabel2=Trình hướng dẫn sẽ cài đặt [name/ver] vào máy tính của bạn (mặc định tại thư mục AppData cá nhân, không yêu cầu quyền Quản trị viên).%n%nSau khi cài đặt xong, lần khởi chạy đầu tiên sẽ tự động kiểm tra GPU/CPU và tải môi trường AI tiếng Việt cần thiết.
ClickNext=Nhấn "Tiếp tục" để sang bước kế tiếp, hoặc "Hủy" để thoát trình cài đặt.
ButtonBack=< &Quay lại
ButtonNext=&Tiếp tục >
ButtonInstall=&Cài đặt
ButtonFinish=&Hoàn tất
ButtonCancel=Hủy
ButtonYes=&Có
ButtonNo=&Không
WizardSelectDir=Chọn thư mục cài đặt
SelectDirDesc=Bạn muốn cài đặt [name] vào đâu?
SelectDirLabel3=Trình cài đặt sẽ cài [name] vào thư mục dưới đây (khuyến nghị giữ mặc định trong AppData).
SelectDirBrowseLabel=Để tiếp tục, nhấn "Tiếp tục". Nếu muốn chọn thư mục khác, nhấn "Duyệt...".
ButtonWizardBrowse=D&uyệt...
DiskSpaceMBLabel=Cần tối thiểu [mb] MB dung lượng ổ đĩa trống cho bộ khung ứng dụng.
WizardSelectTasks=Chọn tác vụ bổ sung
SelectTasksDesc=Bạn muốn thực hiện thêm tác vụ nào?
SelectTasksLabel2=Chọn các lối tắt bổ sung bạn muốn tạo khi cài đặt [name], sau đó nhấn "Tiếp tục".
WizardReady=Sẵn sàng cài đặt
ReadyLabel1=Trình cài đặt đã sẵn sàng cài [name] vào máy tính của bạn.
ReadyLabel2a=Nhấn "Cài đặt" để bắt đầu quá trình cài đặt, hoặc nhấn "Quay lại" nếu muốn xem lại thiết lập.
WizardInstalling=Đang cài đặt
InstallingLabel=Vui lòng đợi trong khi trình cài đặt sao chép [name] vào máy tính của bạn.
FinishedHeadingLabel=Hoàn tất cài đặt [name]
FinishedLabelNoIcons=Trình cài đặt đã cài xong [name] trên máy tính của bạn.
FinishedLabel=Trình cài đặt đã cài xong [name] trên máy tính của bạn. Bạn có thể mở ứng dụng từ Start Menu hoặc lối tắt đã tạo.
ClickFinish=Nhấn "Hoàn tất" để đóng trình cài đặt.
ConfirmUninstall=Bạn có chắc chắn muốn gỡ cài đặt %1 và các thành phần ứng dụng khỏi máy tính không?
UninstalledAll=Đã gỡ cài đặt %1 khỏi máy tính của bạn thành công.

[Tasks]
Name: "desktopicon"; Description: "Tạo lối tắt LumaTranslate ngoài màn hình nền (Desktop)"; GroupDescription: "Lối tắt bổ sung:"

[Files]
Source: "{#SourceDir}\*"; DestDir: "{app}"; Flags: ignoreversion recursesubdirs createallsubdirs

[Icons]
Name: "{autoprograms}\LumaTranslate"; Filename: "{app}\LumaTranslate.exe"; WorkingDir: "{app}"; Comment: "Dịch phụ đề màn hình & đọc thuyết minh tiếng Việt thời gian thực"
Name: "{autodesktop}\LumaTranslate"; Filename: "{app}\LumaTranslate.exe"; WorkingDir: "{app}"; Tasks: desktopicon; Comment: "Dịch phụ đề màn hình & đọc thuyết minh tiếng Việt thời gian thực"

[Registry]
Root: HKCU; Subkey: "Software\Microsoft\Windows\CurrentVersion\Uninstall\LumaTranslate_is1"; ValueType: string; ValueName: "ModifyPath"; ValueData: """{app}\LumaTranslate.exe"" --setup"; Flags: uninsdeletevalue
Root: HKCU; Subkey: "Software\Microsoft\Windows\CurrentVersion\Uninstall\LumaTranslate_is1"; ValueType: dword; ValueName: "NoModify"; ValueData: "0"; Flags: uninsdeletevalue
Root: HKCU; Subkey: "Software\Microsoft\Windows\CurrentVersion\Uninstall\LumaTranslate_is1"; ValueType: dword; ValueName: "NoRepair"; ValueData: "1"; Flags: uninsdeletevalue

[Run]
Filename: "{app}\LumaTranslate.exe"; WorkingDir: "{app}"; Description: "Khởi chạy LumaTranslate ngay bây giờ"; Flags: nowait postinstall skipifsilent

[Code]
function HasCmdSwitch(const SwitchName: String): Boolean;
var
  I: Integer;
begin
  Result := False;
  for I := 1 to ParamCount do
  begin
    if CompareText(ParamStr(I), SwitchName) = 0 then
    begin
      Result := True;
      Exit;
    end;
  end;
end;

procedure StopLumaProcesses(const AppDir: String);
var
  ResultCode: Integer;
  LocalDataRoot, PsCmd: String;
begin
  Exec('taskkill.exe', '/F /IM LumaTranslate.exe /T', '', SW_HIDE, ewWaitUntilTerminated, ResultCode);
  Exec('taskkill.exe', '/F /IM LumaTranslate.Backend.exe /T', '', SW_HIDE, ewWaitUntilTerminated, ResultCode);
  Exec('taskkill.exe', '/F /IM luma_translate_backend.exe /T', '', SW_HIDE, ewWaitUntilTerminated, ResultCode);

  LocalDataRoot := ExpandConstant('{localappdata}\LumaTranslate');
  PsCmd :=
    '$ErrorActionPreference = ''SilentlyContinue''; ' +
    '$roots = @(''' + LocalDataRoot + ''', ''' + AppDir + '''); ' +
    'Get-Process -Name python -ErrorAction SilentlyContinue | ForEach-Object { ' +
    '  $p = $_.MainModule.FileName; ' +
    '  if ($p) { foreach ($r in $roots) { if ($r -and $p.StartsWith($r, [System.StringComparison]::OrdinalIgnoreCase)) { Stop-Process -Id $_.Id -Force } } } ' +
    '}';
  Exec(
    'powershell.exe',
    '-NoProfile -NonInteractive -ExecutionPolicy Bypass -WindowStyle Hidden -Command "' + PsCmd + '"',
    '',
    SW_HIDE,
    ewWaitUntilTerminated,
    ResultCode);
end;

function PrepareToInstall(var NeedsRestart: Boolean): String;
begin
  StopLumaProcesses(WizardDirValue);
  Result := '';
end;

procedure CurStepChanged(CurStep: TSetupStep);
begin
  if CurStep = ssPostInstall then
  begin
    RegDeleteKeyIncludingSubkeys(HKCU, 'Software\Microsoft\Windows\CurrentVersion\Uninstall\LumaTranslate');
  end;
end;

procedure RemoveDirIfEmpty(const DirPath: String);
var
  FindRec: TFindRec;
  HasEntries: Boolean;
begin
  if not DirExists(DirPath) then
    Exit;

  HasEntries := False;
  if FindFirst(DirPath + '\*', FindRec) then
  begin
    try
      repeat
        if (FindRec.Name <> '.') and (FindRec.Name <> '..') then
        begin
          HasEntries := True;
          Break;
        end;
      until not FindNext(FindRec);
    finally
      FindClose(FindRec);
    end;
  end;

  if not HasEntries then
    RemoveDir(DirPath);
end;

procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
var
  LocalDataRoot: String;
  HasAiEnv, HasUserData, RemoveAiEnv, RemoveUserData: Boolean;
begin
  if CurUninstallStep = usUninstall then
  begin
    StopLumaProcesses(ExpandConstant('{app}'));
    RegDeleteKeyIncludingSubkeys(HKCU, 'Software\Microsoft\Windows\CurrentVersion\Uninstall\LumaTranslate');

    LocalDataRoot := ExpandConstant('{localappdata}\LumaTranslate');
    HasAiEnv :=
      DirExists(LocalDataRoot + '\runtimes') or
      DirExists(LocalDataRoot + '\models') or
      DirExists(LocalDataRoot + '\tools');
    HasUserData :=
      DirExists(LocalDataRoot + '\data') or
      FileExists(LocalDataRoot + '\ui-errors.log');

    if UninstallSilent then
    begin
      RemoveAiEnv := not HasCmdSwitch('/KEEPENV');
      RemoveUserData := HasCmdSwitch('/PURGEDATA');
    end
    else
    begin
      RemoveAiEnv := False;
      RemoveUserData := False;

      if HasAiEnv then
      begin
        RemoveAiEnv :=
          MsgBox(
            'Bạn có muốn xóa luôn môi trường Python 3.12 và các mô hình AI đã tải về trong:' + #13#10 +
            LocalDataRoot + #13#10#13#10 +
            '(Chọn "Có" để giải phóng toàn bộ vài GB dung lượng ổ đĩa; chọn "Không" nếu bạn chỉ cài đặt lại ứng dụng và muốn giữ mô hình AI.)',
            mbConfirmation,
            MB_YESNO) = IDYES;
      end;

      if HasUserData then
      begin
        RemoveUserData :=
          MsgBox(
            'Bạn có muốn xóa luôn cài đặt cá nhân, bộ nhớ đệm và lịch sử bản dịch tiếng Việt trong:' + #13#10 +
            LocalDataRoot + '\data ?',
            mbConfirmation,
            MB_YESNO or MB_DEFBUTTON2) = IDYES;
      end;
    end;

    if RemoveAiEnv then
    begin
      DelTree(LocalDataRoot + '\runtimes', True, True, True);
      DelTree(LocalDataRoot + '\models', True, True, True);
      DelTree(LocalDataRoot + '\tools', True, True, True);
    end;

    if RemoveUserData then
    begin
      DelTree(LocalDataRoot + '\data', True, True, True);
      DeleteFile(LocalDataRoot + '\ui-errors.log');
    end;

    if RemoveAiEnv and RemoveUserData then
      DelTree(LocalDataRoot, True, True, True)
    else
      RemoveDirIfEmpty(LocalDataRoot);
  end;
end;
