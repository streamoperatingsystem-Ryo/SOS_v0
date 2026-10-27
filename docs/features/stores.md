# Frontend — stores

> Section déplacée depuis `AGENTS.md` — les règles figées (CONTRAT/RÈGLE/anti-patterns) restent dans `AGENTS.md`.


## Frontend — stores

| Store         | Rôle                                              |
|---------------|---------------------------------------------------|
| `chat.ts`     | Store central : `connexions`, `initChat()`, listeners events, stores d'erreur |
| `twitch.ts`   | `connecterTwitch`, `deconnecterTwitch`            |
| `youtube.ts`  | `connecterYoutube`, `deconnecterYoutube`          |
| `kick.ts`     | `connecterKick`, `deconnecterKick`, `lireSlugSauve` |
| `tiktok.ts`   | `connecterTiktok`, `deconnecterTiktok`, `lireUsernameSauve` |
| `communaute.ts`| Followers/subs/viewers (Twitch Helix + YouTube Data API). Affichage intégré dans la modale Modération & Rôles (onglet Twitch → section « Communauté » lecture seule ; onglet YouTube activé). Chargement réactif dans `App.svelte` ($effect sur `connexions.twitch`/`connexions.youtube`). |
| `presets.ts`   | Galerie de scènes modèles : `presetsStore`, `chargerPresets`, `creerSceneDepuisPreset` (NOUVELLE scène), `initialiserSceneDepuisPreset` (onboarding + recharge UI + sceneSyncCaptures). |
| `langue.ts`    | Store i18n : `langue`, `t` (traducteur `$t("cle")` / `get(t)("cle")`), `localeTag` (toLocaleString), `initLangue`/`setLangue`, `LANGUES`. Voir section i18n + TRADUCTIONS.md. |
