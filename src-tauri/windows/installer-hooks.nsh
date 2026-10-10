; NSIS hooks for the per-user Windows installer (ADR-0008).
;
; Tauri's NSIS template inserts these macros into its install and uninstall
; sections (bundle.windows.nsis.installerHooks in tauri.conf.json). They ship
; the Explorer context-menu handler, shell_ext_win.dll (FR-001, ADR-0005), and
; register it for the current user only: every key is under HKCU, so neither
; the install nor an update asks for elevation (FR-083, NFR-020).
;
; What is registered is the "registry verb" shape documented in
; shell-ext-win/README.md: an ExplorerCommandHandler verb on files and folders
; whose CLSID is served in-process by the DLL. It shows in the Windows 10 menu
; and in the Windows 11 classic menu ("Show more options", Shift+F10). The
; Windows 11 top-level menu additionally needs a signed sparse package, which
; needs a code-signing certificate this project does not have yet (ADR-0008,
; Risks).
;
; This file is ASCII on purpose, and the CLSID below must match
; shell-ext-win/src/clsid.rs (src-tauri/tests/installer_hooks.rs checks both).

!define MAECLYPRINT_SHELLEXT_CLSID "{B269B8DC-044F-4D65-8F1E-8F20A2E653EA}"
!define MAECLYPRINT_SHELLEXT_DLL "shell_ext_win.dll"
!define MAECLYPRINT_SHELLEXT_VERB "MaeclyPrint"

; The DLL is built for the installer's architecture by
; scripts/build-shell-ext.ps1, which copies it next to this file. A missing DLL
; stops the installer build instead of shipping without the context menu.
!if /FileExists "${__FILEDIR__}\${MAECLYPRINT_SHELLEXT_DLL}"
!else
  !error "src-tauri\windows\${MAECLYPRINT_SHELLEXT_DLL} is missing: run scripts\build-shell-ext.ps1 before packaging"
!endif

; Explorer keeps the DLL loaded once a menu has been shown, and a loaded DLL
; cannot be overwritten, but it can be renamed. Move the old copy aside under a
; free name and let Windows delete it at the next restart.
!macro MAECLYPRINT_MOVE_LOADED_DLL_ASIDE
  Delete "$INSTDIR\${MAECLYPRINT_SHELLEXT_DLL}.old*"
  IfFileExists "$INSTDIR\${MAECLYPRINT_SHELLEXT_DLL}" 0 maeclyprint_dll_free
  StrCpy $R9 0
  maeclyprint_dll_next:
    IntOp $R9 $R9 + 1
    IntCmp $R9 100 maeclyprint_dll_free 0 maeclyprint_dll_free
    IfFileExists "$INSTDIR\${MAECLYPRINT_SHELLEXT_DLL}.old$R9" maeclyprint_dll_next 0
    ClearErrors
    Rename "$INSTDIR\${MAECLYPRINT_SHELLEXT_DLL}" "$INSTDIR\${MAECLYPRINT_SHELLEXT_DLL}.old$R9"
    IfErrors 0 +2
      Goto maeclyprint_dll_free
    Delete /REBOOTOK "$INSTDIR\${MAECLYPRINT_SHELLEXT_DLL}.old$R9"
  maeclyprint_dll_free:
!macroend

!macro MAECLYPRINT_REGISTER_VERB CLASS
  WriteRegStr HKCU "Software\Classes\${CLASS}\shell\${MAECLYPRINT_SHELLEXT_VERB}" "" "MaeclyPrint"
  WriteRegStr HKCU "Software\Classes\${CLASS}\shell\${MAECLYPRINT_SHELLEXT_VERB}" "ExplorerCommandHandler" "${MAECLYPRINT_SHELLEXT_CLSID}"
  WriteRegStr HKCU "Software\Classes\${CLASS}\shell\${MAECLYPRINT_SHELLEXT_VERB}" "MultiSelectModel" "Player"
  WriteRegStr HKCU "Software\Classes\${CLASS}\shell\${MAECLYPRINT_SHELLEXT_VERB}" "Icon" "$INSTDIR\maeclyprint.exe,0"
!macroend

!macro NSIS_HOOK_POSTINSTALL
  !insertmacro MAECLYPRINT_MOVE_LOADED_DLL_ASIDE
  SetOutPath "$INSTDIR"
  File "${__FILEDIR__}\${MAECLYPRINT_SHELLEXT_DLL}"

  ; The in-process COM server, per user.
  WriteRegStr HKCU "Software\Classes\CLSID\${MAECLYPRINT_SHELLEXT_CLSID}" "" "MaeclyPrint context menu"
  WriteRegStr HKCU "Software\Classes\CLSID\${MAECLYPRINT_SHELLEXT_CLSID}\InprocServer32" "" "$INSTDIR\${MAECLYPRINT_SHELLEXT_DLL}"
  WriteRegStr HKCU "Software\Classes\CLSID\${MAECLYPRINT_SHELLEXT_CLSID}\InprocServer32" "ThreadingModel" "Apartment"

  ; The verb on every file and on folders. A COM verb has no 15-item limit.
  !insertmacro MAECLYPRINT_REGISTER_VERB "*"
  !insertmacro MAECLYPRINT_REGISTER_VERB "Directory"

  ; SHCNE_ASSOCCHANGED: Explorer re-reads the verbs without a restart.
  System::Call 'shell32::SHChangeNotify(i 0x08000000, i 0, p 0, p 0)'
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  DeleteRegKey HKCU "Software\Classes\*\shell\${MAECLYPRINT_SHELLEXT_VERB}"
  DeleteRegKey HKCU "Software\Classes\Directory\shell\${MAECLYPRINT_SHELLEXT_VERB}"
  DeleteRegKey HKCU "Software\Classes\CLSID\${MAECLYPRINT_SHELLEXT_CLSID}"
  System::Call 'shell32::SHChangeNotify(i 0x08000000, i 0, p 0, p 0)'
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  ; Still loaded by Explorer: removed at the next restart instead.
  Delete /REBOOTOK "$INSTDIR\${MAECLYPRINT_SHELLEXT_DLL}"
  Delete /REBOOTOK "$INSTDIR\${MAECLYPRINT_SHELLEXT_DLL}.old*"
  RMDir /REBOOTOK "$INSTDIR"
!macroend
