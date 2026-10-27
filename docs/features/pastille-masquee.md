# Pastille « Vos scènes » masquée

> Section déplacée depuis `AGENTS.md` — les règles figées (CONTRAT/RÈGLE/anti-patterns) restent dans `AGENTS.md`.


## Pastille « Vos scènes » masquée — confirmation d'ouverture (2026-10-19)

> `SceneIndex.nomMasque` (menu contextuel pastille → « Masquer cette
> scène ! », `scene_masquer_nom` → `scenes::masquer_nom` dans index.json)
> colore le titre en orange (`--surbrillance-orange`) sur la pastille — le
> nom reste visible, « masquée » = scène marquée sensible. Cliquer une
> pastille masquée ≠ courante n'ouvre PAS la scène directement :
> `onPastille` (SceneBar) pose `confirmOuvrirSceneModalOpen` (ui.ts — cible
> portée par la valeur, pattern `dupliquerSceneModalOpen`) →
> `ConfirmOuvrirSceneModal.svelte` (montée par App.svelte) rappelle le titre
> dans son hint + Oui/Non (Enter = oui, Escape/clic overlay = non). Oui →
> `openScene(id)` (chaîne complète save+load+snapshot). Clic sur la masquée
> courante et sur une non-masquée : inchangés (pas de confirmation). L'API
> Stream Deck `/api/scene/ouvrir` n'a PAS de confirmation (appui matériel
> délibéré). Zone i18n dédiée `confirmOuvrirScene` (8 locales), crochet
> `--surbrillance-orange`.
