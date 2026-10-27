# API Stream Deck — HTTP :4321

> Section déplacée depuis `AGENTS.md` — les règles figées (CONTRAT/RÈGLE/anti-patterns) restent dans `AGENTS.md`.


## API Stream Deck — HTTP :4321 (api_deck.rs, 2026-08-27)

> Endpoints `/api/*` servis par le MÊME serveur :4321 que la diffusion —
> pas de 2e port, pas de process Elgato. Consommateurs : plugin Stream
> Deck natif, scripts, curl (le dashboard n'appelle JAMAIS /api/* — il
> passe par les commandes Tauri).

```
GET  /api/scenes           → [{ id, nom }]
POST /api/scene/ouvrir      { id }   → 200 / 404
POST /api/scene/nouvelle    { nom }  → 200 + { id, nom } / 500
GET  /api/scene/courante    → { id, nom }
```

- Mêmes fonctions que l'UI (`scenes::ouvrir` = save + load + snapshot) :
  une action Stream Deck bascule réellement la scène (diffusion incluse).
- **Sécurité (2026-10-13)** — pas d'auth, mais DOUBLE garde locale :
  - `server.rs` merge les routes api_deck APRÈS le layer CORS permissif →
    **/api/* n'a PAS de CORS** : un POST JSON exige un preflight OPTIONS
    sans réponse CORS → bloqué par le navigateur. Une page web quelconque
    ouverte dans le navigateur ne peut donc ni lire ni basculer/créer des
    scènes (le CORS permissif reste pour la LECTURE : médias :4321 →
    texImage2D morphing, inchangé).
  - `api_deck.rs::garde_host_locale` : Host limité à
    `127.0.0.1`/`localhost`/`::1` (port optionnel) — **anti-DNS-rebinding**
    (un domaine attaquant résolu vers 127.0.0.1 porte son propre nom en
    Host et devient same-origin pour le navigateur : le CORS ne
    s'applique alors jamais). Sinon 403.
  - Les clients natifs (plugin Stream Deck, curl) passent inchangés —
    ils se connectent via 127.0.0.1/localhost et ignorent le CORS.
- Structuré pour étendre plus tard (widgets, fond, lecture) sans gonfler
  server.rs — une extension = un handler + une ligne dans `routes()`.
