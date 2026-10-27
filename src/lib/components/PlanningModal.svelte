<script lang="ts">
  // Modale « Planning » — planning Twitch (Helix /schedule) dédié, extrait
  // de la modale Interactions : c'est de la gestion de chaîne, pas une
  // interaction viewer (lanceur « Ma chaîne » de la Toolbar).
  // PlanningTab gère tout : vues semaine/mois/liste, glisser-déposer de
  // jacquettes, vacances — relu au montage (onMount → chargerPlanning).
  import Modal from "./Modal.svelte";
  import PlanningTab from "./PlanningTab.svelte";
  import { t } from "../stores/langue";

  let { onFermer = () => {} }: { onFermer?: () => void } = $props();
</script>

<Modal
  title={$t("planning.titre")}
  onClose={onFermer}
  maxWidth="75rem"
  crochet="var(--crochet-indigo)"
>
  <div class="planning-corps">
    <PlanningTab />
  </div>
</Modal>

<style>
  /* PlanningTab remplit toute la modale (grille semaine + colonne jeux) —
     reprend le comportement de l'ancien .tab-body-planning : pas de
     colonne de contenu centrée à 42.5rem. */
  .planning-corps {
    flex: 1;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    padding: 1rem;
    scrollbar-width: thin;
    scrollbar-color: var(--bordure-ui-active) var(--fond);
  }
  .planning-corps::-webkit-scrollbar {
    width: 0.5rem;
  }
  .planning-corps::-webkit-scrollbar-track {
    background: var(--fond);
  }
  .planning-corps::-webkit-scrollbar-thumb {
    background: var(--bordure-ui-active);
    border: 2px solid var(--fond);
  }
  .planning-corps::-webkit-scrollbar-thumb:hover {
    background: var(--accent-ui);
  }
</style>
