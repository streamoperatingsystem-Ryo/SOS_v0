# Import ZIP complet

> Section déplacée depuis `AGENTS.md` — les règles figées (CONTRAT/RÈGLE/anti-patterns) restent dans `AGENTS.md`.


## Import ZIP complet — restauration d'une sauvegarde (2026-10-19)

> Le bouton « Importer » de la topbar est lui aussi un dropdown
> `.ctx-menu` — « Importer un pack dossier » = l'existant (inchangé),
> « Tout importer (.zip) » = restauration complète d'une archive.

- **Protocole en 2 phases** (même recette que l'import de commandes) :
  `scenes_importer_zip` (phase 1 — dialog « Ouvrir » filtré .zip +
  `choisir_et_analyser_zip`, AUCUNE écriture) retourne
  `AnalyseZipImport { zipPath, scenes, conflits }`. Le frontend
  (`importToutZip`, stores/scenes.ts) tranche chaque conflit par
  `confirm()` puis appelle `scenes_importer_zip_appliquer` (phase 2 —
  `appliquer_zip_import`, `save_current` d'abord).
- **Détection de la racine** (`detecter_racine_zip`) : le préfixe devant
  `scenes/index.json` — un zip re-zippé à la main dans un sous-dossier,
  notre format `<sauvegarde>/` ou un vieux zip sans racine sont lus pareil
  (préfixe le plus court gagne). Pas d'index → « archive invalide ».
- **Conflits** : id déjà dans l'index local → `confirm()` par scène
  (OK = écraser `<id>.json` + reprendre nom/nomMasque de l'archive ;
  Annuler = passé). Scènes nouvelles → fichier + index. Scènes locales
  absentes du zip → **intactes** (fusion, jamais réinitialisation).
  Un `<id>.json` qui ne parse pas comme `Scene` n'est JAMAIS écrit (une
  scène locale saine n'est pas cassée).
- **Médias** : `<racine>medias/**` → `data/medias/**`, écrasement des
  fichiers du même nom (noms uuid — même nom = même fichier). Entrée
  illisible/corrompue → log warn + skip, jamais fatal.
- **Fin** : la scène courante est rechargée depuis le disque en RAM +
  snapshot (si son id a été remplacé, l'ancienne RAM ne réécraserait pas
  par-dessus) ; le front recharge index + scène + `syncCapturesOBS` et
  affiche le bilan `{scenesImportees, mediasRestaures, scenesSautees}`.
  Clés `scenes.importPackDossier` / `importToutZip` / `importZipConflit` /
  `importZipOk` (8 locales).
- **Limite héritée** : les packs utilisent `collecter_medias` → les
  carrousels (`mediaSlides`/`bgSlides`) ne sont pas ramassés (même limite
  que l'export unitaire — non corrigée) ; leurs fichiers restent présents
  dans le miroir `medias/` de l'archive.
- **Front** : `tauri.scenesExporterZip()` → `string | null` (chemin du
  zip) ; `exportToutZip()` (stores/scenes.ts) alerte le chemin au succès.
  Dropdown dans App.svelte (`exportMenuOuvert`, wrap `stopPropagation`,
  fermeture mutualisée avec le menu Langue : `fermerMenusTopbar` au clic
  fenêtre + branche dans `onSupprKey`). Clés `scenes.exportSceneCourante` /
  `exportToutZip` / `exportZipOk` (8 locales).
