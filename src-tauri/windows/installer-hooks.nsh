; StreamOS v0 — Hooks NSIS pour l'installer (setup-base0004).
;
; NSIS_HOOK_PREINSTALL : runs before copying files, after the PageReinstall
; page (which already offered to uninstall the old app version).
;
; Wipe silencieux du dossier %APPDATA%/com.streamos.v0/ (données de toute
; install précédente : scènes, médias importés, kick.json, tiktok.json,
; configs). Garantit un fresh install aux utilisateurs à qui le setup.exe
; est partagé — aucune donnée résiduelle de l'ancienne version n'est reprise.
;
; Les tokens OAuth2 Twitch/YouTube sont stockés dans le keyring OS (coffre
; Windows), PAS dans APPDATA → ils ne sont PAS affectés par ce wipe.
;
; Les autres hooks (POSTINSTALL, PREUNINSTALL, POSTUNINSTALL) sont laissés
; vides — ils existent juste pour que le fichier soit complet.

!macro NSIS_HOOK_PREINSTALL
  ; $APPDATA = C:\Users\<user>\AppData\Roaming (currentUser install mode).
  ; Si le dossier com.streamos.v0 n'existe pas, on ne fait rien (silencieux).
  IfFileExists "$APPDATA\com.streamos.v0\*.*" 0 skip_data_cleanup

    ; Wipe silencieux (sans MessageBox) : fresh install garanti.
    DetailPrint "Suppression des donnees precedentes : $APPDATA\com.streamos.v0"
    RMDir /r "$APPDATA\com.streamos.v0"

  skip_data_cleanup:
!macroend

!macro NSIS_HOOK_POSTINSTALL
!macroend

!macro NSIS_HOOK_PREUNINSTALL
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
!macroend
