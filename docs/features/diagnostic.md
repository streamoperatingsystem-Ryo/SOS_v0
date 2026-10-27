# Diagnostic — modale à onglets

> Section déplacée depuis `AGENTS.md` — les règles figées (CONTRAT/RÈGLE/anti-patterns) restent dans `AGENTS.md`.


## Diagnostic — modale à onglets (consommation 2026-09-22, audience 2026-10-XX)

> La modale « Diagnostic » (`ConsoModal.svelte` — nom conservé) a deux
> onglets : **« Application »** (l'existant : consommation 1 Hz) et
> **« Audience »** (analyse des pics de viewers, Twitch d'abord).
> ⚠️ **Le module backend s'appelle `conso`, pas `diagnostic`** : un module
> racine nommé `diagnostic` masque le namespace built-in rustc
> `#[diagnostic::*]` que la macro `tauri::command` génère → E0433 sur
> TOUTES les commandes.

### Onglet « Application » — consommation (`conso.rs`)

### Ce qui est mesuré (Windows, échec = 0, jamais de crash)

- **Processus** (snapshot ToolHelp32) : StreamOS, WebView2 dashboard
  (enfants `msedgewebview2` agrégés), OBS, jeu (nom suivi via les events
  speedrun `connected`/`disconnected` du tap `chat_tx`), Système (total
  `GetSystemTimes` + `GlobalMemoryStatusEx`). CPU % du total + RAM Mo.
- **Sous-systèmes** (snapshot threads) : CPU % **d'un cœur** agrégé par
  label. Les threads s'enregistrent au spawn via
  `crate::conso::nommer_thread("Label")` (registre TID→label — pas de
  `SetThreadDescription`). Non enregistrés → bucket
  « App + runtime (chat, WS, serveur) ». Threads nommés actuels :
  Splitter (moteur ASL), Input viewer (manette/clavier/souris),
  Télémétrie (écriture).
- **GPU par processus** : compteurs PDH `\GPU Engine(*)\Utilization
  Percentage` (PdhAddEnglishCounterW — vendor-agnostic Intel/AMD/NVIDIA/
  ARM, WDDM 2.x). Instance `pid_<PID>_...` → somme des moteurs par PID.
  Requête échouée (VM/vieux Windows) → `gpu_pct: null` → « N/A » UI.
- **Compteurs d'activité** : tap passif `chat_tx` (même pattern télémétrie)
  — `chat` msgs/s, `speedrun-etat`/s, `input-viewer-etat`/s.

### Contrats

- **Non énergivore** : 1 snapshot processus + 1 snapshot threads + 1
  collecte PDH par seconde — **UNIQUEMENT pendant que la modale est
  affichée** (correctif 2026-10-13 de l'omission : le thread samplait
  1/s à vie dès le boot). Gating : chaque poll `conso_etat` repousse une
  auto-expiration de 3 s (`DEMANDE_MS`/`demande_recente` — pas de commande
  stop, rien à faire si la modale ferme brutalement) ; reprise après
  dormance = baselines rafraîchies (sémantique `premier_tick`) +
  historique vidé (échantillons périmés). Historique roulant **300 pts en
  mémoire** (`TAILLE_HISTORIQUE`). ⚠️ **Exception assumée depuis le
  2026-10-27 : la boîte noire** (voir plus bas) écrit sur disque en
  permanence — décision explicite de l'utilisateur. Les deux boucles sont
  totalement indépendantes (aucune map `prev` partagée).
- **Commande `conso_etat()` → `ConsoEtat { nb_coeurs, historique }`** :
  le frontend la pole **uniquement** quand la modale est ouverte (store
  `conso.ts` : `demarrerPollingConso`/`arreterPollingConso`, 1 Hz) — et
  depuis le correctif, la boucle Rust ne samble que pendant ces polls.
  Le tap passif `chat_tx` (compteurs d'activité + nom du jeu), lui,
  reste permanent : event-driven, coût nul.
- **Init** : `conso::demarrer(chat_tx.clone())` dans `lib.rs` setup
  (après la télémétrie, avant les auto-resume) — démarre le tap + le
  thread échantillonneur (qui dort jusqu'au premier poll).

### Fichiers

| Fichier | Rôle |
|---|---|
| `src-tauri/src/conso.rs` | Échantillonneur 1 Hz, registre threads, PDH GPU, commande `conso_etat`, tests du tap + **boîte noire** (boucle 5 s, rotation, `boite_noire_etat`) |
| `src/lib/stores/conso.ts` | Store `consoEtat` + polling 1 Hz borné à la modale + store `boiteNoire` (lecture à la demande, jamais pollé) |
| `src/lib/components/ConsoModal.svelte` | Modale à onglets (`application`/`audience`, pattern `.tabs` d'InteractionViewerModal) : onglet Application = lignes nom + % + RAM/GPU + sparkline SVG (survol = point précis « il y a N s ») — variables thème uniquement + **section « Boîte noire »** en bas |
| `src/lib/stores/ui.ts` | Flag `consoModalOpen` ; lanceur Toolbar « Diagnostic » ; montage `App.svelte` |

### Boîte noire — journal persistant (2026-10-27)

> Réponse au gel total du 2026-10-27 : `events.jsonl` s'arrêtait à la seconde
> du gel et la modale était fermée → **aucune mesure n'existait** pour la
> période précédente. La boîte noire comble ce trou.

**Ce que c'est** : un second échantillonneur, **indépendant** de celui à 1 Hz,
qui écrit **1 point toutes les 5 s** dans
`%APPDATA%\com.streamos.v0\telemetrie\boite_noire.jsonl`, **même modale
fermée**. C'est la seule trace qui survit à un gel ou à une coupure secteur.

**Contrat assumé — dérogation au « non énergivore »** (décision explicite,
2026-10-27) : cette boucle ne dort jamais. Coût borné à UN snapshot pour soi
(CPU + RSS) + UN snapshot ToolHelp pour `obs64.exe` +
`GlobalMemoryStatusEx` — ni PDH/GPU, ni snapshot threads.

- **`sync_data()` à chaque ligne** : sans lui les octets restent dans le cache
  de l'OS et disparaissent avec la coupure. Coût : une écriture disque / 5 s.
- **Rotation** : au-delà de 2 Mio, le fichier devient `boite_noire.1.jsonl`
  (UNE génération conservée → 2 fichiers max ≈ 2 jours à 5 s).
- **Champs** : `v`, `ts`, `ram_mo` (StreamOS), `cpu_pct` (StreamOS, % du
  total), `ram_systeme_mo` (RAM système **occupée**), `obs_present`,
  `obs_silence_s`.
- **`obs_silence_s`** = secondes depuis le dernier RPC OBS **réussi**
  (`null` = aucun depuis le boot). Horodaté au point de passage UNIQUE
  `obs::rpc` via `conso::marquer_obs_ok()` — c'est exactement le signal qui
  aurait expliqué le silence OBS de 15:18:22 le 2026-10-27.
- **Lecture** : commande `boite_noire_etat(limite)` (défaut 720 = 1 h),
  appelée **UNE fois** à l'ouverture de la modale (jamais pollée). Les lignes
  illisibles sont ignorées et comptées (`ignorees`) — une ligne partielle en
  fin de fichier après coupure est **normale**.
- **UI** : section « Boîte noire » de `ConsoModal.svelte` — les 8 derniers
  points + « Dernier point enregistré : il y a N s ». Après un redémarrage,
  cette dernière valeur est l'heure approximative du **gel précédent**.
- **Vie privée** : fichier local, jamais envoyé — conforme au CONTRAT FIGÉ.

### Onglet « Audience » — analyse des pics de viewers (Twitch d'abord)

> Dashboard historique **consultatif**, alimenté par `events.jsonl` —
> zéro polling, re-fetch au changement de période/session uniquement.

**Pipeline** : `telemetrie/analyse.rs` parse le JSONL (borné `depuis_jours`,
lignes cassées ignorées) → reconstruction des sessions par `session_id` →
série viewers rééchantillonnée grille **60 s** (carry-forward, le kind
`viewers` n'étant émis qu'au changement) → segments jeu aux changements de
`jeu_id` → détection des pics → attribution d'une cause.

**Algorithme de pic** : baseline `b(t)` = médiane de l'historique dispo
(robuste aux pics) ; ouverture si `v ≥ max(1.25×b, b+8)` sur ≥2 points,
fermeture si `v < 1.10×b` sur 2 points ; retenu si `delta ≥ 10` ou `≥30 %`.
Causes ordonnées (1re qui matche dans `[t_debut−300s, +120s]`) : `raid`
(avec `nb_viewers` → confiance certaine/probable) → `contenu` (changement
jeu/titre) → `communaute` (≥3 events bits/sub/gift) → `debut_live`
(≤3 min du début) → `organique`. Sessions <5 min ou <2 points ignorées.

**Commandes** : `audience_sessions(depuis_jours) → { periode_jours,
sessions: [SessionResume], agregats }` et `audience_session(session_id) →
{ meta, serie, segments, pics, timeline }`. Agrégats : top jeux/titres/
créneaux (jour_sem×heure), top pics, totaux events pendant les lives.

**Fichiers** : `stores/audience.ts` (résultats + chargement + erreur, pas
de polling), `AudienceTab.svelte` (graphe SVG aire+ligne, bandes jeu,
marqueurs timeline, zones de pics, cartes agrégats, liste de pics).
`tauri.ts` porte les types miroir.

**Limites documentées** : résolution 60 s (pic <1-2 min invisible),
`viewer_count` Twitch approximatif, `chat:event` Twitch-seul (autres
plateformes → état « Twitch uniquement »), aucune valeur fabriquée en
l'absence de données, tags stockés mais pas de stats par tag en v1.
