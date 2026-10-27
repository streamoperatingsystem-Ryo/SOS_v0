<script lang="ts">
  // Modale « Statistiques » — analyse d'audience des lives Twitch (sessions
  // reconstruites depuis telemetrie/events.jsonl), extraite de l'ancienne
  // modale « Diagnostic » : c'est une lecture post-live de la chaîne
  // (lanceur « Ma chaîne »), pas une surveillance technique de l'app.
  // AudienceTab gère tout : sélecteurs période/live, graphe viewers, pics —
  // chargé au montage (onMount → chargerAudience).
  import Modal from "./Modal.svelte";
  import AudienceTab from "./AudienceTab.svelte";
  import { t } from "../stores/langue";

  let { onFermer = () => {} }: { onFermer?: () => void } = $props();
</script>

<Modal
  title={$t("statistiques.titre")}
  onClose={onFermer}
  maxWidth="61rem"
  crochet="var(--crochet-teal)"
>
  <div class="stats-corps">
    <AudienceTab />
  </div>
</Modal>

<style>
  /* AudienceTab remplit la modale — mêmes marges que le corps de
     l'ancienne modale Diagnostic. */
  .stats-corps {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    padding: 0.6rem 1rem 1rem;
  }
</style>
