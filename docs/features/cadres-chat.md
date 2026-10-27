# Commande cadres — « !cadres <couleur> »

> Doc de feature — les règles figées restent dans `AGENTS.md`.

Un viewer tape `!cadres <couleur>` dans le chat : la **signature couleur
complète** du cadre unifié (`scene.cadreWidget`) bascule vers le duo
**noir + couleur viewer** du rendu par défaut (contrat rétabli le
2026-10-27 — voir l'encart ci-dessous) :

- `couleur` (base) : **`#000000`** — le « trait principal atténué » de
  `CadreConfig::defaut_scene` (`#000000` + `#8b5cf6`). C'est cette base
  noire qui préserve les **nuances d'épaisseur** : le halo
  (`strokeWidth × 2`, opacité 0.18) et la bande principale se confondent
  avec le fond, seuls les liserés de `stopsLiseres` (33 % et 68 %) restent
  visibles, et les encoches des biseaux restent des trous.
- `couleurFin` (vive) : la couleur choisie, brute — liserés + **TOUS** les
  ornements du cadre (équerres, arcs, graduations, circuits, motif des
  coins).

`strokeWidth`, `style`, `bordActif`, `animer` restent les réglages du
streamer. Le calcul vit dans `signature_couleurs` (cadres_cmd.rs) avec un
miroir EXACT `signatureCadresChat(hex)` (stores/cadresChat.ts — mêmes
valeurs — utilisé par les boutons test de la modale).

> ⚠️ **NE PAS RÉINTRODUIRE de base teintée** (2026-10-27) : la signature
> « base = couleur viewer × 0.65 » (2026-10-26 → 2026-10-27) rendait
> visible TOUTE la largeur du trait — halo 100 px à 0.18 + bande
> `strokeWidth` pleine + triangles de coin du motif `stripe-mg` — donc un
> cadre en **bloc de couleur unie** : plus de nuances d'épaisseur, plus
> d'encoches, très éloigné du rendu par défaut (comparatif violet vs rouge
> fourni par l'utilisateur). La justification d'origine (« la base
> #000000 écrasait le dégradé en monochrome ») est fausse pour MGStyle :
> la base noire est précisément ce qui donne les trois traits fins. Une
> couleur de palette sombre (`green` #008000, `navy`, `maroon`…) sert
> d'accent, pas de fond.

> **Convention uniformisée (2026-10-26)** : `couleurFin` = couleur VIVE /
> dominante du cadre, `couleur` = base / ton profond — valable pour TOUS
> les styles. Les 5 styles qui lisaient le vif sur `couleur` (cyberpunk,
> story, hexagon, arena, tribal) ont été alignés dans les 3 sites de rendu
> (`src/lib/cadres/`, `diffusion.html`, `chat-popout.html`). ⚠️ Les scènes
> existantes utilisant ces styles avec des couleurs personnalisées voient
> leurs deux couleurs permuter visuellement — le cadre par défaut (mgstyle)
> n'est pas touché.

## Chaîne

```
Chat (4 plateformes) → cadres_cmd.rs on_message(pseudo, texte)
  → match commande + actif + palette + cadre actif + cooldown
  → ScenesState.cadres_chat = CadresChatOverride { couleur: #000, couleurFin: hex }
  → scenes::push_snapshot → snapshot WS :4321     → diffusion re-render
```

- **Override éphémère, JAMAIS persisté (2026-10-26)** : la couleur viewer
  vit uniquement dans `ScenesState.cadres_chat` (RAM) et est injectée dans
  les snapshots WS par `snapshot_json` — sur une COPIE de la scène.
  `state.scene` et `scenes/<id>.json` gardent les couleurs du streamer :
  **elles priment au redémarrage** (contrat : la commande change le
  direct, pas la sauvegarde). **Zéro appel OBS** (couleur ≠ forme :
  clip-path et masque caméra inchangés).
- **Purges de l'override** : restart (rien n'est écrit), tout
  REMPLACEMENT de la scène courante (`purge_cadres_chat` dans ouvrir /
  creer_avec / supprimer / importer / preset / restauration zip — plus le
  filet `scene_id` de l'override), toute édition du cadre par le
  streamer (`update_scene` compare `cadreWidget` avant/après), et la
  DÉSACTIVATION de la commande (`set_config` avec `actif: false` → purge
  + `push_snapshot` : la couleur du streamer revient immédiatement —
  sans ça l'override survivait en RAM jusqu'au prochain changement de
  scène). Un commit streamer sans rapport (move d'un widget…) conserve
  l'override.
- **Dashboard = config** : la couleur viewer n'est PAS reflétée dans le
  dashboard (comme les alertes, l'état direct vit côté diffusion) — la
  carte cadre affiche toujours les couleurs du streamer.
- **Garde cadre actif** : scène sans cadre ou cadre inactif → refus
  (« aucun cadre actif »). Jamais de création/réactivation — un cadre
  explicitement désactivé n'est jamais rallumé (même règle que
  `sceneAvecWidget`).
- **Palette fermée** (~53 noms anglais → hex CSS, `PALETTE` dans
  cadres_cmd.rs) : pas de hex libre en chat. Exposée au dashboard via
  `cadres_chat_palette` pour la grille de l'onglet.
- **Cooldown global obligatoire 60 s** (`COOLDOWN_GLOBAL_S` — constante,
  volontairement non configurable : les cadres sont l'identité visuelle).
  Consommé APRÈS toutes les gardes (commande inconnue, palette, cadre
  actif) : un `!cadres` refusé ne grille pas les 60 s.
- **Accusé chat** : `on_message` retourne `Option<String>` posté en PRIVMSG
  sur Twitch uniquement (kick/youtube/tiktok n'ont pas de canal d'envoi —
  retour ignoré aux call-sites). `reponses_chat` (config) coupe l'accusé.
- **Réservation de nom** : `noms_reserves()` — `commandes.rs` (set_config +
  import), `fight.rs` et `cadres_cmd.rs` (set_config) refusent les
  collisions dans les deux sens, même mécanique que musique/fight.
- **Accusés commandes chat** : `commandes.rs on_message` retourne aussi
  `Option<String>` (« ✓ !cmd », y compris quand la commande part en file —
  le déclenchement est validé). Champ `reponses_chat` sur `CommandeConfig`,
  toggle dans le bloc « Comportement » du détail commande.

## Fichiers

| Fichier | Rôle |
|---|---|
| `src-tauri/src/cadres_cmd.rs` | État + config (`cadres_chat.json`) + palette + cooldown 60 s + set override `ScenesState.cadres_chat` + `push_snapshot` |
| `src-tauri/src/scenes.rs` | `CadresChatOverride` + injection `snapshot_json`/`snapshot_courant`/`push_snapshot` + `purge_cadres_chat` aux bascules de scène |
| `src-tauri/src/server.rs` | Snapshot initial WS via `snapshot_courant` (même injection) |
| `src-tauri/src/lib.rs` | `update_scene` : purge l'override si `cadreWidget` change (édition streamer) |
| `src-tauri/src/{twitch,kick,youtube,tiktok}_chat.rs` | Hook `on_message` à côté de fight ; PRIVMSG de l'accusé sur Twitch |
| `src-tauri/src/commandes.rs` | `on_message → Option<String>` accusé + `reponses_chat` + collision nom cadres |
| `src/lib/stores/cadresChat.ts` | Listener `cadres:update` + config + palette + `signatureCadresChat(hex)` (couleurFin=hex, couleur=#000000 — miroir de l'override Rust, boutons test seulement) |
| `src/lib/components/InteractionViewerModal.svelte` | Onglet Commandes → entrée rail « Cadres » : activation, nom, accusés, grille palette |
| `src/App.svelte` | `initCadresChat()` + `chargerCadresChat()` au boot |

UI : onglet Commandes → rail « Cadres » — toggle actif, nom de commande
renommable (défaut `cadres`), toggle accusés, rappel du cooldown 60 s, et
volet droit = grille visuelle des couleurs (chip + nom anglais à taper).
Chaque couleur a un bouton **▶ test / ■ stop** : le test applique la
signature complète via `signatureCadresChat` + `mettreAJourCadre` (même
chemin que la commande — la diffusion suit), le stop restaure les DEUX
champs d'origine (`couleur` + `couleurFin`) ; un test en cours est
restauré automatiquement à la fermeture de la modale. Boutons
désactivés sans cadre actif.
