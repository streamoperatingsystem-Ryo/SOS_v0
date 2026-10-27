# Fond de scène = capture OBS — SOS-Fond-*

> Section déplacée depuis `AGENTS.md` — les règles figées (CONTRAT/RÈGLE/anti-patterns) restent dans `AGENTS.md`.


## Fond de scène = capture OBS (app/jeu) — SOS-Fond-* (2026-09-21)

> Le fond de l'application peut être une CAPTURE OBS (fenêtre ou jeu) plein
> canvas au lieu d'un média — « comme un widget trou, mais pour le fond ».

### Mécanisme

```
CarteEdition (fond) → ObsSourceDialog mode "fond" (fenêtre/jeu/écran seulement)
  → lierFondCapture(kind, target) (scene.ts)
      → obsCreateTrouFromPc("SOS-Fond-<sceneId8>", kind, target, 0,0,canvasW,canvasH)
        (create_trou_from_pc REUTILISÉ tel quel : CreateInput/reuse + transform + reorder)
  → scene.bgObsSource = <nom> + commitScene
        → obsSyncTrous : item plein canvas (fit "etirer")
OBS pile (bas → haut) : SOS-Fond-* < SOS-Trou-* < SOS-Caméra-Fond < SOS-Caméra < SOS-Diffusion
diffusion.html : renderBg — bgObsSource PRIORITAIRE sur bgMedia → même chemin
  que « pas de fond » (page transparente → la capture apparaît)
```

- La page :4321 est transparente (`html, body { background: transparent }`) —
  le canvas `#bg-canvas` est la seule couche opaque. Ne pas peindre = voir la
  capture en bas de pile. `peindreFond` n'a PAS besoin de changement (early
  return via `!bgState.mediaEl`).
- **Résolution adaptative + skip-frame (perf, 2026-09-22)** : le backing
  store de `#bg-canvas` suit la taille réelle de la Browser Source
  (`window.innerWidth/Height`, plafonné à 1×) — `bgScaleX/Y` projettent les
  coords scène via `setTransform` dans `peindreFond` (tous les calculs restent
  en pixels scène : fit, offsets, trous, clip-paths). La boucle rAF ne
  repeint que si `mediaEl.currentTime` a changé (pause/stall = zéro coût ;
  les changements de paramètres repeignent via le chemin snapshot).
- **Champ scène `bgObsSource`** (scene.rs + tauri.ts, serde default —
  rétro-compat) : présence = fond capture, PRIORITAIRE sur bgMedia (le média
  reste stocké, non peint ; « Retirer la capture » le restaure).
- **Source PAR SCÈNE** `SOS-Fond-<sceneId8>` (pas singleton — 2 scènes = 2
  fenêtres capturées différentes). `sync_scene_captures` (signature `&Scene`)
  : item == scene.bgObsSource → enable + transform plein canvas
  (0,0,canvasW,canvasH, STRETCH, alignment 5) ; les SOS-Fond-* des autres
  scènes → SetSceneItemEnabled false (orphan hiding).
- **Suppression** : `retirerFondCapture` → RemoveInput (pattern widget trou —
  window/game n'ont pas de conflit d'accès exclusif). La caméra n'est PAS
  proposée en mode fond (conflit dshow avec un éventuel widget caméra).
- **Dashboard** : `Canvas.svelte` — `hasFond` inclut bgObsSource, pastille
  `.lecture-obs-fond` « Capture app/jeu en fond — rendue par OBS » (recette
  .msg-pill), morphable fond désactivé (rien à morpher).
- `commitScene` (scene.ts) : l'item fond plein canvas est poussé dans
  obsSyncTrous (x=0, y=0, fit "etirer" — sync_trous travaille en alignment
  centre, cx = canvasW/2 → couvre exactement le canvas).

### Fichiers

| Fichier | Rôle |
|---|---|
| `src-tauri/src/scene.rs` | Champ `bgObsSource: Option<String>` |
| `src-tauri/src/obs_trou.rs` | `reorder_sos_sources` (SOS-Fond-* bottom) + `sync_scene_captures` (&Scene, boucle SOS-Fond-*) |
| `src-tauri/src/lib.rs` | `scene_sync_captures` clone la scène complète |
| `src/lib/stores/scene.ts` | `lierFondCapture` / `retirerFondCapture` + item fond dans commitScene |
| `src/lib/components/ObsSourceDialog.svelte` | Prop `mode: "widget" \| "fond"` (état interne renommé `etape`) |
| `src/lib/components/CarteEdition.svelte` | Fond : boutons « Mettre une app/jeu en fond » / Changer / Retirer |
| `src/lib/components/Canvas.svelte` | `bgObsSource` derived + pastille + gate morphable |
| `src-tauri/resources/diffusion.html` | `renderBg` : `!rel \|\| scene.bgObsSource` → chemin transparent |
