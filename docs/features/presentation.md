# Présentation streamer — rideaux + bio

> Section déplacée depuis `AGENTS.md` — les règles figées (CONTRAT/RÈGLE/anti-patterns) restent dans `AGENTS.md`.


## Présentation streamer — rideaux + bio + clips + sons (2026-10-05)

> Portage de la feature « streamers favoris » de l'ancienne app React :
> présentation théâtrale d'un streamer (rideaux qui s'ouvrent, bio, infos
> chaîne, jeux récents, clips jouables, voix + musique). Déclenchement
> **manuel uniquement** depuis le dashboard — pas de trigger sur arrivée chat.

### Rust — `src-tauri/src/presentation.rs`

- **`PresentationState`** (tauri::State, `lib.rs`) : `chat_tx` + `favoris`
  (HashMap login→`FavoriConfig`, persisté `presentation_favoris.json` dans
  `config::data_dir`) + `courant` (`Option<PresentationEtat>`, RAM — une
  présentation ne survit pas au redémarrage, pas de resync diffusion).
- **`FavoriConfig`** : `user_id`, `login`, `display_name`, `avatar`,
  `son_voix`, `son_musique` (chemins `medias/...` validés par préfixe +
  existence), `mode` (`"theatrical"` | `"card"` — bascule à chaud via frame
  `presentation-mode` si le streamer est en cours d'affichage).
- **`lancer(user_id, mode, access)`** : `tokio::join!` des 4 fetchs Helix
  indépendants — `broadcaster` (fatal), `channel_info`, `stream_viewers`,
  `liste_clips` (gracieux) → dédup des `game_id` des clips → `game_info`
  (max 4 jeux récents) → frame `presentation-show` + `courant` + refresh
  du cache favori. `ClipInfo.game_id` (`#[serde(default)]`, additif) a été
  ajouté pour la déduction des jeux. `PresentationDonnees.banniere` =
  `offline_image_url` Helix (bannière/entête de chaîne, "" si absente) —
  `BroadcasterInfo.offline_image_url` mappé dans `twitch_helix.rs`.
- **Contrôles** : `fermer` (`presentation-hide`), `clip_jouer` (résolution
  MP4 signée au clic via `twitch_clips::resoudre_mp4` — les URL GQL
  expirent, jamais résolues à l'avance), `clip_stopper`, `audio_jouer` /
  `audio_stopper` (piste `voix`|`musique`, loop pour la musique).
- **12 commandes** `presentation_*` dans `lib.rs` : recherche
  (`search_channels` Helix), favoris CRUD + son + mode, lancer/fermer/
  clip/audio/etat. Émet `presentation:etat` (`{favoris, courant}`) au
  frontend à chaque mutation.

### Frames WS `chat_tx` → diffusion :4321

`presentation-show` (données complètes + `sons`), `presentation-hide`,
`presentation-mode`, `presentation-clip-play` (`{mp4_url,titre,duree_ms}`),
`presentation-clip-stop`, `presentation-audio-play` (`{piste,url,loop}`),
`presentation-audio-stop` (`{piste}` — ou toutes si absent).

### Diffusion (`diffusion.html`, vanilla JS — z-index 10000-10004)

- **Overlay autonome** (theatrical) : `.pres-voile` contient dans l'ordre
  `.pres-fond` (bannière de chaîne `donnees.banniere`, plein écran
  `object-fit:cover` + scrim dégradé `::after` — 1er enfant pour que tout
  le décor reste au-dessus ; masqué si bannière absente), orbes animés,
  `.pres-sol` (plancher), 3 `.pres-spot` (faisceaux clip-path balancés)
  et ~20 `.pres-poussiere` (particules en dérive).
- **Rideaux** `.pres-rideau` : plis larges 88 px + ourlet doré, ouverture
  `translateX(±27 %) scaleX(0.5)` en 1,2 s → les panneaux restent froncés
  en bordure (~220 px/côté), retenus par `.pres-embrasse` dorées (fade
  retardé 0,75 s — le `transition-delay` n'est posé que dans `.open`).
  `.pres-valance` : cantonné festonné permanent en haut (proscenium).
- **Contenu plaqué sur scène** : `.pres-carte` SANS panneau (plus de fond/
  bordure — lisibilité par text-shadows + scrim), 2 colonnes (profil/bio/
  stats/lien chaîne | EN DIRECT + jeu + jeux récents + grille 4 clips),
  max-width 73 % pour rester dans l'ouverture des rideaux.
- **Public** `.pres-public` (z 10003 — devant le contenu, sous le clip) :
  3 rangées de fauteuils bordeaux + têtes silhouettées générées par
  `construirePublic` (~15 % de places vides, balancement `pres-tete-sway`).
- **Mode carte** : `.pres-carte` compacte (colonne) — voile + fondu seuls,
  sans rideaux ni décor ni public.
- **Lecteur clip** : `<video>` MP4 plein écran (z 10004), contrôlé par
  `presentation-clip-play` / `presentation-clip-stop`. Pas d'iframe Twitch.
- **Audio** : `Audio()` voix (once) + musique (loop) servis depuis
  `/medias/...` sur :4321 — même plomberie que les sons alertes/pad.
- Esc/clic voile = fermeture locale uniquement (l'état Rust persiste).

### Dashboard — onglet « Présentation » (`InteractionViewerModal.svelte`)

3 sous-onglets : **Recherche** (`presentation_recherche` → résultats
Helix, ajout favori / présentation directe), **Favoris** (master-detail :
mode, sons voix/musique importés via `import_son` existant, retrait),
**Diffusion** (état courant, clip, stop sons/clip, fermeture). Store
`stores/presentation.ts` (`initPresentation` + `chargerPresentation`
appelés au boot dans `App.svelte`), wrappers `tauri.ts`. i18n
`interactions.presentation.*` + `tabPresentation` dans les 8 locales.

### Limites / hors scope (validé)

- Pas de déclenchement automatique à l'arrivée d'un streamer dans le chat.
- Pas de resync : une présentation lancée avant l'ouverture du navigateur
  source n'est pas rejouée (comportement voulu, état RAM).
- Aucun scope Twitch supplémentaire — tout passe par les helpers Helix
  existants et les scopes déjà en keyring.
