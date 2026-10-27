# Barre de raccourcis — fenêtre TOPMOST

> Section déplacée depuis `AGENTS.md` — les règles figées (CONTRAT/RÈGLE/anti-patterns) restent dans `AGENTS.md`.


## Barre de raccourcis — fenêtre TOPMOST dockée à un bord d'écran (2026-09-19)

> Fenêtre `WebviewWindow` dédiée (label `raccourcis`, `raccourcis.rs` +
> `resources/raccourcis.html` via custom protocol `streamos-raccourcis://`) :
> bande noire translucide `rgba(0,0,0,0.7)` collée à un bord d'écran, toujours
> au-dessus, icônes d'actions du dashboard. **AUCUNE logique métier dans la
> barre** — chaque clic émet `raccourcis:action` {id} vers le dashboard.

### Contrats figés

1. **TOPMOST ≠ plein écran exclusif.** `always_on_top` place la fenêtre dans la
   bande topmost Win32 ; un jeu en **plein écran exclusif** (DirectX flip)
   recouvre TOUT — la barre disparaît, c'est accepté. Cible = **fenêtré sans
   bordure (borderless) + bureau**. JAMAIS d'overlay injecté (DLL, DX/VK) —
   interdit, certains anti-cheat le sanctionnent.
2. **Ré-armement topmost** : un jeu borderless lui aussi topmost peut passer
   devant. Timer tokio 2s → `SetWindowPos(HWND_TOPMOST, SWP_NOMOVE|NOSIZE|
   NOACTIVATE)` (z-order public Win32, cross-thread sûr). La tâche meurt avec
   la fenêtre (check `get_webview_window` à chaque tick).
3. **Flags fenêtre** : `decorations(false)`, `resizable(false)`,
   `always_on_top(true)`, `skip_taskbar(true)`, `focusable(false)` (= ne vole
   pas le focus au jeu ; les clics souris restent reçus), `transparent(true)`
   (sinon `rgba(0,0,0,0.7)` composite sur fond opaque → gris),
   `visible(false)` → géométrie physique → `show()`.
4. **Géométrie physique** : épaisseur 56px logiques × `scale_factor`, rect
   moniteur COMPLET (pas work_area — la barre recouvre la taskbar si même
   bord). `monitor_index` hors bornes (écran débranché) → primaire.
5. **`withGlobalTauri: true`** dans tauri.conf.json : REQUIS — le HTML vanilla
   de la barre utilise `__TAURI__.event.emit/listen` (contrairement au pop-out
   chat qui passe par le WS :4321 sans IPC).
6. **Orientation** : `?bord=` dans l'URL à la création + `emit_to("raccourcis",
   "raccourcis:bord", bord)` sur ré-application → bascule colonne (G/D) ↔
   rangée (H/B) dans le HTML.

### Commandes (`raccourcis.rs`)

| Commande | Rôle |
|----------|------|
| `raccourcis_moniteurs` | Liste moniteurs (index, nom, résolution, primaire) pour le `<select>` Toolbar |
| `raccourcis_etat` | `{visible, bord, monitor_index}` — visible = fenêtre réellement ouverte |
| `raccourcis_appliquer` | Crée/montre la barre dockée + persiste `raccourcis.json`. **Async obligatoire** (deadlock Webview2 en sync) |
| `raccourcis_masquer` | Détruit la fenêtre + `visible:false` (bord/moniteur conservés) |

### Ids d'actions (`raccourcis:action` → dispatch App.svelte)

- Widgets : `widget-media`, `widget-chat`, `widget-camera` (garde singleton
  caméra identique à la Toolbar), `widget-input-viewer`, `widget-speedrun`
- Connexions (toggle selon `connexions`) : `connexion-twitch`,
  `connexion-kick`, `connexion-youtube`, `connexion-tiktok`, `connexion-obs`.
  Kick/TikTok sans slug/username sauvegardé → `openSectionExplicit("connexions")`.
  **OBS = one-shot** (pas de déconnexion) : clic = `obsConnect` +
  `sceneSyncCaptures` (re-sync, comme le bouton « Connecter »).
- Modales : `modale-interactions`, `modale-moderation`, `modale-cadres`
- Écran d'attente : `ecran-attente` (toggle lancer/arrêter — pastille verte
  `data-plat="attente"` alimentée par `raccourcis:connexions.attente`)
- Just Chatting : `just-chatting` (toggle overlay — pastille verte
  `data-plat="justchatting"` alimentée par `raccourcis:connexions.justchatting`)
- Système : `systeme-masquer`

### Événements

- `raccourcis:action` {id} — barre → dashboard (dispatch).
- `raccourcis:ready` — barre → dashboard : handshake, la barre demande l'état
  des connexions (pastilles vertes) après init de ses listeners.
- `raccourcis:connexions` {twitch,youtube,kick,tiktok,obs} — dashboard → barre
  (`emitTo`), poussé par `$effect` sur `connexions`/`obsStatus` + au `ready`.
- `raccourcis:bord` — Rust → barre : nouvelle orientation sans recréer.
- `raccourcis-closed` — Rust → dashboard : fermeture externe (sync Toolbar).

Frontend : `stores/raccourcis.ts` (visible/bord/monitorIndex/moniteurs +
init/appliquer/masquer), bloc en queue de la section « Ergonomie » de la
Toolbar (show/hide, 4 bords, `<select>` moniteurs, hint borderless —
l'ancienne section accordéon « Raccourcis » a été supprimée, son contenu
migré tel quel dans Ergonomie).
