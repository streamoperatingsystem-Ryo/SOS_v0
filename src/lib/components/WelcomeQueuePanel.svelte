<script lang="ts">
  // Panel file d'attente des clips de bienvenue.
  // Affiche : config globale (on/off), clip en cours, queue, contrôles
  // (stop, skip, retirer, remonter, descendre, vider, reset session).
  // Écoute le store welcomeEtat (pushed par Rust via event welcome:etat).
  import {
    welcomeEtat,
    welcomeStop,
    welcomeSkip,
    welcomeRetirer,
    welcomeRemonter,
    welcomeDescendre,
    welcomeVider,
    welcomeResetSession,
    welcomeSetConfigGlobaleActif,
  } from "../stores/welcome";

  let etat = $derived($welcomeEtat);
  let actif = $derived(etat?.config_globale_actif ?? false);
  let current = $derived(etat?.current ?? null);
  let queue = $derived(etat?.en_attente ?? []);
</script>

<div class="welcome-queue-panel">
  <div class="header">
    <h3>Clips de bienvenue</h3>
    <label class="toggle">
      <input
        type="checkbox"
        checked={actif}
        onchange={(e) => welcomeSetConfigGlobaleActif(e.currentTarget.checked)}
      />
      <span>Activé</span>
    </label>
  </div>

  {#if current}
    <div class="current">
      <div class="current-label">En cours</div>
      <div class="item">
        {#if current.avatar}
          <img src={current.avatar} alt="" class="avatar" />
        {/if}
        <div class="item-info">
          <div class="item-pseudo">{current.display_name}</div>
          <div class="item-clip">{current.clip_titre}</div>
        </div>
        <div class="item-actions">
          <button title="Skip" onclick={() => welcomeSkip()}>⏭</button>
          <button title="Stop tout" onclick={() => welcomeStop()}>⏹</button>
        </div>
      </div>
    </div>
  {/if}

  <div class="queue-section">
    <div class="queue-header">
      <span>File d'attente ({queue.length})</span>
      <div class="queue-actions">
        <button title="Vider la queue" onclick={() => welcomeVider()} disabled={queue.length === 0}>
          Vider
        </button>
        <button title="Reset session (tous redeviennent éligibles)" onclick={() => welcomeResetSession()}>
          Reset session
        </button>
      </div>
    </div>

    {#if queue.length === 0}
      <p class="empty">Aucun clip en attente</p>
    {:else}
      <ul class="queue-list">
        {#each queue as item, i (item.id)}
          <li class="item">
            {#if item.avatar}
              <img src={item.avatar} alt="" class="avatar" />
            {/if}
            <div class="item-info">
              <div class="item-pseudo">{item.display_name}</div>
              <div class="item-clip">{item.clip_titre}</div>
            </div>
            <div class="item-actions">
              <button
                title="Remonter"
                onclick={() => welcomeRemonter(item.id)}
                disabled={i === 0}
              >↑</button>
              <button
                title="Descendre"
                onclick={() => welcomeDescendre(item.id)}
                disabled={i === queue.length - 1}
              >↓</button>
              <button title="Retirer" onclick={() => welcomeRetirer(item.id)}>✕</button>
            </div>
          </li>
        {/each}
      </ul>
    {/if}
  </div>
</div>

<style>
  .welcome-queue-panel {
    padding: 0.5rem;
    font-size: 0.85rem;
  }
  .header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-bottom: 0.5rem;
  }
  .header h3 {
    margin: 0;
    font-size: 0.95rem;
  }
  .toggle {
    display: flex;
    align-items: center;
    gap: 0.3rem;
    cursor: pointer;
  }
  .current {
    margin-bottom: 0.6rem;
    padding: 0.4rem;
    background: rgba(255, 255, 255, 0.08);
    border-radius: 0.3rem;
  }
  .current-label {
    font-size: 0.75rem;
    opacity: 0.7;
    margin-bottom: 0.2rem;
  }
  .queue-section {
    margin-top: 0.4rem;
  }
  .queue-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-bottom: 0.3rem;
  }
  .queue-actions {
    display: flex;
    gap: 0.2rem;
  }
  .queue-list {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 0.2rem;
  }
  .item {
    display: flex;
    align-items: center;
    gap: 0.4rem;
    padding: 0.3rem;
    background: rgba(255, 255, 255, 0.04);
    border-radius: 0.25rem;
  }
  .avatar {
    width: 24px;
    height: 24px;
    border-radius: 50%;
    object-fit: cover;
    flex-shrink: 0;
  }
  .item-info {
    flex: 1;
    min-width: 0;
  }
  .item-pseudo {
    font-weight: 600;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .item-clip {
    font-size: 0.75rem;
    opacity: 0.7;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .item-actions {
    display: flex;
    gap: 0.15rem;
    flex-shrink: 0;
  }
  .item-actions button {
    padding: 0.1rem 0.3rem;
    font-size: 0.75rem;
    cursor: pointer;
  }
  .item-actions button:disabled {
    opacity: 0.3;
    cursor: default;
  }
  .empty {
    opacity: 0.5;
    font-style: italic;
    padding: 0.3rem;
  }
</style>
