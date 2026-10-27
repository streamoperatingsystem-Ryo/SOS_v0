# Compteur viewers — widget chat en diffusion

> Section déplacée depuis `AGENTS.md` — les règles figées (CONTRAT/RÈGLE/anti-patterns) restent dans `AGENTS.md`.


## Compteur viewers — widget chat en diffusion (2026-09-17)

> UNE seule tâche centralisée (`viewers.rs`) pour toutes les plateformes — PAS
> de boucle de polling par plateforme. Tick toutes les 15s ; chaque plateforme
> connectée est fetchée selon son propre rythme : **60s si en live, 180s si
> hors-ligne/erreur** (back-off). Max ~3 GET/min au total.

- **Twitch** : `twitch_helix::stream_viewers(user_id, access)` → `Option<u32>`.
- **YouTube** : `youtube_data::live_viewers(access, channel_id)` → `Option<u32>`.
- **Kick** : `kick::fetch_viewers(slug)` → `livestream.viewer_count` du même
  endpoint `api/v2/channels/<slug>` que `resolve_chatroom` (GET partagé via
  `get_channel`, headers anti-Cloudflare).
- **TikTok** : AUCUN polling — `RoomUserSeq` (push WS natif) est forwardé vers
  `chat_tx` dans `tiktok_chat.rs` avec dedup locale.

**Contrat WS** (sur `chat_tx` → diffusion :4321) :
`{"type":"viewers","plateforme":"twitch","count":123|null}` — `null` =
hors-ligne/déconnecté. Envoyé SEULEMENT quand le count change (dedup) ou qu'une
plateforme se déconnecte. Emit frontend : `viewers:update` `{plateforme,count}`.

**Diffusion** (`diffusion.html`) : `.chat-viewers` = barre en tête du widget
chat (flex column : barre + `.chat-body` flex:1). `:empty` → `display:none`
(aucun count = layout identique à avant). Widget `unifie` → une pastille
(logo + count compact « 1,2k ») par plateforme live ; widget filtré → seulement
la plateforme filtrée. `viewersParPlateforme` + `fillViewersBar` +
`rerenderViewersBars` (handler WS `type==="viewers"`).

**Pop-out** (`chat-popout.html`, 2026-10-20) : même `.chat-viewers` en tête
de fenêtre (sans filtre — « unifie », reste visible aussi en mode bulles :
chrome de la fenêtre, pas un message). **Resync** : les frames étant dedup'd,
le client envoie `{"type":"viewers-resync"}` sur `ws.onopen` → `server.rs`
appelle `viewers::reemit` qui rejoue les derniers counts de
`derniers_counts` (Mutex<HashMap> alimentée par `noter_count` dans
`publier`/`deconnecte` et `tiktok_chat` RoomUserSeq). Sans ça, un pop-out
ouvert en plein live attendait le prochain changement de count. TikTok émet
aussi `count:null` à la déconnexion depuis cette date (parité `deconnecte`
— avant, sa pastille restait affichée après déconnexion).
