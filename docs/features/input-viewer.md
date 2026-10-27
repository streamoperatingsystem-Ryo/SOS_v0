# Input Viewer — capture globale

> Section déplacée depuis `AGENTS.md` — les règles figées (CONTRAT/RÈGLE/anti-patterns) restent dans `AGENTS.md`.


## Input Viewer — capture globale clavier + souris + manette (2026-10-03)

> Port du concept "Input Viewer" de RUST_SOS_2026 (E:\SteamOs - All Version\RUST_SOS_2026v0.1,
> features/input_viewer.rs + capture_clavier.rs). Widget de scène qui affiche
> les entrées en temps réel côté diffusion. Modes : **clavier** (AZERTY/QWERTY),
> **souris**, **numpad**, **manette** (gilrs → mapping standard Gamepad).

### Flux

```
Clavier (poll GetAsyncKeyState 60Hz, thread Rust) + Souris (hook WH_MOUSE_LL)
  → thread moteur : état central (touches BTreeSet + souris) + throttle 30Hz
  → chat_tx.send({"type":"input-viewer-etat","etat":{"touches":[...],"souris":{...}}})
  → serveur :4321 → diffusion.html
  → maj DOM directe (classList.toggle('presse') sur [data-touche]) — zéro re-render
```

- **Rust** (`input_viewer.rs`) : `InputViewerState` (manage, inactif au boot).
  - Poll clavier 60Hz (256 vkCodes, fronts down/up seulement) + mapping
    `vk_vers_code_html` (vkCode → KeyboardEvent.code, table portée de
    capture_clavier.rs). Modificateurs génériques 0x10/0x11/0x12 ignorés
    (variantes L/R 0xA0-0xA5 à la place).
  - Souris : hook `WH_MOUSE_LL` installé dans un thread dédié avec sa boucle
    de messages (GetMessageW bloquant — zéro CPU au repos) ; boutons
    immédiats, position throttlée 50ms. Le hook est installé ET désinstallé
    sur le MÊME thread (leçon ancienne app) ; arrêt via
    `PostThreadMessageW(WM_QUIT)`.
  - Publication WS : throttle 30Hz + skip si signature identique (le CEF
    d'OBS saccade si bombardé).
  - **Bouton ON/OFF** dans les options du widget (Toolbar) → commandes
    `input_viewer_set_actif` / `input_viewer_etat_actif`. Zéro thread/hook
    quand OFF.
  - **Auto-stop (2026-10-03)** : la capture est GLOBALE (pas liée à la
    scène). `syncInputViewerCapture()` (scene.ts) coupe la capture quand la
    scène chargée n'a AUCUN widget `input-viewer` — appelé dans `loadScene`
    (couvre openScene/createScene/deleteScene/duplicateScene/importScene/
    presets) et `deleteWidget` (suppression du dernier widget). Jamais
    d'auto-START : le bouton ON/OFF reste le choix de l'utilisateur.
- **Widget de scène** `type: "input-viewer"` : champs `inputMode`
  ("clavier"|"souris"|"numpad"|"manette"), `inputLayout`
  ("azerty"|"qwerty"), `inputSkin` ("xbox"|"ps"|"8bitdo", manette seulement),
  `inputCouleur` (couleur presse, défaut #8b5cf6), `inputMapping` (remap
  boutons manette + inversion D-pad). Créé via Toolbar →
  `createInputViewerWidget()` (500×220). Placeholder dashboard « Input
  viewer ». Cadre SVG automatique (applyCadreWidget).
- **Diffusion** (`diffusion.html`) : branche renderScene `input-viewer` —
  markup généré UNE FOIS (recréé si `ivKey` = mode|layout|skin change), maj
  d'état par `classList.toggle('presse')` sur les éléments qui changent
  seulement (union ancien/nouveau). WS `input-viewer-etat` →
  `majInputViewerEtat` (touches `[data-touche]`/`[data-touche-alt]`, souris
  `[data-souris]` + X/Y, manette `[data-bouton]`/`[data-dpad]`/`[data-stick-indicateur]`).

### Skins input-viewer — SVG réaliste (2026-10-03)

> Toutes les manettes et la souris sont des **SVG inline** générés par
> `ivGenererManette8BitDoSVG` / `ivGenererManetteXboxSVG` /
> `ivGenererManettePsSVG` / `ivGenererSourisHTML` ; clavier et numpad gardent
> leurs touches DOM (layout dynamique) posées sur un châssis SVG
> (`ivChassisHTML` : plaque + plateau encaissé + câble + LEDs CAPS/NUM/SCRL
> qui s'allument via `data-touche`). Ancien markup DOM de la manette
> supprimé (boutons verre génériques + labels `::after`).

- **Contrat pressé intact** : `.presse` est togglée sur des `<g>` SVG
  (`classList` fonctionne sur SVG) ; les fills de base sont des gradients
  `fill="url(#ivg*)"`, la règle CSS `.presse` les écrase (attribut de
  présentation < CSS). Glow = `filter: drop-shadow` (pas de
  outline/box-shadow en SVG). Labels = `<text>` (`::after` ne marche pas en
  SVG) ; couleurs par classes `iv-fig-lbl-*` (Xbox lettres / PS symboles).
- **Sticks SVG** : `<g class="iv-fig-stick" data-stick-indicateur data-bouton>`
  — le cap bouge (axes) ET s'illumine au clic L3/R3. Branche dédiée dans
  `majInputViewerEtat` : `translate(px)` en unités viewBox (le `calc(-50%+%)`
  DOM ne s'applique pas aux éléments SVG).
- **Boutons bonus** : home=`data-bouton="16"`, touchpad PS=`17`.
- **Responsive** : `viewBox` + `preserveAspectRatio="xMidYMid meet"` partout
  (sauf le châssis clavier, volontairement étiré `none`).
- **8BitDo fallback** : `data-skin="8bitdo"` sur `.iv-manette` → si D-pad
  inactif, axes LeftStick lus comme D-pad (émulation DualShock4).

### Pièges (leçons ancienne app + v0)

1. **Hook WH_MOUSE_LL** : SetWindowsHookExW exige que la boucle de messages
   tourne sur le MÊME thread que l'installation ; la désinstallation aussi.
   Thread dédié : install → GetMessageW (bloquant, zéro CPU) → unhook. Arrêt
   propre via `PostThreadMessageW(thread_id, WM_QUIT)`.
2. **Throttle souris 20Hz** sur les déplacements purs (boutons toujours
   immédiats) — sinon la diffusion OBS saccade (leçon ancienne app).
3. **Throttle WS 30Hz + skip si signature identique** — le CEF d'OBS décode
   la vidéo en logiciel : chaque message WS inutile affame le décodeur.
4. **Maj DOM directe** (classList.toggle) — jamais de innerHTML par event.
5. **Modificateurs** : poller les variantes L/R (0xA0-0xA5), jamais les
   génériques 0x10/0x11/0x12 (sinon Shift gauche/droite indiscernables).

### Fichiers

| Fichier | Rôle |
|---|---|
| `src-tauri/src/input_viewer.rs` | Capture : poll clavier 60Hz (table vkCode→KeyboardEvent.code portée), hook souris LL, état central, publication WS throttlée 30Hz |
| `src-tauri/src/lib.rs` | `InputViewerState` + commandes `input_viewer_set_actif` / `input_viewer_etat_actif` |
| `src-tauri/src/scene.rs` | Champs Widget `inputMode` / `inputLayout` / `inputSkin` / `inputCouleur` / `inputMapping` |
| `src/lib/stores/scene.ts` | `createInputViewerWidget()` + `setInputViewerConfig()` |
| `src/lib/components/CarteEdition.svelte` | Options input-viewer (mode/layout/skin/couleur + ON/OFF capture) |
| `src/lib/components/Widget.svelte` | Placeholder « Input viewer » (dashboard = config, pas un player) |
| `src-tauri/resources/diffusion.html` | 6 rendus : clavier/numpad (touches DOM + châssis SVG `ivChassisHTML`), souris SVG, manettes SVG (8bitdo/xbox/ps) + maj DOM directe (WS `input-viewer-etat`) |
