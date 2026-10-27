# Emoji Arena — combat d'emojis 1v1 piloté par le chat

> Mini-jeu de chat : les viewers s'affrontent en tapant des commandes
> `!fight` / `!a` / `!b` / `!c` — les deux emojis combattants s'affichent
> en bas de la diffusion (gauche vs droite) avec barres de vie, timer de
> tour et dégâts flottants. Modes : viewer vs viewer (y compris
> multi-plateforme — Twitch vs Kick…) ou viewer vs BOT.
>
> **Découverte et feedback (2026-10-27)** : `!fight aide` affiche les règles
> (chat Twitch + carte overlay pour toutes les plateformes) **même quand
> l'arène est inactive** (avec un avertissement explicite), `!fight liste`
> rappelle le pool d'emojis, `!fight` pendant un lobby/combat renvoie un
> **statut contextuel** au lieu d'une erreur, et chaque événement important
> (cooldown, mauvais tour, lobby expiré, spécial déjà utilisé) pousse un
> **toast overlay** — seule source d'info sur Kick/YouTube/TikTok.
> La liste générique `!cmd` (commandes perso + modules actifs) est décrite
> dans `docs/features/architecture-chat.md`.

## Architecture

Même modèle que `musique.rs` + `commandes.rs` : moteur **autoritaire en
Rust**, diffusion = dumb renderer.

```
Chat → on_message(plateforme, pseudo, texte, est_mod) [fight.rs]
     → machine à états Idle → Lobby → Combat → Fin → Idle
     → frame WS `fight-state` (snapshot COMPLET à chaque transition)
     → diffusion.html #fight-root (z 9993 — SOUS pad/commandes/alertes)
```

- **`FightState`** (`src-tauri/src/fight.rs`) — `app.manage`, clonable
  (Arc internes). `Phase` non sérialisée → sérialisation manuelle dans
  `combat_json`.
- **Hooks chat** : `on_message` appelé dans les 4 modules chat juste après
  le hook musique (`twitch_chat.rs`, `youtube_chat.rs`, `tiktok_chat.rs`,
  `kick.rs`). `est_mod` calculé au call-site (badges par plateforme,
  mutualisé avec musique). Retour `Option<String>` = accusé posté en chat
  — **Twitch seul** a un canal de réponse (PRIVMSG) ; les autres
  plateformes ignorent le retour (l'overlay est la seule source d'info —
  les noms de commandes sont transportés dans la frame `cmds`).
- **Messages asynchrones Twitch** (annonce de tour, rappel 5 s, expiration
  de lobby, fin AFK) : `dire_chat` passe par
  `TwitchState::envoyer_message_chat` (même canal mpsc que `/clear`), car
  ces messages ne répondent pas à un `on_message`. `reponses_chat` les
  coupe aussi.
- **`info` dans la frame** : chaque snapshot `fight-state` porte
  `info: { seq, kind, texte, duree_ms, age_ms }` — dernier toast/carte
  d'aide, dédupliqué par `seq` et ignoré si `age_ms > duree_ms` (resync /
  reload OBS sans rejouer un vieux message). `kind` = `info` | `warn` |
  `ok` | `aide`.
- **Emoji** : validation par heuristique (1 séquence ≤ 16 codepoints, non
  ASCII, sans mot/URL) ; mémoire de session par viewer (cap 500) pour
  réutiliser le dernier emoji sans le retaper ; `!fight liste` affiche le
  pool. Un nom d'emote (`Kappa`) est refusé avec un toast explicite.
- **Identité joueur** : clé `"plateforme:pseudo_lower"` (ChatMessage n'a
  pas de user_id) — un combat Twitch vs Kick est possible.
- **Timers tokio gardés par génération** : `inner.gen` incrémenté à chaque
  mutation ; un timer en vol vérifie son gen au réveil → obsolète = no-op.
  Exception : le timer **match** compare son `fin_match` (Instant) au lieu
  du gen — la deadline match doit survivre aux mutations du combat alors
  que lobby/tour/fin/bot doivent être invalidés par toute action.
- **Resync** : upstream `fight-resync` à l'open du WS (server.rs) →
  `reemit()` repousse le snapshot — reload OBS mid-fight couvert. La
  frame `derniere.seq` déduplique l'animation d'action côté diffusion.

