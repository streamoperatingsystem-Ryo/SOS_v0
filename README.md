# StreamOS v0

Application de bureau Windows pour streamer : **dashboard** d'édition de scènes
et **diffusion** (OBS ou moteur natif Rust), plus une couche de
communauté (chat multi-plateformes, alertes, commandes, clips, HUD).

> **Plateforme** : Windows 10/11 uniquement. Le backend utilise des APIs
> natives (WGC/DirectX, WASAPI, Media Foundation) et le coffre d'identifiants
> Windows ; il n'existe pas de portage Linux/macOS.

## Pile technique

| Couche | Techno |
|---|---|
| Frontend | Svelte 5 + Vite 8 + TypeScript (svelte-check) |
| Backend | Rust 2021 (≥ 1.77) + Tauri 2 |
| Serveur local | axum (`127.0.0.1:4321`, WS + `/api/*` pour Stream Deck) |
| Moteur natif | crates locales : capture WGC, composition Direct2D, encodage NVENC/MFT, audio WASAPI |

## Vie privée — tout local, zéro envoi

Il n'existe **aucun serveur StreamOS**. Les scènes, configs et journaux vivent
dans `%APPDATA%\com.streamos.v0\` ; les tokens Twitch/YouTube dans le keyring
Windows. Le réseau ne sert qu'aux APIs officielles des plateformes, aux APIs
publiques d'emotes et aux CDN d'images/clips, toujours déclenchés par une
action utilisateur. Aucun analytics, aucun crash-reporting, aucun fingerprinting.

Contrat complet et exceptions documentées : [`AGENTS.md`](AGENTS.md), section
« Vie privée — tout local, zéro envoi ».

## Démarrer

```bash
npm install
npm run tauri dev          # dev complet (Rust + frontend)
```

Le sidecar **FFmpeg n'est pas versionné** (voir `.gitignore`) : place
`ffmpeg-x86_64-pc-windows-msvc.exe` dans `src-tauri/binaries/` avant de
compiler si tu touches à l'encodage natif.

## Build et vérification

```bash
# Frontend
npm run check              # svelte-check — 0 erreur exigé
npm run build              # build production

# Rust
cd src-tauri
cargo check                # compilation
cargo clippy               # lints — 0 warning exigé
cargo build                # build debug
```

Nettoyage disque (les objets orphelins de `target/debug/deps` ne sont jamais
supprimés par rustc) :

```powershell
powershell -ExecutionPolicy Bypass -File tools\nettoyage-cargo.ps1 -Simulation
```

## Arborescence

```
src/                        Frontend Svelte
  App.svelte                Racine (dashboard + diffusion)
  lib/components/           Composants (Canvas, cartes d'édition, modales…)
  lib/stores/               Stores Svelte (scene, ui, moteur, chat…)
  lib/i18n/                 8 locales (fr = source de vérité)
  theme/glace.css           Tokens --t-* (couleurs jamais en dur)

src-tauri/                  Backend Tauri
  src/                      Commandes et services (chat, alertes, OBS, config…)
  src/moteur/               Façade du moteur natif
  crates/sos-scene/         Contrats de scène (types purs partagés)
  crates/sos-moteur-api/    Trait Moteur, capacités, config de sortie
  crates/sos-moteur-natif/  Moteur : capture, composition, encodage, audio
  resources/diffusion.html  Rendu de diffusion (page lue par OBS)
  binaries/                 FFmpeg sidecar (non versionné)

docs/features/              Documentation d'implémentation, une feature par fichier
tools/                      Scripts PowerShell (mesure, purge cargo)
vendor/                     Dépendances vendorisées (TikTok Webcast)
```

## Documentation

- [`AGENTS.md`](AGENTS.md) — règles transversales, contrats figés, persistance,
  plan de suivi du moteur natif et **index complet de `docs/features/`**.
- [`docs/features/`](docs/features/) — une fiche par feature (contrat,
  implémentation, pièges).
- [`PLAN-MOTEUR-NATIF.md`](PLAN-MOTEUR-NATIF.md) — plan de parité OBS → Rust.
- [`docs/HANDOFF.md`](docs/HANDOFF.md) — état des lieux pour reprendre le projet.

## Pour lire ce dépôt (agents / LLM)

Les gros fichiers du dépôt dépassent la limite de la plupart des outils de
lecture web, et GitHub renvoie du HTML — **clone le dépôt plutôt que de
scraper les pages** :

```bash
git clone https://github.com/streamoperatingsystem-Ryo/SOS_v0.git
```

Fichiers notoirement volumineux, à lire par tranches :

| Fichier | Taille |
|---|---|
| `src-tauri/src/lib.rs` | ~4 800 lignes |
| `src-tauri/resources/diffusion.html` | ~13 300 lignes |
| `AGENTS.md` | ~75 Ko |

Accès texte brut si le clone est impossible :
`https://raw.githubusercontent.com/streamoperatingsystem-Ryo/SOS_v0/master/<chemin>`
