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

; The DLL is built for the installer's architecture by
; scripts/build-shell-ext.ps1, which copies it next to this file. A missing DLL
; stops the installer build instead of shipping without the context menu.
; Resolved here, at include time: inside a macro ${__FILEDIR__} would name the
; generated installer.nsi's folder instead of this file's.
!define PLURYPRINT_SHELLEXT_SRC "${__FILEDIR__}\${PLURYPRINT_SHELLEXT_DLL}"
!if /FileExists "${PLURYPRINT_SHELLEXT_SRC}"
!else
  !error "src-tauri\windows\${PLURYPRINT_SHELLEXT_DLL} is missing: run scripts\build-shell-ext.ps1 before packaging"
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
      DetailPrint "Cannot tell where ${PLURY_LEGACY_PRODUCT} is installed; leaving it in place"
      StrCpy $R3 0
    ${ElseIf} ${FileExists} "$R1\uninstall.exe"
      DetailPrint "Removing the previous ${PLURY_LEGACY_PRODUCT} install in $R1"
      ClearErrors
      ExecWait '"$R1\uninstall.exe" /S _?=$R1' $R2
      ${If} ${Errors}
        StrCpy $R2 "not started"
      ${EndIf}
      DetailPrint "${PLURY_LEGACY_PRODUCT} uninstaller exited with $R2"
      ${If} $R2 == 0
      ${AndIfNot} ${FileExists} "$R1\${PLURY_LEGACY_EXE}"
        ; Run in place, the uninstaller cannot delete itself or its folder.
        Delete "$R1\uninstall.exe"
        RMDir "$R1"
      ${Else}
        DetailPrint "${PLURY_LEGACY_PRODUCT} was not removed; it stays in Apps & features"
        StrCpy $R3 0
      ${EndIf}
    ${Else}
      ; Registered, but its uninstaller is gone: only the registration is left.
      DetailPrint "${PLURY_LEGACY_PRODUCT} uninstaller not found in $R1; removing its stale registration"
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
      DetailPrint "A per-machine ${PLURY_LEGACY_PRODUCT} install ($R5) was found but is not an MSI; remove it from Apps & features"
      Goto plury_hklm_done
    ${EndIf}
    DetailPrint "Found a per-machine ${PLURY_LEGACY_PRODUCT} install ($R5)"
    MessageBox MB_YESNO|MB_ICONQUESTION "${PLURY_LEGACY_PRODUCT} is also installed for all users of this computer. Remove it now? Windows will ask for administrator approval.$\n$\nYour settings, profiles and queue are kept either way." /SD IDNO IDYES plury_hklm_remove
    DetailPrint "Left the per-machine ${PLURY_LEGACY_PRODUCT} install in place; remove it from Apps & features"
    Goto plury_hklm_done
  plury_hklm_remove:
    ClearErrors
    ; msiexec runs unelevated and asks for consent itself.
    ExecWait 'msiexec.exe /x $R5 /passive /norestart' $R2
    ${If} ${Errors}
      StrCpy $R2 "not started"
    ${EndIf}
    ; 0: removed. 3010: removed, restart pending. 1602: the user declined.
    DetailPrint "${PLURY_LEGACY_PRODUCT} (per machine) uninstall exited with $R2"
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
  !insertmacro PLURYPRINT_MOVE_LOADED_DLL_ASIDE
  SetOutPath "$INSTDIR"
  File "${PLURYPRINT_SHELLEXT_SRC}"

  ; The in-process COM server, per user.
  WriteRegStr HKCU "Software\Classes\CLSID\${PLURYPRINT_SHELLEXT_CLSID}" "" "Pluryprint context menu"
  WriteRegStr HKCU "Software\Classes\CLSID\${PLURYPRINT_SHELLEXT_CLSID}\InprocServer32" "" "$INSTDIR\${PLURYPRINT_SHELLEXT_DLL}"
  WriteRegStr HKCU "Software\Classes\CLSID\${PLURYPRINT_SHELLEXT_CLSID}\InprocServer32" "ThreadingModel" "Apartment"

  ; The verb on every file and on folders. A COM verb has no 15-item limit.
  !insertmacro PLURYPRINT_REGISTER_VERB "*"
  !insertmacro PLURYPRINT_REGISTER_VERB "Directory"

  ; SHCNE_ASSOCCHANGED: Explorer re-reads the verbs without a restart.
  System::Call 'shell32::SHChangeNotify(i 0x08000000, i 0, p 0, p 0)'
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
  RMDir /REBOOTOK "$INSTDIR"
!macroend
