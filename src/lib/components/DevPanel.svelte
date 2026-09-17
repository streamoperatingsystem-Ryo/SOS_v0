<!-- TEMPORAIRE — panneau Dev. Retirer avant release. -->
<script lang="ts">
  import { devLog } from "../dev-log";
</script>

<div class="dev-panel">
  {#each devLog as e (e.id)}
    <div class="dev-row">
      <span class="check">&#10003;</span>
      <span class="titre">{e.titre}</span>
      <span class="date">{e.date}</span>
      <span class="note">{e.note}</span>
    </div>
  {/each}
</div>

<style>
  .dev-panel {
    position: absolute;
    top: 100%;
    right: 0;
    z-index: 50;
    background: var(--fond-panneau);
    border: 1px solid var(--bordure-active);
    border-top: none;
    box-shadow: 0 0 20px rgba(139, 92, 246, 0.2);
    padding: 0.5rem 0.6rem;
    display: flex;
    flex-direction: column;
    gap: 0.3rem;
    min-width: 420px;
    max-width: 640px;
    max-height: 70vh;
    overflow-y: auto;
    font-size: 0.8rem;
  }
  .dev-row {
    display: grid;
    grid-template-columns: 1.1rem auto 6rem;
    grid-template-rows: auto auto;
    column-gap: 0.4rem;
    row-gap: 0.05rem;
    align-items: baseline;
    padding: 0.2rem 0;
    border-bottom: 1px solid var(--bordure);
  }
  .dev-row:last-child {
    border-bottom: none;
  }
  .check {
    grid-row: 1 / 3;
    color: var(--message-ok-color);
    font-weight: 600;
    text-align: center;
  }
  .titre {
    grid-column: 2;
    font-weight: 600;
  }
  .date {
    grid-column: 3;
    grid-row: 1;
    opacity: 0.6;
    text-align: right;
    font-variant-numeric: tabular-nums;
  }
  .note {
    grid-column: 2 / 4;
    grid-row: 2;
    opacity: 0.75;
    line-height: 1.3;
  }
</style>
