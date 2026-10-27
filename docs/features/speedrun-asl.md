# Handlers ASL + hot-reload + sauvegarde PB

> Section déplacée depuis `AGENTS.md` — les règles figées (CONTRAT/RÈGLE/anti-patterns) restent dans `AGENTS.md`.


## Handlers ASL + hot-reload + sauvegarde PB (2026-10-09)

> Trois correctifs moteur issus du retour utilisateur « les runs se
> mélangent / bloqué au 2e split après 3-4 parties alors que le timer
> tourne / changer de catégorie ou de splitter doit être pris en compte ».

- **Handlers ASL `onStart`/`onSplit`/`onReset`/`exit`** : parsés et
  compilés depuis le début mais JAMAIS invoqués — or LiveSplit les
  déclenche sur les transitions du timer, et beaucoup de scripts y
  réinitialisent leur état interne (`vars`). Sans `onReset`, l'état
  persistait entre les runs → `split()` ne se déclenche plus après
  quelques parties. `appeler_handler` (asl_engine.rs) les appelle sur
  chaque transition : start (ASL + manuel), split (`faire_split`),
  reset (ASL, manuel, mort du processus — `exit()` AVANT le teardown des
  watchers, `onReset` ensuite). Non-fatal : erreurs JS log-warnées.
- **Hot-reload config** : commandes moteur internes `Resync` et
  `SetLssPath { path, total_splits }` (asl_engine.rs). Charger un AUTRE
  .lss pendant que la boucle tourne (autre catégorie du même jeu, autre
  splitter) → reset complet de la tentative : timer neuf avec le
  NOUVEAU total, `temps_segs`/`cumuls_segs` vidés, processus décroché →
  re-attach + re-init (watchers reconstruits) + event `reset`. Recharger
  le MÊME chemin (event `saved` → relecture PB) → AUCUN reset, la run en
  cours continue. Charger un .asl à chaud → `resync()` (total du .lss
  conservé). `speedrun_demarrer` pousse aussi le chemin .lss au moteur
  (au boot le moteur est créé APRÈS le premier `speedrun_charger_lss`).
- **Sauvegarde PB automatique** : à la fin d'une run (`ended`), si la
  run bat le PB du .lss (comparaison des cumuls finaux — la base de
  temps du timer, `is_game_time_initialized()`, doit correspondre à la
  colonne PB du .lss : game time si le PB n'a pas de RealTime, sinon
  refus), `lss::maj_pb` réécrit les `<SplitTime name="Personal Best">`
  de chaque segment de façon CHIRURGICALE (AttemptHistory, Best
  Segments et métadonnées préservés ; balise temps créée si le SplitTime
  est self-closing ; .tmp + rename avec fallback Windows). Gardes : un
  segment skippé → PAS de sauvegarde (ne jamais effacer un PB) ; nombre
  de segments ≠ cumuls → refus. Event `saved { chemin }` → le store
  recharge le .lss en silence (PB affichés rafraîchis). Cumuls exacts
  via `cumuls_segs` (primary au moment du split — téléscope exact même
  après un skip, contrairement à une somme des durées). Choix : sauvegarde
  AUTO sans fenêtre de confirmation.

> **Sujet suivant (non implémenté)** : à la sortie du jeu détectée par l'ASL,
> proposer à l'utilisateur de sauver ou non ses splits (nom + dossier) —
> aujourd'hui la sauvegarde PB est automatique à la fin d'une run gagnante ;
> l'ASL serait chargé par layout (3 layouts → 3 jeux, le switch recharge).

### Disposition « ligne » horizontale (2026-09-29)

