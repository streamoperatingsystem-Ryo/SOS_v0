# Unfollows Twitch — détection au démarrage

> Section déplacée depuis `AGENTS.md` — les règles figées (CONTRAT/RÈGLE/anti-patterns) restent dans `AGENTS.md`.


## Unfollows Twitch — détection au démarrage (2026-09-15)

> À chaque démarrage de StreamOS, la liste des followers est comparée au
> snapshot précédent (`followers_snapshot.json`). Les user_id présents dans
> l'ancien snapshot mais absents du nouveau = unfollows. Ils sont enregistrés
> dans `unfollows.json` (append-only, dédupliqué par user_id) avec la date de
> détection (timestamp ISO 8601 du démarrage). Le nouveau snapshot écrase
> l'ancien.

### Flux

```
twitch_communaute_followers (lib.rs)
  ├─ Helix /channels/followers → liste followers (login, user_id, followed_at, display_name)
  ├─ enrichir_avatars (twitch_helix.rs) : batch /users?id=... (max 100 par requête)
  │   → remplit profile_image_url + display_name pour chaque follower
  ├─ lire_followers_snapshot (config.rs) → anciens user_id
  ├─ diff anciens vs courants → unfollows détectés
  ├─ ajouter_unfollows (config.rs) → append dans unfollows.json (dédupliqué)
  └─ sauver_followers_snapshot (config.rs) → écrase followers_snapshot.json
```

### Affichage

- **Modale Modération & Rôles → onglet Twitch → section « Communauté »** :
  followers affichés avec avatar + display_name + date de follow.
- **Modale Modération & Rôles → onglet Twitch → section « Unfollows »** :
  historique des unfollows (login + date de détection), trié plus récent en premier.
- Store `communaute.ts` : `unfollows` (writable), `chargerUnfollows()` (lecture disque via `twitch_communaute_unfollows`).
- `chargerCommunaute()` inclut `chargerUnfollows()` (Promise.allSettled).

### Limitations

- La détection ne se fait qu'au démarrage (pas en temps réel — Twitch n'a pas
  d'event IRC pour les unfollows). Si un follower unfollow puis re-follow
  entre deux démarrages, il n'apparaîtra pas dans les unfollows.
- Le login d'un unfollow vient du snapshot précédent. Si le snapshot est
  corrompu ou absent (premier démarrage), le login peut être vide (user_id
  seulement).
- `enrichir_avatars` est non-fatal : si le batch /users échoue (rate limit,
  réseau), les followers sont retournés sans avatar (profile_image_url vide).
