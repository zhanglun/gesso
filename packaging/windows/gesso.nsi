; Gesso Windows installer (NSIS 3) — same installer form as our Tauri
; projects (lettura / pavo): per-user setup.exe, no admin, installs to
; %LOCALAPPDATA%\Gesso, Start-menu + desktop shortcuts, uninstaller entry
; in "Apps & features" (HKCU).
; Version is injected by build_portable.ps1: makensis /DVERSION=0.1.0
; Keep literals ASCII: script files are read verbatim, locale-dependent
; codepages would garble non-ASCII text on some systems.

Unicode true

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

Section "Install"
  SetOutPath "$INSTDIR"
  File "..\..\target\release\gesso.exe"
  ; Asset trees copied whole (host page + built-in samples)
  SetOutPath "$INSTDIR\assets"
  File /r "..\..\crates\app\assets\host"
  File /r "..\..\crates\app\assets\samples"

  WriteUninstaller "$INSTDIR\uninstall.exe"

  ; Shortcuts (Start menu folder + desktop, same affordances as Tauri NSIS)
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
