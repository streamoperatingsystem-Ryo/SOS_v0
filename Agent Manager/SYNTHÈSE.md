# SYNTHÈSE — StreamOS v0

**Date :** samedi 19 septembre 2026
**Destinataire :** Agent Manager (nouvelle fenêtre de contexte)
**Rédigé par :** Devin (session du 19/09/2026 — resynchronisation documentaire)
**Commit de référence :** `a755b4a` — feat: barre de raccourcis topmost + v0.1.3 (setup-base0004)

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

## 3. CE QUI RESTE À FAIRE (roadmap)

### Interactions viewers
- **Emojis géants** : un viewer envoie un emoji → l'emoji apparaît en grand et
  danse à l'écran (effet CSS dans `diffusion.html` + détection côté Rust).
  **Recommandation historique :** commencer par celui-là — effet visible
  immédiat, périmètre restreint, pose les fondations de l'overlay d'effets que
  les autres réutiliseront. La décision appartient à l'utilisateur.
- **Barres de progression** : avec pseudos/personnages.
- Autres effets d'interaction à définir.

### Hors scope actuel
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

### État du git
```
a755b4a (HEAD -> master) feat: barre de raccourcis topmost + v0.1.3 (setup-base0004)
9187bff (origin/master) feat: effets visuels du fond sur le dashboard
```
- `master` ahead of `origin/master` by 1 commit (pas de push effectué).
- Working tree : `AGENTS.md` + `.gitignore` modifiés, `.windsurf/` et
  `Agent Manager/` nouveaux non trackés (ajout prévu), `src-tauri/Cargo.lock`
  modifié (antérieur à cette session).
- `SETUP/` + `Agent Manager/archives/` ignorés (docs de travail, hors repo).

---

*Document de synthèse — roadmap + relais uniquement. Le détail technique vit
dans `AGENTS.md` (racine), mis à jour à chaque feature. Le commit `a755b4a` est
le point de retour sûr.*
