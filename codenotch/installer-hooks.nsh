; Code Center keeps the former com.immidi.codenotch identity and data directory, but Tauri's
; NSIS installer keys upgrades by productName. Detect the final Codenotch release explicitly so
; the rename replaces it instead of creating a second installed app. Tauri's silent uninstaller
; leaves %APPDATA%\codenotch untouched unless the user explicitly selects "Delete app data".
!macro NSIS_HOOK_PREINSTALL
  ReadRegStr $R7 HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\Codenotch" "UninstallString"
  StrCmp $R7 "" code_center_migration_done

  IfSilent code_center_migration_run 0
  MessageBox MB_OKCANCEL|MB_ICONINFORMATION \
    "Codenotch has been renamed to Code Center.$\r$\n$\r$\nSetup will remove the old app before installing Code Center. Your settings and provider data will stay in place." \
    IDOK code_center_migration_run IDCANCEL code_center_migration_cancel

code_center_migration_cancel:
  Abort

code_center_migration_run:
  ; The old app may still be in the tray. Closing it prevents locked-file failures.
  nsExec::ExecToLog 'taskkill /F /IM codenotch.exe'
  ExecWait '$R7 /S' $R8
  IntCmp $R8 0 code_center_migration_done
  MessageBox MB_OK|MB_ICONSTOP \
    "Setup could not remove the previous Codenotch installation (exit code $R8). Nothing from Code Center has been installed. Close Codenotch and try again."
  Abort

code_center_migration_done:
!macroend
