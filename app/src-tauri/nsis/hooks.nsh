; Tauri NSIS installer hooks (docs/V0.1.1.md B3).
; Explain the data lifecycle at uninstall time: notes are user documents stored
; beside the user's files (hidden .nuggets folders), so the uninstaller leaves
; them in place by design. Point at the in-app "Delete all notes" for a clean
; wipe. /SD IDOK keeps silent uninstalls (/S) non-interactive.

!macro NSIS_HOOK_PREUNINSTALL
  MessageBox MB_OK|MB_ICONINFORMATION "Your Tofu Nuggets notes are saved beside your files (hidden .nuggets folders) and will be LEFT IN PLACE.$\r$\n$\r$\nTo remove all your notes as well, open Tofu Nuggets and use Settings > Delete all notes before uninstalling." /SD IDOK
!macroend
