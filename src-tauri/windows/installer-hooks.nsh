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
; Before any of that, NSIS_HOOK_PREINSTALL takes over an install made under
; the product's former name, MaeclyPrint (ADR-0009); see its section below.
;
; This file is ASCII on purpose, and the CLSID below must match
; shell-ext-win/src/clsid.rs (src-tauri/tests/installer_hooks.rs checks both).

!define PLURYPRINT_SHELLEXT_CLSID "{B269B8DC-044F-4D65-8F1E-8F20A2E653EA}"
!define PLURYPRINT_SHELLEXT_DLL "shell_ext_win.dll"
!define PLURYPRINT_SHELLEXT_VERB "Pluryprint"

; The DLL ships for BOTH Windows architectures, as bundle resources
; (src-tauri/resources/shellext/<arch>/, see the README there), and the one
; registered is chosen by the operating system's native architecture, never by
; the installer's: Explorer on Windows on ARM is a native ARM64 process and
; cannot load an x64 DLL, even when this x64 installer runs there emulated.
; scripts/build-shell-ext.ps1 builds them. A build with neither DLL stops
; here instead of shipping without the context menu, and a CI build ($%CI% is
; "true" on GitHub Actions) must carry both. A local build may carry only its
; own architecture; installed on the other one, it simply registers no menu.
; Resolved here, at include time: inside a macro ${__FILEDIR__} would name the
; generated installer.nsi's folder instead of this file's.
!define PLURYPRINT_SHELLEXT_RES "${__FILEDIR__}\..\resources\shellext"
!if /FileExists "${PLURYPRINT_SHELLEXT_RES}\x64\${PLURYPRINT_SHELLEXT_DLL}"
  !define PLURYPRINT_SHELLEXT_HAS_X64
!endif
!if /FileExists "${PLURYPRINT_SHELLEXT_RES}\arm64\${PLURYPRINT_SHELLEXT_DLL}"
  !define PLURYPRINT_SHELLEXT_HAS_ARM64
!endif
!ifndef PLURYPRINT_SHELLEXT_HAS_X64
  !ifndef PLURYPRINT_SHELLEXT_HAS_ARM64
    !error "src-tauri\resources\shellext\<x64|arm64>\${PLURYPRINT_SHELLEXT_DLL} is missing: run scripts\build-shell-ext.ps1 before packaging"
  !endif
!endif
!if "$%CI%" == "true"
  !ifndef PLURYPRINT_SHELLEXT_HAS_X64
    !error "CI builds ship the x64 context-menu DLL too (resources\shellext\x64)"
  !endif
  !ifndef PLURYPRINT_SHELLEXT_HAS_ARM64
    !error "CI builds ship the arm64 context-menu DLL too (resources\shellext\arm64): Explorer on Windows on ARM cannot load the x64 one"
  !endif
!endif

; Explorer keeps the DLL loaded once a menu has been shown, and a loaded DLL
; cannot be overwritten, but it can be renamed. Move the old copy aside under a
; free name and let Windows delete it at the next restart.
!macro PLURYPRINT_MOVE_LOADED_DLL_ASIDE
  Delete "$INSTDIR\${PLURYPRINT_SHELLEXT_DLL}.old*"
  IfFileExists "$INSTDIR\${PLURYPRINT_SHELLEXT_DLL}" 0 pluryprint_dll_free
  StrCpy $R9 0
  pluryprint_dll_next:
    IntOp $R9 $R9 + 1
    IntCmp $R9 100 pluryprint_dll_free 0 pluryprint_dll_free
    IfFileExists "$INSTDIR\${PLURYPRINT_SHELLEXT_DLL}.old$R9" pluryprint_dll_next 0
    ClearErrors
    Rename "$INSTDIR\${PLURYPRINT_SHELLEXT_DLL}" "$INSTDIR\${PLURYPRINT_SHELLEXT_DLL}.old$R9"
    IfErrors 0 +2
      Goto pluryprint_dll_free
    Delete /REBOOTOK "$INSTDIR\${PLURYPRINT_SHELLEXT_DLL}.old$R9"
  pluryprint_dll_free:
