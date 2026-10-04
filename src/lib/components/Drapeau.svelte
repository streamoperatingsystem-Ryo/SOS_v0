<script lang="ts">
  // Mini-drapeau SVG du sélecteur de langue (18×12). Pas d'emoji drapeau :
  // WebView2/Windows ne les rend pas (ils s'affichent « FR », « EN »…).
  // Traits volontairement simplifiés — lisibles à ~18 px de large.
  let { id }: { id: string } = $props();
</script>

<svg class="drapeau" viewBox="0 0 18 12" aria-hidden="true" focusable="false">
  {#if id === "en"}
    <rect width="18" height="12" fill="#012169" />
    <path d="M0 0L18 12M18 0L0 12" stroke="#fff" stroke-width="2.4" />
    <path d="M0 0L18 12M18 0L0 12" stroke="#C8102E" stroke-width="0.8" />
    <path d="M9 0V12M0 6H18" stroke="#fff" stroke-width="4" />
    <path d="M9 0V12M0 6H18" stroke="#C8102E" stroke-width="2.4" />
  {:else if id === "fr"}
    <rect width="6" height="12" fill="#0055A4" />
    <rect x="6" width="6" height="12" fill="#fff" />
    <rect x="12" width="6" height="12" fill="#EF4135" />
  {:else if id === "zh"}
    <rect width="18" height="12" fill="#DE2910" />
    <path
      d="M3.5 1.8L4 3.31L5.59 3.32L4.31 4.26L4.79 5.78L3.5 4.85L2.21 5.78L2.69 4.26L1.41 3.32L3 3.31Z"
      fill="#FFDE00"
    />
    <circle cx="7" cy="1.6" r="0.6" fill="#FFDE00" />
    <circle cx="8.4" cy="3" r="0.6" fill="#FFDE00" />
    <circle cx="8.4" cy="5" r="0.6" fill="#FFDE00" />
    <circle cx="7" cy="6.4" r="0.6" fill="#FFDE00" />
  {:else if id === "de"}
    <rect width="18" height="4" fill="#000" />
    <rect y="4" width="18" height="4" fill="#D00" />
    <rect y="8" width="18" height="4" fill="#FFCE00" />
  {:else if id === "es"}
    <rect width="18" height="12" fill="#F1BF00" />
    <rect width="18" height="3" fill="#AA151B" />
    <rect y="9" width="18" height="3" fill="#AA151B" />
  {:else if id === "it"}
    <rect width="6" height="12" fill="#009246" />
    <rect x="6" width="6" height="12" fill="#fff" />
    <rect x="12" width="6" height="12" fill="#CE2B37" />
  {:else if id === "ru"}
    <rect width="18" height="4" fill="#fff" />
    <rect y="4" width="18" height="4" fill="#0039A6" />
    <rect y="8" width="18" height="4" fill="#D52B1E" />
  {:else if id === "ja"}
    <rect width="18" height="12" fill="#fff" />
    <circle cx="9" cy="6" r="3.2" fill="#BC002D" />
  {/if}
  <rect class="drapeau-bord" x="0.3" y="0.3" width="17.4" height="11.4" rx="1" fill="none" />
</svg>

<style>
  .drapeau {
    width: 1.1rem;
    height: 0.73rem;
    display: block;
    flex-shrink: 0;
    border-radius: 2px;
  }
  .drapeau-bord {
    stroke: rgba(0, 0, 0, 0.3);
    stroke-width: 0.6;
  }
</style>
