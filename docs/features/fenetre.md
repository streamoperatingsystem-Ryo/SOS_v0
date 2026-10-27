# Fenêtre principale — écran + mode

> Section déplacée depuis `AGENTS.md` — les règles figées (CONTRAT/RÈGLE/anti-patterns) restent dans `AGENTS.md`.


## Fenêtre principale — écran + mode au démarrage (2026-10-04)

> Section « Ergonomie » de la Toolbar : choix du moniteur de démarrage +
> mode d'affichage (fenêtré / maximisé / fullscreen borderless). Appliqué
> **immédiatement** (la fenêtre saute en direct) ET persisté pour le
> prochain boot — même pattern que `raccourcis_appliquer`.

- **Persistance** : `fenetre.json` (`config.rs::FenetreConfig`) —
  `{ monitor_index, mode, geo? }`. `mode` ∈ `"fenetre"|"maximise"|
  "fullscreen"` (`String` normalisé par `mode_valide`, défaut `"maximise"`
  = comportement historique → migration douce, fichier absent = défauts).
  `geo` = dernière géométrie fenêtrée en **px physiques**
  (`outer_position` + `inner_size` — restaurée par `set_position` +
  `set_size`, pas de dérive de décorations) ; `None` → centrage 1280×800
  (= taille `tauri.conf.json`).
- **Application** (`fenetre.rs::appliquer`) : normalisation AVANT le
  déplacement — `set_fullscreen(false)` + `unmaximize()` (une fenêtre
  maximisée/fullscreen ne se déplace pas par `set_position`), puis pose
  dans le rect du moniteur cible (`maximize()`/`set_fullscreen(true)`
  s'appliquent sur l'écran contenant la fenêtre). Index moniteur hors
  bornes (écran débranché) → primaire, même règle que raccourcis.
  `geo_intersecte` rejete une géométrie sauvée sur un autre écran
  (pointerait hors écran) → repli centrage.
- **Sauvegarde de `geo`** : recapturée par `capturer_geo` (no-op si
  fullscreen/maximisé — jamais de pollution) à trois moments : hook
  `CloseRequested` de la fenêtre main (enregistré dans
  `appliquer_au_boot`), `app_arreter`/`app_redemarrer` (exit()/restart()
  ne déclenchent pas forcément CloseRequested), et chaque
  `fenetre_appliquer` (post-apply si mode fenêtré, pré-apply sinon).
- **Anti-flash boot + splash (2026-10-06)** : `"visible": false` dans
  `tauri.conf.json` → `appliquer_au_boot` (setup) positionne/mode SANS
  `show()`. Le splash `streamos-splash://localhost/` (fenêtre « splash »
  440×280 topmost centrée créée par `splash::demarrer` en tête de setup —
  wallpaper Windows via `SPI_GETDESKWALLPAPER` + mascotte + « Bienvenue
  <pseudo Windows> » traduit via langue.json) couvre le chargement.
  `splash::armer_affichage` montre `main` + ferme le splash à
  max(4 s de splash, event `frontend:pret` émis par App.svelte en fin
  d'onMount) — garde-fou : timeout 8 s supplémentaires si le frontend ne
  répond pas (jamais de fenêtre invisible définitive, splash ≤ ~12 s).
  `show()`/`close()` idempotents (HMR, double appel = no-op). Wallpaper
  indisponible (couleur unie/fichier illisible) → 404 → dégradé CSS,
  jamais bloquant. **Dev = prod** (2026-10-26) : le splash est la même
  petite fenêtre éphémère dans les deux modes et se ferme après le même
  timer — plus de fenêtre splash persistante en dev.
- **Frontend** : sélecteur moniteur **partagé** avec la section
  Raccourcis (`raccourcisMoniteurs` / `monsOptions` — la commande
  `raccourcis_moniteurs` liste les écrans, pas de doublon). 3 boutons
  `.action.small` façon `.bord-row` + `<select>` moniteur + hint i18n.

| Fichier | Rôle |
|---|---|
| `src-tauri/src/fenetre.rs` | Commandes `fenetre_etat`/`fenetre_appliquer`, `appliquer_au_boot`, `sauver_geo`/`capturer_geo`, `geo_intersecte` |
| `src-tauri/src/config.rs` | `FenetreConfig`/`FenetreGeo` + lire/sauver `fenetre.json` |
| `src-tauri/src/lib.rs` | `mod fenetre`/`mod splash`, handler, `appliquer_au_boot` + `splash::demarrer`/`armer_affichage` dans setup, protocole `streamos-splash`, `sauver_geo` dans `app_arreter`/`app_redemarrer` |
| `src-tauri/src/splash.rs` | `demarrer` (fenêtre splash), `repondre` (routes `/`, `/wallpaper`, `/mascotte`), `armer_affichage` (transition splash→main) |
| `src-tauri/resources/splash.html` | Page du splash (placeholders `__USER__`/`__GREETING__`) |
| `src-tauri/tauri.conf.json` | `"visible": false` (anti-flash — show() différé par `splash::armer_affichage` sur `frontend:pret`) |
| `src/lib/tauri.ts` | `ModeFenetre`/`FenetreEtat` + `fenetreEtat`/`fenetreAppliquer` |
| `src/lib/stores/fenetre.ts` | `fenetreMonitorIndex`/`fenetreMode` + `initFenetre`/`appliquerFenetre` |
| `src/lib/components/Toolbar.svelte` | Contrôles écran+mode dans la section Ergonomie |
| `src/App.svelte` | `initFenetre()` au boot + `emit("frontend:pret")` en fin d'onMount |
| `src/lib/i18n/{fr,en,de,es,it,ru,zh,ja}.json` | `toolbar.mode.{fenetre,maximise,fullscreen}` + `toolbar.demarrageHint` |