!macroend

!macro PLURYPRINT_REGISTER_VERB CLASS
  WriteRegStr HKCU "Software\Classes\${CLASS}\shell\${PLURYPRINT_SHELLEXT_VERB}" "" "Pluryprint"
  WriteRegStr HKCU "Software\Classes\${CLASS}\shell\${PLURYPRINT_SHELLEXT_VERB}" "ExplorerCommandHandler" "${PLURYPRINT_SHELLEXT_CLSID}"
  WriteRegStr HKCU "Software\Classes\${CLASS}\shell\${PLURYPRINT_SHELLEXT_VERB}" "MultiSelectModel" "Player"
  WriteRegStr HKCU "Software\Classes\${CLASS}\shell\${PLURYPRINT_SHELLEXT_VERB}" "Icon" "$INSTDIR\pluryprint.exe,0"
!macroend

; ---- Taking over a MaeclyPrint install (ADR-0009) --------------------------
;
; Take over an install made under the product's former name, MaeclyPrint
; (ADR-0009). Tauri derives the per-user install folder, the Uninstall key,
; the HKCU\Software\<publisher>\<product> key and the shortcuts from
; productName, so a Pluryprint install would otherwise sit next to a
; MaeclyPrint one, both in "Apps & features" and both answering Explorer.
;
; User data is not touched here: the app moves it from the MaeclyPrint
; folders on its first launch (src-tauri/src/util/legacy.rs). The MaeclyPrint
; uninstaller only deletes app data when its "delete app data" box is ticked,
; which a silent run never does.
;
; Two kinds of old install exist:
;
; - Per user (the NSIS setup, HKCU). Removed silently. Its folder comes from
;   the default value of HKCU\Software\Maecly\MaeclyPrint, which Tauri writes
;   without quotes (the Uninstall key's InstallLocation is quoted, and `_?=`
;   must not be). `_?=` makes the uninstaller run in place, so ExecWait really
;   waits for it; without it the uninstaller copies itself to %TEMP% and
;   returns at once. If the uninstaller fails, its registration is left alone
;   so the old install can still be removed from "Apps & features".
; - Per machine (the WiX MSI that builds made before c1466b4, under Program
;   Files and HKLM). This installer runs without elevation, so it cannot
;   remove that install by itself: it asks, and msiexec then requests
;   administrator approval (UAC). A silent run, a "No" or a declined UAC
;   prompt leave the MSI install in place, and the setup says so in its log.

!define PLURY_LEGACY_PRODUCT "MaeclyPrint"
!define PLURY_LEGACY_PUBLISHER "Maecly"
!define PLURY_LEGACY_EXE "maeclyprint.exe"
!define PLURY_LEGACY_UNINSTKEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\${PLURY_LEGACY_PRODUCT}"
!define PLURY_LEGACY_PRODUCTKEY "Software\${PLURY_LEGACY_PUBLISHER}\${PLURY_LEGACY_PRODUCT}"
!define PLURY_UNINSTROOT "Software\Microsoft\Windows\CurrentVersion\Uninstall"

; Every message below, in English and Spanish (FR-093).
!include "${__FILEDIR__}\installer-strings.nsh"

; Drop one pair of surrounding double quotes from VAR, if it has them.
!macro PLURY_UNQUOTE VAR
  StrCpy $9 ${VAR} 1
  ${If} $9 == '"'
    StrCpy ${VAR} ${VAR} "" 1
    StrCpy $9 ${VAR} 1 -1
    ${If} $9 == '"'
      StrCpy ${VAR} ${VAR} -1
    ${EndIf}
  ${EndIf}
!macroend

