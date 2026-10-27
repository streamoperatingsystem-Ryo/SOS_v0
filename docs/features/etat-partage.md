# État partagé (tauri::State)

> Section déplacée depuis `AGENTS.md` — les règles figées (CONTRAT/RÈGLE/anti-patterns) restent dans `AGENTS.md`.


## État partagé (tauri::State)

| State         | Champs clés                          | Commandes associées |
|---------------|--------------------------------------|---------------------|
| `TwitchState` | irc_handle, cancel, connected, login | twitch_connecter, twitch_deconnecter, twitch_etat, twitch_reconnecter |
| `YoutubeState`| chat_handle, cancel, connected, login, channel_id, access | youtube_connecter, youtube_deconnecter, youtube_etat |
| `KickState`   | ws_handle, cancel, connected, slug   | kick_connecter, kick_deconnecter, kick_etat, kick_slug_courant |
| `TiktokState` | chat_handle, cancel, connected, username | tiktok_connecter, tiktok_deconnecter, tiktok_etat, tiktok_username_courant |
