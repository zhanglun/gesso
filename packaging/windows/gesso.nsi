; Gesso Windows installer (NSIS 3, per-user): setup.exe, no admin, installs to
; %LOCALAPPDATA%\Gesso, Start-menu + desktop shortcuts, uninstaller entry
; in "Apps & features" (HKCU).
; Version is injected by build_portable.ps1: makensis /DVERSION=<workspace version>
; Keep literals ASCII: script files are read verbatim, locale-dependent
; codepages would garble non-ASCII text on some systems.

Unicode true

!include LogicLib.nsh

!define APPNAME "Gesso"
!define PUBLISHER "Gesso"
!ifndef VERSION
  !define VERSION "0.0.0"
!endif
!ifndef ARCH
  !define ARCH "x64"
!endif

Name "${APPNAME} ${VERSION}"
; NSIS resolves relative paths against the SCRIPT directory
; (packaging\windows), so repo-root targets need ..\..\ — CWD is ignored.
OutFile "..\..\target\release-bundle\Gesso-${VERSION}-${ARCH}-setup.exe"
InstallDir "$LOCALAPPDATA\${APPNAME}"
InstallDirRegKey HKCU "Software\${APPNAME}" "InstallDir"
RequestExecutionLevel user
SetCompressor /SOLID lzma

; Need admin? Never — per-user install only.
ShowInstDetails show
ShowUninstDetails show

Page directory
Page instfiles
UninstPage uninstConfirm
UninstPage instfiles

; The in-app updater (design spec 4.7, 2026-11-10) quits Gesso before running
; this installer; this wait loop is the safety net for manual/edge cases: a
; running gesso.exe would lock the file copy below and pop "Retry/Abort".
Function .onInit
  StrCpy $R0 0
  ${While} $R0 < 30
    ; find.exe exit code: 0 = gesso.exe still running, 1 = gone
    nsExec::ExecToStack 'cmd /c tasklist /NH /FO CSV | find /I "gesso.exe"'
    Pop $R1
    Pop $R2
    ${If} $R1 != 0
      ${Break}
    ${EndIf}
    Sleep 500
    IntOp $R0 $R0 + 1
  ${EndWhile}
FunctionEnd

; Upgrade loop closure (design spec 4.7, 2026-11-10): launch the freshly
; installed Gesso as soon as the copy succeeds, no need to hunt for shortcuts.
Function .onInstSuccess
  Exec '"$INSTDIR\gesso.exe"'
FunctionEnd

Section "Install"
  SetOutPath "$INSTDIR"
  File "..\..\target\release\gesso.exe"
  ; Asset trees copied whole (host page + built-in samples)
  SetOutPath "$INSTDIR\assets"
  File /r "..\..\crates\app\assets\host"
  File /r "..\..\crates\app\assets\samples"

  WriteUninstaller "$INSTDIR\uninstall.exe"

  ; Shortcuts (Start menu folder + desktop)
  CreateDirectory "$SMPROGRAMS\${APPNAME}"
  CreateShortcut "$SMPROGRAMS\${APPNAME}\${APPNAME}.lnk" "$INSTDIR\gesso.exe"
  CreateShortcut "$SMPROGRAMS\${APPNAME}\Uninstall ${APPNAME}.lnk" "$INSTDIR\uninstall.exe"
  CreateShortcut "$DESKTOP\${APPNAME}.lnk" "$INSTDIR\gesso.exe"

  ; "Apps & features" entry (HKCU — per-user)
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\${APPNAME}" "DisplayName" "${APPNAME}"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\${APPNAME}" "DisplayVersion" "${VERSION}"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\${APPNAME}" "Publisher" "${PUBLISHER}"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\${APPNAME}" "DisplayIcon" "$INSTDIR\gesso.exe"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\${APPNAME}" "UninstallString" "$INSTDIR\uninstall.exe"
  WriteRegDWORD HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\${APPNAME}" "NoModify" 1
  WriteRegDWORD HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\${APPNAME}" "NoRepair" 1
SectionEnd

Section "Uninstall"
  ; App files + assets are ours; user data in %USERPROFILE%\.gesso is kept.
  RMDir /r "$INSTDIR\assets"
  Delete "$INSTDIR\gesso.exe"
  Delete "$INSTDIR\uninstall.exe"
  RMDir "$INSTDIR"
  Delete "$SMPROGRAMS\${APPNAME}\${APPNAME}.lnk"
  Delete "$SMPROGRAMS\${APPNAME}\Uninstall ${APPNAME}.lnk"
  RMDir "$SMPROGRAMS\${APPNAME}"
  Delete "$DESKTOP\${APPNAME}.lnk"
  DeleteRegKey HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\${APPNAME}"
SectionEnd