; SumatraPDF, bundled under resources\sumatrapdf, runs portable: it writes
; SumatraPDF-settings.txt next to its exe the first time it prints. No
; uninstaller lists that file, so it kept resources\ (and with it the install
; folder) on disk. Remove it and the folders it was holding, if now empty.
!macro PLURY_REMOVE_SUMATRA_SETTINGS DIR
  Delete "${DIR}\resources\sumatrapdf\SumatraPDF-settings.txt"
  RMDir "${DIR}\resources\sumatrapdf"
  RMDir "${DIR}\resources"
!macroend

; Explorer keeps the old context-menu DLL loaded once its menu has been shown,
; so the old uninstaller cannot delete it, and a per-user process cannot have
; it deleted at the next restart either (that needs HKLM). A loaded DLL can
; still be moved within the volume: park it next to ours as
; shell_ext_win.dll.old<N>, which the next update or uninstall deletes
; (PLURYPRINT_MOVE_LOADED_DLL_ASIDE, NSIS_HOOK_POSTUNINSTALL). Uses $R5.
!macro PLURY_PARK_LEGACY_DLL DIR
  Delete "${DIR}\${PLURYPRINT_SHELLEXT_DLL}"
  IfFileExists "${DIR}\${PLURYPRINT_SHELLEXT_DLL}" 0 plury_park_done
  CreateDirectory "$INSTDIR"
  StrCpy $R5 0
  plury_park_next:
    IntOp $R5 $R5 + 1
    IntCmp $R5 100 plury_park_late 0 plury_park_late
    IfFileExists "$INSTDIR\${PLURYPRINT_SHELLEXT_DLL}.old$R5" plury_park_next 0
    ClearErrors
    Rename "${DIR}\${PLURYPRINT_SHELLEXT_DLL}" "$INSTDIR\${PLURYPRINT_SHELLEXT_DLL}.old$R5"
    IfErrors 0 plury_park_done
  plury_park_late:
    DetailPrint "$(PLURY_PARK_LATE) ${DIR}\${PLURYPRINT_SHELLEXT_DLL}"
  plury_park_done:
!macroend

