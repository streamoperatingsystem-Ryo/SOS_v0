# Morphing (bulge/pinch)

> Section déplacée depuis `AGENTS.md` — les règles figées (CONTRAT/RÈGLE/anti-patterns) restent dans `AGENTS.md`.


## Morphing (bulge/pinch) — widgets média + fond de scène (2026-09-12)

> Concept repris de MorphLens (projet externe, code NON porté) : clic sur une
> zone → agrandir/rétrécir avec rayon + intensité. Réécrit pour StreamOS en
> **morphs non destructifs** (liste de paramètres persistée, jamais baked dans
> les pixels).

### Principe

```
morphs: [{ x, y, rayon, intensite, mode }]  (normalisés 0-1, ordre = cumul)
   │ rendu : média → offscreen 2D (fit/zoom/rot existants) → shader WebGL
   │         (bulge/pinch : échantillonnage plus près/loin du centre, courbe
   │          hermite) → canvas visible
   ├─ Widget média : dashboard (Widget.svelte, one-shot vignette) + diffusion
   │   (diffusion.html, rAF pour vidéos / one-shot images)
   └─ Fond : dashboard (Canvas.svelte) + diffusion (peindreFond → GL → drawImage
       → trous destination-out inchangés)
```

### Règles

1. **ACTIVATION PARESSEUSE** : contexte GL créé SEULEMENT si le média a des
   morphs. Sans morph → rendu natif img/video (zéro coût — clips, interactions
   et le reste de l'app jamais impactés). WebGL indisponible → fallback rendu
   natif (log, pas de crash).
2. **Non destructif** : le média source n'est jamais modifié — le rendu
   réapplique la chaîne de morphs dans l'ordre. Reset = vide la liste.
3. **Coordonnées normalisées (0-1)** de la zone visible → survivent au resize,
   au changement de scène, au reload.
4. **Moteur GL en DEUX exemplaires synchronisés** : `src/lib/morph/gl-morph.ts`
   (dashboard, module TS) + moteur inliné dans `diffusion.html` (vanilla JS —
   la page est autonome sans Tauri). **Garder les deux synchronisés** (shaders
   identiques).
5. **Mode morphing** (toggle sidebar) : clic sur un widget média = bulge/pinch
   au point cliqué (pas de drag) ; clic sur le canvas vide = morph du FOND.
   Params du prochain morph : stores/morph.ts (intensité 10-200%, rayon 5-50%,
   sens agrandir/rétrécir).
6. **Persistance** : `morphs` sur Widget, `bgMorphs` sur Scene (JSON scène,
   commit + snapshot WS comme les autres champs).

### Fichiers

| Fichier | Rôle |
|---|---|
| `src-tauri/src/scene.rs` | `MorphPoint` + `morphs` (Widget) + `bgMorphs` (Scene) |
| `src/lib/morph/gl-morph.ts` | Moteur GL dashboard + `dessinerFitCanvas` (object-fit canvas partagé) |
| `src/lib/stores/morph.ts` | Params UI du prochain morph (mode/sens/intensité/rayon) |
| `src/lib/stores/scene.ts` | `ajouterMorphWidget` / `resetMorphsWidget` / `ajouterMorphFond` / `resetMorphsFond` |
| `src/lib/components/Widget.svelte` | Rendu canvas morphé widget + clic-pour-morpher |
| `src/lib/components/Canvas.svelte` | Rendu canvas morphé fond + clic fond |
| `src-tauri/resources/diffusion.html` | Moteur GL inliné + rendu widgets morphés (rAF vidéos) + fond morphé dans peindreFond |
| `src/lib/components/Toolbar.svelte` | Blocs Morphing (section Widgets pour le média, section Arrière-plan pour le fond) |
