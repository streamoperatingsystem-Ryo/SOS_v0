# Sélecteur de capture — miniatures WGC

> Section déplacée depuis `AGENTS.md` — les règles figées (CONTRAT/RÈGLE/anti-patterns) restent dans `AGENTS.md`.


## Sélecteur de capture — miniatures WGC + type « Écran » (2026-10-13)

> `ObsSourceDialog.svelte` liste les cibles PC en **grille de cartes**
> (vignette 16:10 + titre + sous-ligne + filtre texte), plus en liste
> texte. 4 types : caméra (widget seul — jamais en mode fond), fenêtre,
> jeu, **écran**.

- **Miniatures = Windows Graphics Capture** (`pc_thumb.rs`) — la MÊME API
  qu'OBS utilise pour window_capture/monitor_capture : la vignette montre
  ce qu'OBS captera réellement. `pc_window_thumbnail(hwnd)` /
  `pc_monitor_thumbnail(hmon)` → `CreateForWindow`/`CreateForMonitor` →
  frame pool D3D11 **free-threaded** (pas d'exigence MTA) → 1re frame
  (timeout 1,5 s) → staging texture CPU → downscale box ≤256 px → PNG
  (`mask_png::encoder_png_pixels`, deflate stored) → data URL. Device
  D3D11 + `IDirect3DDevice` WinRT créés UNE fois (`OnceLock`, fallback
  WARP si pas de GPU) ; sessions sérialisées par `WGC_LOCK` (les ~20
  invokes des cartes se succèdent). `CoInitializeEx(MTA)` par appel avec
  uninit seulement si S_OK.
- **Fallback icône fenêtre** : WGC échoue sur fenêtre minimisée / contenu
  protégé (DRM) → `WM_GETICON` (petit→grand) puis icône de classe
  (`GCLP_HICONSM`/`GCLP_HICON`) → DIB 32 bpp 48×48 → PNG. Une carte a
  TOUJOURS une image ; None = emoji côté front. Caméra : jamais de
  miniature (conflit d'accès exclusif dshow avec OBS — règle existante).
- **Énumération** (`pc_enum.rs`) : `WindowEntry` a gagné `hwnd` (isize —
  miniature à la demande). `MonitorEntry { index, name ("\\.\DISPLAYn"),
  monitor_id, hmon, primary, w, h }` via `EnumDisplayMonitors` ;
  `monitor_id` = `"\\?\DISPLAY#…#{guid}"` via `EnumDisplayDevices` +
  `EDD_GET_DEVICE_INTERFACE_NAME` — propriété `monitor_id` de
  `monitor_capture` OBS (fiable quel que soit l'ordre des écrans,
  contrairement à l'ancien index int "monitor").
- **`CaptureKind::Monitor`** (obs_trou.rs) → `monitor_capture`, target =
  `monitor_id` ; `kind_matches` : "monitor"/"display". Un monitor_id vide
  → target null → OBS capte l'écran par défaut.
- **Frontend** : grille `repeat(auto-fill, minmax(9.5rem, 1fr))`, tout en
  rem → scale 2K/4K automatique via tailleEcran. Miniatures à la demande
  par carte (clé = obs_value ; "" = pas d'image → emoji, absente =
  chargement pulsé). Tri alphabétique fenêtres/jeux. `maxWidth` 56 rem.
- **i18n** : `obsSourceDialog.kind.monitor` + `filtrer` + `ecranN`
  ({n} = index+1) + `principal` dans les 8 locales.
