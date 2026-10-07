; StreamOS v0 — Hooks NSIS pour l'installer.
;
; NSIS_HOOK_PREINSTALL : runs before copying files, after the PageReinstall
; page (which already offered to uninstall the old app version).
;
; Les données utilisateur dans %APPDATA%/com.streamos.v0/ (scènes, médias
; importés, configs) sont CONSERVÉES à l'installation : une mise à jour ne
; doit jamais détruire le travail de l'utilisateur. La rétro-compatibilité
; des formats est gérée par serde(default) + migrations dans le code
; (Scene::migrer_cadres / migrer_titres / migrer_ancien_config).
;
; Les tokens OAuth2 Twitch/YouTube sont stockés dans le keyring OS (coffre
; Windows), PAS dans APPDATA → indépendants du dossier de données.

!macro NSIS_HOOK_PREINSTALL
!macroend

!macro NSIS_HOOK_POSTINSTALL
  ; Langue par défaut du dashboard : anglais. langue.json est créé SEULEMENT
  ; s'il est absent (première install) — une mise à jour ne doit pas écraser
  ; la langue choisie par l'utilisateur. Au 1er boot, lire_langue retourne
  ; "en" (prioritaire sur navigator.language). Le sélecteur topbar permet
  ; de changer ensuite.
  IfFileExists "$APPDATA\com.streamos.v0\langue.json" skip_langue_create
    CreateDirectory "$APPDATA\com.streamos.v0"
    FileOpen $0 "$APPDATA\com.streamos.v0\langue.json" w
    FileWrite $0 '{"langue": "en"}'
    FileClose $0
  skip_langue_create:
!macroend

!macro NSIS_HOOK_PREUNINSTALL
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
!macroend