> Façon LiveSplit horizontal : champ widget `srLayout` (`"auto"` | `"colonne"`
> | `"ligne"`, Option serde — absent = auto). En **auto**, le splitter bascule
> en ligne quand `hauteur < ~4×srTaillePolice` (même seuil des deux côtés ;
> fallbacks taille = 16 diffusion / 27 dashboard, identiques aux fontSize
> appliqués). En ligne : `.sr-ligne` toggle sur `.widget.speedrun` /
> `.speedrun-widget` → `flex-direction: row`, header/meta/barre masqués,
> splits en scroll horizontal (cellules empilées nom / delta+temps via
> `flex-wrap`, index masqué), timer épinglé à droite. Le seul JS dépendant
> de l'axe est le scroll auto vers le split actif (`scrollTop`↔`scrollLeft`)
> dans `srMajSegmentsVisuel` (diffusion) et le `$effect` de `WidgetSpeedrun`.

### Bugs corrigés (portés depuis le legacy, déjà fixés dans le code source)

1. **vars/settings persistants** — `globalThis.__asl_vars` / `__asl_settings` en
   mémoire Boa (sinon fonctions/instances détruites par JSON round-trip).
2. **strip .exe** — Process::find_by_name matche "mgsi" (ASL) contre "mgsi.exe".
3. **new_state_var prepend 0** — LiveSplit InitializeOffsets : deref base en premier.
4. **module vide → module principal** — pas process_name (RCA-1).
5. **gameTime Option** — fallback realTime (RTA) si null.
6. **init guard** — ne pas appeler start/split/reset si init a échoué.
7. **transpiler char-safe** — char_indices() pas bytes[i] (panics multi-byte).
8. **Mutex non réentrant** (AGENTS.md) — singleton AslEngine : lire UNE fois.
9. **Boa Context non Send** — ScriptContext vit dans un seul thread (mpsc channels).
10. **Throttle WS** — skip si signature identique (CEF OBS saccade sinon).
11. **Maj DOM directe** diffusion.html — jamais innerHTML par event.

### Fichiers

| Fichier | Rôle |
|---|---|
| `src-tauri/src/speedrun/mod.rs` | Déclaration modules + cfg(windows) + stubs non-Windows |
| `src-tauri/src/speedrun/commands.rs` | 11 commandes Tauri + singleton AslEngine + emit (Tauri + WS) |
| `src-tauri/src/speedrun/asl/parser.rs` | Parser .asl manuel (states + 13 méthodes) |
| `src-tauri/src/speedrun/asl/lss.rs` | Parser .lss XML (segments + PB) |
| `src-tauri/src/speedrun/engine/timer.rs` | TimerModel (start/split/reset/pause/undo/skip) |
| `src-tauri/src/speedrun/engine/asl_engine.rs` | Boucle polling 15Hz + emit events |
| `src-tauri/src/speedrun/engine/transpiler.rs` | Transpileur C#→JS (29 étapes) |
| `src-tauri/src/speedrun/engine/script_bridge.rs` | ScriptContext Boa + API compat C# |
| `src-tauri/src/speedrun/engine/pont_memoire.rs` | Pont JS↔Rust thread_local |
| `src-tauri/src/speedrun/memory/` | Scan Win32 (process, deep_pointer, memory_watcher, signature_scanner) |
| `src-tauri/src/config.rs` | Persistance speedrun.json (chemins + settings) |
| `src-tauri/src/scene.rs` | Type Widget "speedrun" + champs speedrunCheminAsl/Lss/Settings |
| `src/lib/stores/speedrun.ts` | Store TS (écoute event Tauri + actions invoke) |
| `src/lib/components/WidgetSpeedrun.svelte` | Composant dashboard (timer + contrôles + LSS) |
| `src-tauri/resources/diffusion.html` | Rendu diffusion (branche speedrun + WS speedrun-etat + majSpeedrunEtat) |

### Plateforme

- **Windows** : scan mémoire Win32 (ReadProcessMemory) + Boa + timer manuel + LSS.
- **Non-Windows** : stubs renvoient erreur explicite (pas de crash). Timer manuel
  + LSS fonctionnent partout (pas de scan mémoire).
