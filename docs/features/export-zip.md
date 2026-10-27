# Export ZIP complet

> Section déplacée depuis `AGENTS.md` — les règles figées (CONTRAT/RÈGLE/anti-patterns) restent dans `AGENTS.md`.


## Export ZIP complet — toutes les scènes + tous les médias (2026-10-19)

> EN PLUS de l'export/import unitaire existant (pack dossier, inchangé) :
> le bouton « Exporter » de la topbar est devenu un dropdown `.ctx-menu`
> (recette du sélecteur Langue) — « Scène courante » = l'existant,
> « Tout exporter (.zip) » = la nouvelle archive complète.

- **Commande** `scenes_exporter_zip` (lib.rs, `async` + `spawn_blocking` —
  dialog natif + écriture potentiellement lourde, jamais le thread UI) →
  `scenes::exporter_tout_zip` (scenes.rs). Dialog **« Enregistrer sous »**
  (`blocking_save_file`, filtre .zip, nom défaut
  `streamos-scenes-<AAAAMMJJ-HHMM>.zip`) : dossier + nom choisis par
  l'utilisateur ; un .zip existant est ÉCRASÉ (comportement voulu : « si
  les scènes existent déjà dans le dossier, on les remplace »). Extension
  `.zip` forcée si retirée dans le dialog. `save_current` d'abord → le
  miroir reflète l'état RAM de la courante.
- **Contenu de l'archive** — UN dossier racine = UNE sauvegarde : le nom
  du fichier choisi (sans `.zip`) devient le dossier racine
  `<sauvegarde>/`, tout est rangé dessous :
  - `<sauvegarde>/scenes/index.json` + `scenes/<id>.json` — miroir brut
    (restauré tel quel par l'import « Archive .zip », l'index est toujours
    écrit même si le fichier manquait).
  - `<sauvegarde>/medias/**` — TOUS les médias, récursif (`musique/`
    incluse, référencés ou non).
  - `<sauvegarde>/packs/<nom-scène>/scene.json` + `medias/` — un pack par
    scène au format de l'export unitaire (Scene + champ `nom`) →
    réimportable via Importer après dézip. Noms sanitizés (`<>:"/\|?*` +
    contrôles → `-`, trim espaces/points de bord, vide → `scene`) +
    dédupliqués (`-2`,`-3`).
- **`zip_store.rs`** : écrivain ZIP maison « stored » (méthode 0, zéro
  dépendance — même esprit que `mask_png.rs` ; les médias sont déjà des
  formats compressés, le deflate n'y gagnerait rien). Noms UTF-8
  (flag 0x800), CRC32 table `OnceLock`, limites classiques gardées
  (4 Gio / 65535 entrées). `ajouter_fichier` → `Ok(false)` = fichier
  illisible AVANT le header (skip propre, archive valide) ; `Err` =
  échec PENDANT l'écriture → archive corrompue → fatal et le .zip
  partiel est supprimé (`inspect_err`).
- **`ZipLecteur`** (même fichier) : lecteur pour l'import — parse du
  central directory (autorité tailles/CRC → les zips étrangers à data
  descriptor passent), méthodes **0 stored + 8 deflate** (inflate raw via
  `flate2` — les zips compressés par Windows/7-Zip sont lus aussi),
  CRC32 vérifié à chaque `lire_bytes`. Gardes : multi-disque et Zip64
  refusés ; noms normalisés anti zip-slip (`\`→`/`, `..`/absolu/`:`/vide
  rejetés → entrée ignorée, pas fatal) ; décompression bornée à la taille
  déclarée + 1 (anti bombe) ; entrée > 512 Mo refusée.
