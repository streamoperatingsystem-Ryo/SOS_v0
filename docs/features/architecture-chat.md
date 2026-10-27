# Architecture chat — flux des messages

> Section déplacée depuis `AGENTS.md` — les règles figées (CONTRAT/RÈGLE/anti-patterns) restent dans `AGENTS.md`.


## Architecture chat — flux des messages

```
Plateforme (IRC/WS/HTTP) → Rust (twitch_chat/kick/tiktok_chat/youtube_chat)
  → app.emit("chat:message", ChatMessage)     → frontend Svelte (chat widget)
  → chat_tx.send(json)                        → serveur :4321 (diffusion OBS)
```

`ChatMessage` struct (dans `twitch_chat.rs`) :
```rust
struct ChatMessage {
    plateforme: String,  // "twitch" | "youtube" | "kick" | "tiktok"
    pseudo: String,
    texte: String,
    badges: Option<...>,
    avatar: Option<String>,
    color: Option<String>,      // tag IRCv3 color (Twitch)
    message_id: Option<String>, // suppression Helix (Twitch)
    emotes: Option<Vec<EmoteSeg>>, // emotes Twitch (officielles + tierces)
}
```

**Rendu diffusion incrémental (2026-09-22)** : un nouveau message n'ajoute
qu'UN nœud DOM par widget (`appendChatMessage` → `creerNoeudChat`), plus
rebuild des 200 messages à chaque message. Le trim du buffer (MAX_CHAT=200)
retire les nœuds correspondants dans chaque widget filtré. `fillChatBody`
(rebuild complet) reste le chemin des changements rares : création du widget,
changement de filtre/taille/cadre (le border-image inline suit le gradient
du cadre — CEF ne recalcule pas les var() en dynamique).

**Emotes Twitch (twitch_emotes.rs)** : au démarrage de la tâche IRC, une
tâche parallèle charge les listes BTTV/7TV/FFZ (global + chaîne, APIs
publiques sans auth — 404 = chaîne non inscrite, non-fatal) dans un
`Arc<RwLock<CacheEmotes>>`. Par PRIVMSG, `construire_segments` produit des
`EmoteSeg { debut, fin, url, nom }` : les emotes **officielles** viennent du
tag IRC `emotes` (positions en **codepoints Unicode**, converties en UTF-16
pour `String.slice` JS — fin exclusive) ; les **tierces** sont matchées par
mot dans les zones non couvertes (priorité BTTV > 7TV > FFZ, case-sensitive,
ponctuation collée acceptée). `diffusion.html` et `chat-popout.html` rendent
`<img class="emote">` entre texte échappé (URL contrainte à `https://`,
XSS-safe). Les émojis Unicode passent en texte brut (transport UTF-8 intact,
fallback police OS).

## Commande générique `!cmd` — liste des commandes viewer actives

`!cmd` est géré par `commandes.rs` (`CMD_LISTE`, nom **réservé** dans tous
les modules : commandes perso, musique, arène, cadres). Il liste uniquement
les commandes **viewer actives** :

- commandes perso actives (`commandes_config.json`, `actif: true`) ;
- musique active : `!rs`, `!retirer`, `!file` (`!skip` est mods → exclu) ;
- arène active : `!fight`, `!accept`, `!refuse`, `!a`, `!b`, `!c`, `!ff`,
  `!fight aide` (`!fightstop` est mods → exclu) ;
- cadres actif : `!cadres` ;
- `!cmd` lui-même.

Deux rendus :

1. **Overlay** (`cmd-liste` sur le WS :4321, `#cmd-liste-root` dans
   `diffusion.html`, z 9993) : carte groupée multi-ligne, 12 s — seule
   source possible sur Kick/YouTube/TikTok (aucun canal d'envoi).
2. **Chat Twitch** : chunks d'une ligne ≤ 440 caractères (préfixe
   `(i/N)`), postés via `TwitchState::envoyer_message_chat` — la liste
   complète passe même si elle dépasse la limite d'un message.

Cooldown global 10 s (`dernier_global`, clé interne `__cmd`). Le nom `cmd`
est refusé par `commandes_set_config`, `fight_set_config`,
`musique_set_config` et `cadres_chat_set_config`.