!macro NSIS_HOOK_PREINSTALL
  Push $R0
  Push $R1
  Push $R2
  Push $R3
  Push $R4
  Push $R5
  Push $9

  ; --- Per-user MaeclyPrint (HKCU) -------------------------------------------
  ReadRegStr $R0 HKCU "${PLURY_LEGACY_UNINSTKEY}" "UninstallString"
  ; The folder, unquoted: Tauri's own reinstall path reads the same value.
  ReadRegStr $R1 HKCU "${PLURY_LEGACY_PRODUCTKEY}" ""
  ${If} $R1 == ""
    ReadRegStr $R1 HKCU "${PLURY_LEGACY_UNINSTKEY}" "InstallLocation"
    !insertmacro PLURY_UNQUOTE $R1
  ${EndIf}
  ${If} $R1 == ""
  ${AndIf} $R0 != ""
    ; Last resort: the folder that holds the uninstaller.
    StrCpy $R1 $R0
    !insertmacro PLURY_UNQUOTE $R1
    ${GetParent} $R1 $R1
  ${EndIf}

  ; $R3: 1 when the old registration may be cleaned up.
  StrCpy $R3 1
  ${If} $R0 != ""
    ${If} $R1 == ""
      DetailPrint "$(PLURY_LEGACY_UNKNOWN_DIR)"
      StrCpy $R3 0
    ${ElseIf} ${FileExists} "$R1\uninstall.exe"
      DetailPrint "$(PLURY_LEGACY_REMOVING)"
      ClearErrors
      ExecWait '"$R1\uninstall.exe" /S _?=$R1' $R2
      ${If} ${Errors}
        StrCpy $R2 "$(PLURY_NOT_STARTED)"
      ${EndIf}
      DetailPrint "$(PLURY_LEGACY_EXITED)"
      ${If} $R2 == 0
      ${AndIfNot} ${FileExists} "$R1\${PLURY_LEGACY_EXE}"
        ; Run in place, the uninstaller cannot delete itself or its folder.
        ; Windows also keeps its image locked for a moment after ExecWait
        ; returns, so the first Delete fails: retry for up to 5 s, then
        ; leave it to the next restart.
        StrCpy $R4 0
        plury_unexe_retry:
          ClearErrors
          Delete "$R1\uninstall.exe"
          IfErrors 0 plury_unexe_done
          IntOp $R4 $R4 + 1
          IntCmp $R4 50 plury_unexe_late 0 plury_unexe_late
          Sleep 100
          Goto plury_unexe_retry
        plury_unexe_late:
          Delete /REBOOTOK "$R1\uninstall.exe"
        plury_unexe_done:
        ; SumatraPDF runs portable and writes its settings next to itself.
        ; The old uninstaller does not know that file, so without this its
        ; folders, and the install folder, would stay behind.
        !insertmacro PLURY_REMOVE_SUMATRA_SETTINGS "$R1"
        !insertmacro PLURY_PARK_LEGACY_DLL "$R1"
        RMDir /REBOOTOK "$R1"
      ${Else}
        DetailPrint "$(PLURY_LEGACY_KEPT)"
        StrCpy $R3 0
      ${EndIf}
    ${Else}
      ; Registered, but its uninstaller is gone: only the registration is left.
      DetailPrint "$(PLURY_LEGACY_STALE)"
    ${EndIf}
  ${EndIf}

  ${If} $R3 = 1
    ; Whatever registration the old install (or a developer following the old
    ; shell-ext-win README) left behind, whether or not an uninstaller ran.
    DeleteRegKey HKCU "${PLURY_LEGACY_UNINSTKEY}"
    DeleteRegKey HKCU "${PLURY_LEGACY_PRODUCTKEY}"
    DeleteRegKey /ifempty HKCU "Software\${PLURY_LEGACY_PUBLISHER}"
    DeleteRegKey HKCU "Software\Classes\*\shell\${PLURY_LEGACY_PRODUCT}"
    DeleteRegKey HKCU "Software\Classes\Directory\shell\${PLURY_LEGACY_PRODUCT}"
    DeleteRegKey HKCU "Software\Classes\AllFileSystemObjects\shellex\ContextMenuHandlers\${PLURY_LEGACY_PRODUCT}"
    Delete "$SMPROGRAMS\${PLURY_LEGACY_PRODUCT}.lnk"
    Delete "$DESKTOP\${PLURY_LEGACY_PRODUCT}.lnk"
  ${EndIf}

  ; --- Per-machine MaeclyPrint (WiX MSI, HKLM) -------------------------------
  ; A WiX install is keyed by its product code, so look for it by name, the
  ; way Tauri's own PageReinstall looks for a WiX install of this product.
  StrCpy $R4 0
  plury_hklm_loop:
    EnumRegKey $R5 HKLM "${PLURY_UNINSTROOT}" $R4
    StrCmp $R5 "" plury_hklm_done
    IntOp $R4 $R4 + 1
    ReadRegStr $R0 HKLM "${PLURY_UNINSTROOT}\$R5" "DisplayName"
    ReadRegStr $R1 HKLM "${PLURY_UNINSTROOT}\$R5" "Publisher"
    ; StrCmp ignores case: early builds published as "maecly".
    StrCmp "$R0|$R1" "${PLURY_LEGACY_PRODUCT}|${PLURY_LEGACY_PUBLISHER}" 0 plury_hklm_loop
    ReadRegStr $R0 HKLM "${PLURY_UNINSTROOT}\$R5" "UninstallString"
    ${StrLoc} $R2 $R0 "msiexec" ">"
    ${If} $R2 == ""
      DetailPrint "$(PLURY_MSI_NOT_MSI)"
      Goto plury_hklm_done
    ${EndIf}
    DetailPrint "$(PLURY_MSI_FOUND)"
    MessageBox MB_YESNO|MB_ICONQUESTION "$(PLURY_MSI_ASK)" /SD IDNO IDYES plury_hklm_remove
    DetailPrint "$(PLURY_MSI_LEFT)"
    Goto plury_hklm_done
  plury_hklm_remove:
    ClearErrors
    ; msiexec runs unelevated and asks for consent itself.
    ExecWait 'msiexec.exe /x $R5 /passive /norestart' $R2
    ${If} ${Errors}
      StrCpy $R2 "$(PLURY_NOT_STARTED)"
    ${EndIf}
    ; 0: removed. 3010: removed, restart pending. 1602: the user declined.
    DetailPrint "$(PLURY_MSI_EXITED)"
  plury_hklm_done:

  Pop $9
  Pop $R5
  Pop $R4
  Pop $R3
  Pop $R2
  Pop $R1
  Pop $R0
