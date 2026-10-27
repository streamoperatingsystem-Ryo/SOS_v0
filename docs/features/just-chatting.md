# Just Chatting — overlay non-destructif

> Section déplacée depuis `AGENTS.md` — les règles figées (CONTRAT/RÈGLE/anti-patterns) restent dans `AGENTS.md`.


## Just Chatting — overlay non-destructif caméra + chat (2026-10-04)

> Mode bascule « scene IRL » : en diffusion, seuls la caméra et le(s) chat(s)
> restent visibles, repositionnés sur un layout calculé. **La scène réelle
> n'est JAMAIS mutée** — ni `commitScene`, ni `update_scene`, ni JSON. Pas de
> titre au-dessus de la caméra, pas de zoom fond (écarts assumés vs la
> version React). `scene.cadreWidget` reste actif (règle figée cadres).

### État — runtime, OFF au boot, non persisté

`just_chatting.rs` (pattern `ecran_attente` sans timer ni JSON) :
`JustChattingState { actif: Mutex<bool>, derniere_frame, chat_tx, app }`.
Commandes `just_chatting_etat` / `just_chatting_set_actif` ; frame WS
`just-chatting-etat` `{etat:{actif, paysage, portrait}}` sur `chat_tx` +
event `just-chatting:etat` → dashboard. **Resync** : la diffusion envoie
`just-chatting-resync` au `ws.onopen` → `server.rs` → `reemit()` —
différence volontaire avec `ecran_attente::reemit` : **jamais un no-op**,
une frame `actif:false` est reconstruite si aucune n'a été émise (une
Browser Source connectée avant le premier toggle doit recevoir l'état).

### Formule de layout — unique, en Rust (fractions 0–1)

La frame porte les DEUX variantes (`paysage` + `portrait`) ; la diffusion et
le store choisissent selon `canvasW >= canvasH` puis × canvas réel en px.
Paysage : `m=0.04, g=0.02, h=0.78, y=(1-h)/2, inner=1-2m-g`, caméra
`x=m, w=0.62*inner`, chat `x=m+wCam+g, w=0.38*inner` (même hauteur/centre).
Portrait : mêmes marges 0.04 sur les 4 côtés, empilés, caméra 62 % de la
hauteur utile au-dessus. Plusieurs chats → tous au même rect. Les fractions
du preset embarqué `presets/just-chatting.json` NE SONT PAS utilisées (le
preset reste un modèle de scène distinct).

### Dérivation — deux copies indépendantes, même résultat

- **Diffusion** (`diffusion.html`, `jcDeriverScene` en tête de
  `renderScene`) : copie `{...scene, widgets: caméra+chats aux rects JC}` —
  le sweep `seen` retire le DOM des widgets filtrés, `peindreFond` ne perce
  plus leurs trous, `#sos-cam-clip` suit le rect JC. Aucun rendu dédié.
- **OBS** (`stores/justChatting.ts` → `sceneDerivee`) : copie complète
  identique + **`bgObsSource: undefined`** (sinon la capture plein canvas
  resterait affichée) passée à `scene_sync_captures` via le paramètre
  optionnel `scene: Option<Scene>` de la commande (absent = scène chargée,
  comportement historique). `sync_scene_captures` (contrat unique dans
  `obs_trou.rs`) : caméra `ensure_camera` au rect JC, `SOS-Trou-*`/`SOS-Fond-*`
  absents de la copie → `SetSceneItemEnabled false`. Jamais `RemoveInput`,
  jamais d'écriture (`state.scene` ni disque).

### Toggle (dashboard, non-fatal OBS offline)

- **ON** : `setActif` → store rafraîchi AVANT les syncs →
  `sceneSyncCaptures(copie dérivée)` + `cameraSync(null, rect JC)` +
  `cameraMaskSync(points cadre, jcW, jcH, "remplir", 1)` — le masque PNG
  dépend des DIMS du widget (sync_scene_captures cale le transform, pas le
  masque). Pas de widget caméra → skip cameraSync/masque, masquage OK.
- **OFF** : `syncCapturesOBS()` (scène réelle — réactive trous + fond,
  recale la caméra) + **`syncCameraMask(true)` obligatoire** : le cache
  `cleMasqueCamera` détient encore la clé réelle alors que le PNG OBS est
  aux dims JC → sans `forcer`, le rétablissement serait sauté.
- **Ordre critique au OFF** : `jcActif` doit être à `false` AVANT
  `syncCapturesOBS` — sinon le hook post-sync renverrait la copie dérivée
  et le mode ne se couperait pas dans OBS.

### Hook post-sync captures (`scene.ts` — `registerApresSyncCaptures`)

Enregistré par `initJustChatting`, exécuté à la fin de `syncCapturesOBS`
(openScene/duplicate/preset/boot/reconnexion OBS) **ET** de `commitScene` :
si JC actif → `sceneSyncCaptures(copie dérivée)` re-cache les trous de la
nouvelle scène et recale `SOS-Caméra`/`SOS-Caméra-Fond` au rect JC
(`obsSyncTrous` repositionne les items sans jamais toucher
`sceneItemEnabled`, mais il remet la caméra au rect réel à chaque commit).
**Un hook n'appelle JAMAIS `syncCapturesOBS`** (re-enchaînement → boucle) —
wrapper Tauri direct uniquement. Coût assumé en v1 : un one-shot OBS à
chaque pointerup tant que le mode est actif (documenté, pas optimisé).

### UI

Toolbar : section toggle directe `header output-btn` + `.dot` (état) ;
topbar : pillule `.indicateur-btn` visible si `jcActif`, clic = coupe ;
raccourcis : `data-id="just-chatting"` `data-plat="justchatting"`.
i18n : `toolbar.justChatting` + `toolbar.justChattingTitle` (5 dicts).
