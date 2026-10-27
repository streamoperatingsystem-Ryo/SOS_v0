# Section Prévisualisation — projecteur OBS

> Section déplacée depuis `AGENTS.md` — les règles figées (CONTRAT/RÈGLE/anti-patterns) restent dans `AGENTS.md`.


## Section Prévisualisation — miniature live + projecteur OBS (2026-10-06)

Section **accordéon standard** de la barre latérale (`.sec-preview`,
carte jaune `.cible-preview`, section `"preview"` dans `ui.ts`) : pliée =
tout démonté (l'iframe est détruite → zéro coût, c'est le lazy-loading) ;
ouverte = miniature + sélecteur écran + bouton projecteur. Deux fonctions :

- **Miniature live** : `<iframe src="http://127.0.0.1:4321/?preview">`
  rendue aux dims du canvas scène puis réduite par `transform: scale()`
  dans une boîte `aspect-ratio` (~224 px de large). `pointer-events:none`
  sur l'iframe : le clic monte au bouton → ouvre le projecteur OBS.
  `previewMonIdx` (écran choisi) persisté en `localStorage` ; les moniteurs
  OBS sont relus à l'ouverture de la section ($effect).
- **`?preview` dans `diffusion.html`** : const `PREVIEW`
  (`URLSearchParams`). Tout le son coupé (alertes, welcome, présentation,
  commande, pad, écran d'attente) et vidéos **persistantes figées**
  (widgets média, fond, média-fond — `currentTime` posé pour afficher la
  bonne frame). Le reste reste live (chat, alertes visuelles, JC, écran
  d'attente). Sans le param (source OBS), la page est **inchangée**.
  ⚠️ La miniature ne montre que la couche overlay : caméra et captures
  OBS sont des trous dans la page (rendues par OBS sous la source
  navigateur — le vrai rendu = le projecteur OBS).
- **Projecteur OBS** (équivalent du clic droit OBS → « Ouvrir le
  projecteur (prévisualisation) ») : obs-websocket
  `OpenVideoMixProjector` (`videoMixType: OBS_WEBSOCKET_VIDEO_MIX_TYPE_PREVIEW` — l'enum
  côté websocket est préfixé `OBS_WEBSOCKET_VIDEO_MIX_TYPE_*`, PAS
  `OBS_VIDEOMIX_TYPE_*` → "invalid enum value" sinon),
  `monitorIndex` choisi, **-1 = fenêtré**). Le select des écrans vient de
  `GetMonitorList` — les indices sont **ceux d'OBS**, pas ceux de
  `raccourcis_moniteurs` (Win32). One-shot comme tout `obs.rs` :
  connect → requête → close. Pas de suivi d'ouverture : fermer le
  projecteur se fait dans Windows/OBS. Après `result=true`, la nouvelle
  fenêtre projecteur est remontée au premier plan (poll ~2 s, la fenêtre
  arrive en différé sur le thread UI d'OBS).
- **Fenêtres projecteur = titre + obs64.exe** : les fenêtres Qt d'OBS
  partagent la classe générique `Qt<ver>QWindowIcon` (PAS "OBSProjector")
  → `hwnds_projecteurs()` matche titre contenant "project" + exe
  obs64.exe. `fermer_projecteurs()` envoie WM_CLOSE à chacune.
- **Par défaut au boot = projecteur fermé** : OBS restaure ses projecteurs
  d'une session à l'autre → `obsConnect` appelle `obs_projecteur_fermer`
  à chaque connexion réussie (fermeture active, pas d'option). Non-fatal
  et silencieux.
- Chaînage : `stores/obs.ts` (`obsMoniteurs`, `ouvrirProjecteurPreview`
  — credentials via `obsCreds()`) → `tauri.ts` → commandes
  `obs_projecteur_moniteurs` / `obs_projecteur_preview` /
  `obs_projecteur_fermer` (lib.rs) → `moniteurs_liste` /
  `ouvrir_projecteur_preview` / `fermer_projecteurs` (obs.rs).
