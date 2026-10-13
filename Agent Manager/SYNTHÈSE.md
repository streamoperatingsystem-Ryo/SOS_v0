# SYNTHÈSE — StreamOS v0

**Date :** mardi 13 octobre 2026
**Destinataire :** Agent Manager (nouvelle fenêtre de contexte)
**Rédigé par :** Devin (session du 13/10/2026 — bilan de clôture + exécution)
**Commit de référence :** état de clôture du 2026-10-13 (bilan + lots de
clôture commités ce jour — voir `git log`)

---

## 1. LE PROJET EN UNE PHRASE

StreamOS v0 est une application desktop (Tauri v2 + Svelte 5 + Rust) qui sert de
**dashboard de stream** : l'utilisateur compose des scènes (widgets média, chat
multi-plateformes, caméra, input viewer, speedrun, alertes…) dans une interface
Svelte, et un serveur HTTP embarqué (:4321) diffuse ces scènes vers OBS via une
browser source. Twitch, YouTube, Kick et TikTok sont intégrés (chat + communauté).

---

## 2. DOCUMENTATION TECHNIQUE → `AGENTS.md`

Architecture, stack, structure des fichiers, état des features, conventions,
commandes de build, pièges et contrats figés : **voir `AGENTS.md` à la racine du
projet** — source de vérité technique, mise à jour à chaque feature. Ce fichier
ne duplique plus ce périmètre : il ne contient que la roadmap et l'historique de
relais.

En cas de contradiction entre ce fichier et `AGENTS.md` sur un point technique,
`AGENTS.md` fait foi.

⚠️ Rappel corrigé : la persistance est dans `%APPDATA%\com.streamos.v0\`
(l'ancienne version de ce fichier disait `%APPDATA%/StreamOS/` — faux).

---

## 3. ROADMAP — CLÔTURÉE (2026-10-13)

> **Décision utilisateur : le périmètre de fonctionnalités est FIGÉ.** Le
> bilan de clôture (Devin, 13/10/2026) n'a trouvé aucune feature à moitié
> implémentée — l'application est fonctionnellement complète. Les sujets
> ci-dessous restent archivés comme PISTES SI RÉOUVERTURE un jour ; ils ne
> sont PAS à faire et ne doivent pas être relancés sans décision explicite.

- **Emojis géants** (viewer envoie un emoji → apparaît en grand et danse à
  l'écran) — ancienne recommandation, jamais démarré.
- **Barres de progression** avec pseudos/personnages — jamais démarré.
- Sujets documentés « non implémentés » dans AGENTS.md : sauvegarde des
  splits proposée à la sortie du jeu (speedrun), rotation mensuelle de la
  télémétrie.
- Lecteur musical sans son (ancien repo, en pause).

*Retirés de la roadmap car faits depuis la v0.17 : commandes viewer `!cmd`,
clips de bienvenue, YouTube, Kick, multi-chat unifié, modération
(mods/VIP/bans), refonte CSS globale (thème HUD), speedrun splitter —
détail dans `AGENTS.md` + `git log`.*

---

## 4. POINT DE PASSAGE — RELAIS

### Historique des agents
1. **Grok** : scaffold initial (5 lots), extensions (vidéo, gizmo 3D,
   multi-scènes, trous OBS, énumération PC), Twitch (Device Code, IRC, avatars,
   auto-resume, revoke, reconnexion), pop-out chat, bandeau header, Communauté
   Helix LOT 1 + 2 (bug keyring non résolu).
2. **GLM-5.2 High** (28/08/2026) : diagnostic bug Communauté, correction
   keyring `windows-native`, nettoyage logs + pop-out, commit v0.17 `660394e`.
3. **Agents intermédiaires (29/08 → 19/09)** : v0.18 `15081ec` (alertes Twitch,
   bandeau 1er message, commandes chat, pad numérique, speedrun splitter,
   compteur viewers), welcome-clip `e8b1e20`, caméra au-dessus des trous
   `912904f`, effets visuels du fond `9187bff`, barre de raccourcis topmost +
   v0.1.3 `a755b4a`. + YouTube/Kick/TikTok, input viewer, morphing, cadres SVG,
   titre de widget, modération, unfollows, remux MKV, thème HUD (détail :
   `AGENTS.md` + `git log`).
4. **Devin** (19/09/2026) : resynchronisation documentaire — Protocole
   d'exécution ajouté dans `AGENTS.md`, création `.windsurf/rules/agents-md.md`,
   refonte de ce fichier (roadmap + relais uniquement), archivage de
   l'historique brut dans `Agent Manager/archives/`.
5. **Devin** (13/10/2026) : **bilan de clôture** (audit complet lecture
   seule, 4 lots exécutés après validation utilisateur) — commit de la
   feature « Lire la sélection musicale » trouvée non commitée, retrait du
   résidu legacy welcome-clip (TS + i18n), double garde locale sur l'API
   Stream Deck (CORS retiré de /api/* + Host anti-DNS-rebinding),
   resynchronisation AGENTS.md (api_deck, pad numérique, cadres
   arena/tribal, persistance complète, dates, speedrun 11 commandes) +
   TRADUCTIONS.md (phases faites) + ce fichier. Chantier « rem » :
   verdict utilisateur PENDANT (section annulation d'AGENTS.md conservée).

### Leçons apprises
- **keyring v3** : les features par défaut ont sauté vs v2 — sans
  `features = ["windows-native"]`, mock keystore → coffre vide au reboot.
- **Mutex non réentrant** : jamais 2 `.lock()` du même `Mutex` dans une même
  expression → thread principal figé (détail : `AGENTS.md`).
- **Périmètre de lot sacré** : un Coder a déjà modifié un fichier « à ne pas
  toucher » — le périmètre est une liste blanche fermée (Protocole d'exécution,
  `AGENTS.md`).
- **Une seule source de vérité par périmètre** : ce fichier dupliquait la doc
  technique et a dérivé (chemin APPDATA faux, roadmap obsolète). Désormais :
  technique → `AGENTS.md`, roadmap/relais → ici.

### État du git (13/10/2026, après les lots de clôture)
```
master : docs AGENTS.md resynchronisé (bilan de clôture) + 3 commits
        d'exécution (musique non commitée, welcome-clip, api_deck)
origin/master : 1718b55 (ui: pastilles .msg-pill, 2026-09-16)
```
- `master` ahead of `origin/master` by 28+ commits (pas de push effectué).
- Working tree : propre (docs `PLAN-*.md` / `TRADUCTIONS.md` trackés à
  partir de la clôture ; `image.png` / `message-a-manu.html` = artefacts
  locaux non trackés, à la main de l'utilisateur).
- `SETUP/` + `Agent Manager/archives/` + `IDEE/` ignorés (docs de travail,
  hors repo).

---

*Document de synthèse — roadmap + relais uniquement. Le détail technique vit
dans `AGENTS.md` (racine), mis à jour à chaque feature. Point de retour sûr
avant les lots de clôture : `acec456` (couleurs de l'équaliseur, état du
2026-10-13 au matin).*