## Règles de jeu (constantes dans fight.rs)

| Coup | Commande défaut | Effet |
|---|---|---|
| Rapide | `!a` | 8-12 dégâts, 90 % toucher — sûr |
| Lourd | `!b` | 18-26 dégâts, 55 % toucher — risqué |
| Spécial | `!c` | vol de vie 10+10, 85 % toucher, **1 usage par combat** |

- Critique ×2 : 10 % (rapide/lourd seulement). PV 100 par défaut.
- Tour par tour strict, `duree_tour_s` = 15 s — **timeout = tour sauté**
  (anti-AFK, indispensable en live).
- **Match mort** : après `PASSES_AFK` = 3 passes consécutifs de **chaque**
  joueur, fin immédiate `afk` (« Match abandonné — inactivité »), sans
  cooldown. Une action réelle (même ratée) remet le compteur du joueur à 0.
- Match capé `duree_match_s` = 180 s → plus haut PV gagne (égalité sinon).
- Lobby expire après `duree_lobby_s` = 30 s (toast + message chat).
- Cooldown par viewer `cooldown_combat_s` = 120 s — appliqué **aux deux
  participants** (gagnant ET perdant — anti-rematch en boucle), **sauf**
  sur `stop` (arrêt de mod) et `afk` (décision 2026-10-27 : ne pas punir
  les joueurs pour une action de modération ou un match mort).
- BOT : délai simulé 1,5-3 s ; spécial si PV ≤ 50 % (60 % chance), sinon
  pondéré rapide 65 % / lourd 35 %.

## Commandes chat (toutes renommables dans l'onglet Arène)

| Commande | Rôle |
|---|---|
| `!fight` | défi ouvert · `!fight @pseudo` ciblé · `!fight bot` vs CPU · suffixe emoji optionnel (« !fight 🐲 ») · **pendant un lobby/combat : statut contextuel** |
| `!fight aide` | règles + liste des commandes (alias `help`, `regles`) — chat Twitch + carte overlay 12 s (version compacte pendant un combat) ; **disponible même arène inactive** (avertissement en tête) |
| `!fight liste` | pool d'emojis aléatoires (alias `emojis`) |
| `!accept` | rejoindre (ouvert : tout sauf le défiant ; ciblé : le pseudo visé) — emoji optionnel |
| `!refuse` | défi ciblé seulement |
| `!a` `!b` `!c` | coups du joueur actif — silence pour les non-joueurs ; un participant hors tour reçoit un toast throttlé |
| `!ff` | abandon (ou annuler son propre lobby) |
| `!fightstop` | arrêt forcé — mods/broadcaster (`est_mod`), **sans cooldown** |

**Sous-commandes réservées** : `aide`/`help`/`regles` et `liste`/`emojis`
sont interceptées **avant** le parse emoji — sinon `!fight aide` créerait
un lobby avec l'emoji « aide ».