!macroend

; ---- End of the MaeclyPrint takeover ---------------------------------------

!macro NSIS_HOOK_POSTINSTALL
  Push $R8
  !insertmacro PLURYPRINT_MOVE_LOADED_DLL_ASIDE

  ; The bundle installed both DLLs under resources\shellext; the one Explorer
  ; loads sits next to pluryprint.exe, because the DLL finds the exe it
  ; launches through its own path (shell-ext-win/src/module.rs).
  ${If} ${IsNativeARM64}
    StrCpy $R8 "arm64"
  ${Else}
    StrCpy $R8 "x64"
  ${EndIf}
  ClearErrors
  CopyFiles /SILENT "$INSTDIR\resources\shellext\$R8\${PLURYPRINT_SHELLEXT_DLL}" "$INSTDIR\${PLURYPRINT_SHELLEXT_DLL}"
  ${If} ${Errors}
  ${OrIfNot} ${FileExists} "$INSTDIR\${PLURYPRINT_SHELLEXT_DLL}"
    DetailPrint "This installer carries no $R8 context-menu DLL; the Explorer menu is not registered"
    Goto pluryprint_register_done
  ${EndIf}
  DetailPrint "Registering the $R8 Explorer context menu"

  ; The in-process COM server, per user.
  WriteRegStr HKCU "Software\Classes\CLSID\${PLURYPRINT_SHELLEXT_CLSID}" "" "Pluryprint context menu"
  WriteRegStr HKCU "Software\Classes\CLSID\${PLURYPRINT_SHELLEXT_CLSID}\InprocServer32" "" "$INSTDIR\${PLURYPRINT_SHELLEXT_DLL}"
  WriteRegStr HKCU "Software\Classes\CLSID\${PLURYPRINT_SHELLEXT_CLSID}\InprocServer32" "ThreadingModel" "Apartment"

  ; The verb on every file and on folders. A COM verb has no 15-item limit.
  !insertmacro PLURYPRINT_REGISTER_VERB "*"
  !insertmacro PLURYPRINT_REGISTER_VERB "Directory"

  ; SHCNE_ASSOCCHANGED: Explorer re-reads the verbs without a restart.
  System::Call 'shell32::SHChangeNotify(i 0x08000000, i 0, p 0, p 0)'
  pluryprint_register_done:
  Pop $R8
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  DeleteRegKey HKCU "Software\Classes\*\shell\${PLURYPRINT_SHELLEXT_VERB}"
  DeleteRegKey HKCU "Software\Classes\Directory\shell\${PLURYPRINT_SHELLEXT_VERB}"
  DeleteRegKey HKCU "Software\Classes\CLSID\${PLURYPRINT_SHELLEXT_CLSID}"
  System::Call 'shell32::SHChangeNotify(i 0x08000000, i 0, p 0, p 0)'
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  ; Still loaded by Explorer: removed at the next restart instead.
  Delete /REBOOTOK "$INSTDIR\${PLURYPRINT_SHELLEXT_DLL}"
  Delete /REBOOTOK "$INSTDIR\${PLURYPRINT_SHELLEXT_DLL}.old*"
  !insertmacro PLURY_REMOVE_SUMATRA_SETTINGS "$INSTDIR"
  RMDir /REBOOTOK "$INSTDIR"
!macroend
