# Mixeur audio natif — N sources, périphériques, vu-mètres

> **Feature IMPLÉMENTÉE** (2026-11-02) — mode Rust uniquement. En mode OBS,
> l'audio se règle dans OBS (le mixeur natif ne capterait rien).
> Contrats transversaux : `AGENTS.md` (protocole, vie privée) priment.

---

## 1. Objectif

Parité OBS sur l'audio, avec trois exigences produit :

1. **Choisir les périphériques** — loopback d'une sortie précise, micro
   d'une entrée précise (pas seulement « défaut système »).
2. **N sources** — plusieurs sorties et/ou plusieurs micros, mixées dans
   l'ordre de la liste (comme le mixer OBS).
3. **Réglages à chaud** — gain, mute, ajout/retrait et changement de
   périphérique s'appliquent **pendant** un enregistrement ou un stream.

Ce qui fait « mieux qu'OBS » : des **vu-mètres mesurés** (peak/RMS/peak-hold
pré-fader par source + bus programme post-gain), une lecture **numérique en
dBFS** toujours visible, et une sémantique d'accessibilité native
(`role="meter"`, `aria-valuetext` en dBFS, valeurs jamais portées par la
seule couleur).

---

## 2. Contrat moteur (`sos-moteur-api`)

```rust
pub enum GenreAudio { Desktop, Micro }          // ids stables "desktop" | "micro"

pub struct SourceAudioCfg {
    pub id: String,                  // clé stable (diff à chaud + niveaux)
    pub genre: GenreAudio,
    pub peripherique: Option<String>, // ID IMMDevice ; None = défaut système
    pub gain: u32,                   // millièmes (1000 = unité, max 4000)
    pub mute: bool,                  // gain conservé sous mute
}

pub struct AudioCfg { pub sources: Vec<SourceAudioCfg> }
```

Règles :

- `id` **unique** (les niveaux sont indexés par id) et **stable** — un
  changement de périphérique ne le modifie pas.
- `gain_effectif()` = `0` si `mute`, sinon `gain` : une seule vérité pour le
  moteur, le mute ne détruit jamais le réglage.
- `actif()` = liste non vide. Une source muette **garde sa piste** (silence),
  comme une source mutée d'OBS ; aucune source = pas de piste dans le
  conteneur (comportement historique des harnais).
- `valider()` refuse id vide/doublon et gain > 4000 **avant** tout contact
  WASAPI.

---

## 3. Moteur (`sos-moteur-natif`)

### 3.1 Énumération et ouverture (`audio/capture.rs`)

- `lister(sens)` : périphériques **actifs** (`EnumAudioEndpoints`), nom
  convivial (`PKEY_Device_FriendlyName`), ID `IMMDevice`, drapeau « défaut » ;
  défaut en tête puis ordre alphabétique.
- `ouvrir(sens, id, niveaux)` : ouvre l'ID demandé ; **ID introuvable →
  repli sur le défaut**, signalé (`AppareilOuvert::repli`) — l'enregistrement
  continue et l'UI affiche l'avertissement (jamais une piste muette sans le
  dire).
