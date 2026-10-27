# Import média — MKV remuxé en MP4

> Section déplacée depuis `AGENTS.md` — les règles figées (CONTRAT/RÈGLE/anti-patterns) restent dans `AGENTS.md`.


## Import média — MKV remuxé en MP4 via sidecar ffmpeg (2026-09-14)

> Chromium (dashboard + source navigateur OBS) ne lit **pas** le MKV dans un
> `<video>` — seul MP4 (H.264) et WebM le sont. MKV et WebM partagent les magic
> bytes EBML (`1A 45 DF A3`) → `check_magic_video` accepte déjà le MKV ; seul le
> filtre d'extension bloquait `.mkv`.

### Flux

```
Dialog fichier (mp4/webm/mkv + images) → validate_and_copy_media (lib.rs)
  ├─ extension + taille (≤80 Mo) + magic bytes
  ├─ MKV → remux_to_mp4 : sidecar ffmpeg `-c copy -sn -movflags +faststart`
  │        → medias/<uuid>.mp4 (sans réencodage, rapide)
  └─ MP4/WebM → fs::copy → medias/<uuid>.<ext>
```

- `VIDEO_EXT = ["mp4", "webm", "mkv"]` (lib.rs). Le kind retourné est `"video"`.
- Le chemin stocké en scène finit en `.mp4` (même pour un MKV importé) → le
  frontend `kindFromMedia` (VIDEO_EXT `["mp4","webm"]`) retourne `"video"`
  naturellement. **Aucun changement frontend** — on ne propage pas "mkv".
- Sidecar ffmpeg embarqué : `tauri-plugin-shell` + `externalBin` dans
  `tauri.conf.json` (`binaries/ffmpeg`). Binaire à
  `src-tauri/binaries/ffmpeg-x86_64-pc-windows-msvc.exe` (target triple
  Windows x64). **Exclu de git** (`.gitignore` `src-tauri/binaries/`) — chaque
  env dev/CI doit le fournir (build statique gyan.dev ffmpeg-release-essentials).
- Permission : `shell:allow-execute` pour `binaries/ffmpeg` (capabilities).
- `remux_to_mp4` est blocking sync, exécuté via les commandes d'import
  `async` + `spawn_blocking` (pool bloquant Tokio — plus sur le thread UI ;
  avant 2026-09-27 elles étaient `fn` sync = thread principal → gel
  « Ne répond plus »). Remux `-c copy` d'un fichier ≤80 Mo prend quelques
  secondes (I/O bound).

### Limitations

- **Codecs non-MP4** : `-c copy` échoue si un flux n'est pas compatible MP4
  (ex. HEVC vidéo, audio Opus hors profil). L'erreur ffmpeg est retournée
  telle quelle (pas de crash). Pas de fallback de transcodage (choix : remux
  seul). Un fallback `-c:v copy -c:a aac` (transcode audio uniquement) pourrait
  être ajouté ultérieurement si besoin.
- **Sous-titres** : `-sn` les dropped (compatibilité MP4/browser).
- **Binaire absent** : message clair « ffmpeg sidecar indisponible ».