**Anti-spam** : même commande d'un même viewer < 2 s ignorée (les coups
sont exclus — le tour les cadence) ; réponses de statut max 1/10 s ;
aide/liste max 1/30 s. Noms de commandes : 20 caractères max,
`[a-z0-9_-]` uniquement (protège l'affichage overlay).

**Anti-collision bidirectionnelle** : `commande_set_config` consulte
`fight.noms_reserves()` (refus création d'une `!cmd` du même nom) et
`fight_set_config` refuse un nom déjà pris par une commande chat ou par
la musique (`musique.noms_reserves()`).

**PRNG** : xorshift64 local (seed horloge) — pas de crate `rand`
(zéro dépendance ajoutée, déterminisme suffisant pour un jeu chat).

## Rendu diffusion (`diffusion.html`, `#fight-root`, z 9993)

- Deux zones ancrées bas-gauche / bas-droite : barre PV au-dessus
  (gradient vert→orange→rouge par paliers), emoji ~128 px, pseudo + badge
  plateforme. Zone B en `scaleX(-1)` (fait face).
- **Zone active** : halo doré, badge « À TOI », chips `⚡ !a` / `💥 !b` /
  `🩸 !c` (spécial grisé après usage) et barre de timer locale ; zone
  inactive à 55 %. Le centre affiche « Au tour de X · N s » + timer match.
- **Toasts / carte d'aide** : racine SÉPARÉE `#fight-info-root` (toujours
  visible quand l'arène repasse en idle), `textContent`, `white-space:
  pre-line`, dedup `seq`, expiration `age_ms`.
- **Lobby** : bannière avec l'emoji du défiant (72 px), la commande
  `!accept <emoji>` et une mini-règle « 3 coups · 15 s/tour · N min ».
- **Fin** : bannière résultat + KO visuel du perdant ; raison `afk` →
  « Match abandonné — inactivité », `stop` → « Combat arrêté ».
- Animations CSS pures : lunge attaquant, shake/esquive défenseur,
  popups dégâts/soins/crit, KO (chute + grayscale), idle (respiration sur
  `.fight-emoji-idle`), impacts `💥`/`💢`/`🩸`/`✨`, flash, micro-secousse
  de l'arène. `animations_reduites` coupe FX + secousse + idle (les popups
  de dégâts restent). Reflow-forcé pour rejouer deux coups identiques.
- **KO zone B** : keyframe dédiée `fight-ko-b` qui conserve `scaleX(-1)`
  (sinon l'adversaire se retourne en tombant).
- **Jamais innerHTML avec le pseudo** — `textContent` uniquement
  (le pseudo vient du chat).
- Countdowns locaux : la frame porte des deadlines ms, la diffusion
  décrémente (intervalle 250 ms) entre les frames.

## Dashboard

Onglet « Arène » de la modale Interactions Live (`FightTab.svelte`,
store `stores/fight.ts`, event `fight:update`) : toggle actif, 8 noms
de commandes, PV/durées/cooldown, carte de partie en direct (spécial
utilisé), boutons « Tester » (combat factice Streamy vs BOT — pipeline
réel) et « Forcer l'arrêt ».

**Message d'accueil** : bloc copiable généré avec les noms de commandes
**courants** (à épingler dans le chat / panneau de chaîne) + bouton
« Prévisualiser l'aide » (commande `fight_previsualiser_aide` → carte
overlay, sans chat).

**Options** : `reponses_chat` (Twitch), `annonce_tour` (annonce chaque
tour humain), `rappel_tour` (rappel à 5 s), `animations_reduites`
(FX/secousses/idle). Les annonces asynchrones ignorent les combattants
`local` du bouton « Tester » (pas de pollution du chat).

Persistance : `data/fight_config.json` (config seule — un combat en cours
ne survit pas au redémarrage). `actif` **false par défaut** (opt-in :
`!a`/`!b`/`!c` sont des noms très génériques).

## Décisions assumées

- **Pas de télémétrie** : `fight-state` n'est PAS whitelisté dans
  `telemetrie/collecte.rs` — c'est une frame d'état haute fréquence
  (chaque tour), pas un signal par événement ; la whitelist est fermée
  par design.
- **Pas de file de challengers** (v2 possible) — un seul combat à la
  fois, le lobby est le point d'entrée.
- **`!fightstop` et `afk` sans cooldown** (2026-10-27) — un arrêt de mod
  ou un match mort ne doit pas bloquer les viewers 120 s.
- **Fin AFK après 3 passes chacun** (2026-10-27) — un match où les deux
  joueurs sont AFK n'occupe plus l'overlay pendant 3 min.
- **Overlay éphémère** (z 9993) : l'arène est invisible hors combat —
  n'éclipse jamais la scène ; les alertes/commandes/pad restent au-dessus.
  Les **toasts/carte d'aide** vivent dans `#fight-info-root` (même z) et
  restent visibles quand l'arène repasse en idle.
- **Overlay FR uniquement** : `diffusion.html` n'est pas traduit (périmètre
  i18n v1 = dashboard). Les nouveaux textes d'aide/toast sont FR, comme le
  reste de l'overlay ; le dashboard reste i18n 8 langues.
- **Emoji natif + FX** (2026-10-27) : pas d'emotes animées BTTV/7TV pour
  l'instant — les glyphes système sont animés par CSS (idle, impacts,
  secousse) ; zéro réseau, zéro asset.