- COM : `CoInitializeEx(MTA)` dans chaque appel (idempotent). Aucune
  interface COM ne quitte le thread de capture (`Capteur` n'est pas `Send`).

### 3.2 Mesure (`audio/mod.rs`)

`NiveauxPartages` — **atomics uniquement**, aucun verrou dans le chemin
chaud. Alimenté par le thread de capture (pré-fader) et par le mixeur (bus
programme, post-gain).

| Paramètre | Valeur |
|---|---|
| Plancher d'affichage | −96 dBFS |
| Décroissance du peak | 20 dB/s (PPM broadcast) |
| Peak-hold | 1,5 s puis décroissance |
| RMS | moyenne exponentielle, τ = 0,3 s |
| Écrêtage | latch si \|x\| ≥ 0,999 (effacé par le bouton UI) |

Mono = dupliqué sur les deux canaux ; au-delà de 2 canaux, seuls les deux
premiers sont mesurés. Un périphérique qui cesse de livrer des paquets
**décroît** (`decroitre`) : le vu-mètre ne gèle jamais sur un pic ancien.

### 3.3 Sources vivantes (`audio/live.rs`, `audio/piste.rs`)

- Voie **live** (nominale) : `SourceLive` — anneau borné (1 s, drop-oldest)
  + PCM fichier (réparation du post-mux). `attendre_demarrage` rend le
  périphérique réellement ouvert ou l'erreur d'ouverture.
- Voie **post-mux** (repli si le pipe audio FFmpeg n'a pas pu être créé) :
  `CapturePiste` — PCM fichier uniquement, `muxer_audio` à l'arrêt.
- Une source indisponible **dégrade** (muette + erreur publiée), elle
  n'annule jamais la sortie.

### 3.4 Boucle (`moteur.rs`)

- `audio_cfg: Arc<Mutex<AudioCfg>>` + `audio_version: AtomicU64` : la boucle
  relit la config **quand la version change** (aucun verrou par frame).
- Diff par `id` (`appliquer_audio`) : retrait → arrêt + archivage du PCM ;
  ajout → démarrage ; **périphérique/genre changé → redémarrage de la seule
  source** ; gain/mute → mise à jour de l'entrée de mix (immédiat).
- Entrées fixes du mixeur = sources WASAPI, puis anneaux médias/overlays
  reconstruits à la frame. L'écran d'attente coupe **toutes** les sources
  `Desktop` (gain 0), équivalent natif du mute « bureau » d'OBS.
- `etat_audio()` publie périphériques réels, replis, erreurs et compteurs
  partagés — l'app lit, le moteur n'appelle jamais l'UI.

---

## 4. Persistance

`%APPDATA%\com.streamos.v0\moteur.json` → bloc `audio` :

```json
{
  "audio": {
    "sources": [
      { "id": "desktop", "genre": "desktop", "peripherique": null,
        "gain": 1000, "mute": false },
      { "id": "micro", "genre": "micro", "peripherique": "{0.0.1.00000000}.{…}",
        "gain": 1000, "mute": false }
    ]
  }
}
```

- **Migration one-shot** : si `audio` est absent, l'ancien bloc `sortie`
  (`audio_loopback`, `audio_micro`, `gain_loopback`, `gain_micro`) est
  converti puis écrit. Absents = activés (défauts historiques).
- Lecture non fatale côté démarrage : bloc cassé → défauts produit
  (`desktop` + `micro`, périphériques par défaut).
- Écriture atomique (`lire_moteur_json` + `ecrire_moteur_json`), clés
  inconnues préservées.

---

## 5. UI

- **Section « Audio »** de la barre latérale, visible **en mode Rust**
  (`stores/ui.ts` → `Section = "audio"`). La modale Sortie renvoie vers elle.
- `MixeurAudio.svelte` : liste de sources (nom, mute, retrait, sélecteur de
  périphérique, gain en dB, vu-mètre, erreur/repli), ajout « + Son du PC » /
  « + Micro », bus **Programme** + bouton « Effacer l'écrêtage ».
- `VuMetre.svelte` : deux pistes stéréo, échelle −60…0 dBFS, repère de
  peak-hold, valeur numérique, `role="meter"` + `aria-valuetext` en dBFS.
  La recette `.vu-*` vit dans `app.css` (jamais recopiée).
- Gains : slider **−60…+12 dB** → millièmes (`10^(dB/20) × 1000`, borné
  1…4000 ; 0 est réservé au mute). Les sliders écrivent après 150 ms
  (debounce), mute/périphérique/ajout/retrait immédiatement.
- Polling **20 Hz uniquement panneau monté** (`demarrerPollingAudio` /
  `arreterPollingAudio`) — aucun polling global, invariant du dépôt.

---

## 6. Limites assumées (v1)

- **Pas d'audio par application** (phase P5 du contrat moteur :
  `Capacites.audio_par_app = false`).
- **Pas de filtres** (compresseur, gate, EQ par source) ni de mute/solo par
  bus : le mixage est un simple gain par source.
- **Bus programme** : mesuré uniquement en voie live. En repli post-mux
  (pipe FFmpeg indisponible), les sources restent mesurées mais le master
  affiche le silence ; un changement de gain en cours de session s'applique
  alors à la piste entière au muxage (pas d'enveloppe par bloc).
- **Changement de périphérique / ajout** : redémarre la source sur le thread
  de composition → micro-blocage ≈ 200 ms (attente du premier paquet
  WASAPI). Gain/mute ne bloquent jamais.
- **Périphérique débranché** : repli sur le défaut signalé ; le
  rebranchement n'est pas détecté à chaud (choisir « Rafraîchir » puis le
  périphérique).
- **Loopback post-volume** : baisser/muter la sortie Windows baisse/mute la
  capture — comportement identique à OBS.

---

## 7. Vérification

```bash
cd src-tauri
cargo clippy -p sos-moteur-natif --all-targets -- -D warnings   # exit 0
cargo test  -p sos-moteur-natif --lib                            # 304 tests
cargo clippy -p streamos-v0 -- -D warnings                       # exit 0
cargo test  -p streamos-v0 --lib                                 # 248 tests
cd .. && npm run check                                           # 0 erreur
```

Harnais : `cargo run --release --example audio_scene -- sortie.mp4 10 deux`
(le MP4 doit contenir H.264 + AAC 48 kHz stéréo).
