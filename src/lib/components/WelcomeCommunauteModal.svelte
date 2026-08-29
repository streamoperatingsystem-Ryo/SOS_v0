<script lang="ts">
  // Modal d'attribution des clips de bienvenue.
  // Affiche la liste des followers Twitch (depuis le store communaute) +
  // permet : (1) attribution manuelle (sélectionner un clip pour un follower),
  // (2) attribution automatique (clip aléatoire pour tous les followers).
  // Utilise welcomeAttribuerAuto (Rust fetch clips + résout MP4 + sauve config).
  import { followers, chargerFollowers } from "../stores/communaute";
  import {
    welcomeRegistre,
    welcomeSauverViewer,
    welcomeSupprimerViewer,
    welcomeAttribuerAuto,
    welcomeAttributionProgress,
  } from "../stores/welcome";
  import { tauri, type WelcomeViewerConfig, type WelcomeClipInfo } from "../tauri";

  let { onFermer = () => {} }: { onFermer?: () => void } = $props();

  let listeFollowers = $derived($followers?.liste ?? []);
  let registre = $derived($welcomeRegistre);
  let progress = $derived($welcomeAttributionProgress);
  let attributing = $state(false);

  // Follower sélectionné pour attribution manuelle.
  let selectedLogin = $state<string | null>(null);
  let clipsStreamer = $state<WelcomeClipInfo[]>([]);
  let loadingClips = $state(false);
  let selectedClipId = $state<string | null>(null);

  // Map registre pour lookup rapide.
  let registreMap = $derived(new Map(registre));

  function configPourLogin(login: string): WelcomeViewerConfig | undefined {
    return registreMap.get(login);
  }

  async function onSelectionnerFollower(login: string, userId: string) {
    selectedLogin = login;
    selectedClipId = configPourLogin(login)?.clip_id ?? null;
    clipsStreamer = [];
    loadingClips = true;
    try {
      clipsStreamer = await tauri.welcomeListerClipsStreamer(userId, 20);
    } catch (e) {
      console.error("[Welcome] lister clips streamer:", String(e));
    }
    loadingClips = false;
  }

  async function onAttribuerManuel() {
    if (!selectedLogin || !selectedClipId) return;
    const clip = clipsStreamer.find((c) => c.id === selectedClipId);
    if (!clip) return;
    const login = selectedLogin;
    const config: WelcomeViewerConfig = {
      actif: true,
      clip_id: clip.id,
      clip_titre: clip.title,
      clip_thumbnail: clip.thumbnail_url,
      clip_mp4_url: "", // résolu à la volée au déclenchement
      clip_duree_ms: Math.round(clip.duration * 1000),
      message: `Bienvenue ${login} !`,
      display_name: login,
      avatar: null,
    };
    try {
      await welcomeSauverViewer(login, config);
      console.log("[Welcome] attribué manuel:", login, clip.title);
    } catch (e) {
      console.error("[Welcome] attribuer manuel:", String(e));
    }
  }

  async function onAttribuerAuto() {
    if (listeFollowers.length === 0) return;
    attributing = true;
    try {
      const followersInput = listeFollowers.map((f) => ({
        login: f.login,
        user_id: f.user_id,
        display_name: f.login,
      }));
      const result = await welcomeAttribuerAuto(followersInput);
      console.log("[Welcome] attribution auto terminée:", result);
    } catch (e) {
      console.error("[Welcome] attribution auto:", String(e));
    }
    attributing = false;
  }

  async function onSupprimer(login: string) {
    try {
      await welcomeSupprimerViewer(login);
      if (selectedLogin === login) {
        selectedClipId = null;
      }
    } catch (e) {
      console.error("[Welcome] supprimer viewer:", String(e));
    }
  }

  async function onToggleActif(login: string) {
    const config = configPourLogin(login);
    if (!config) return;
    try {
      await welcomeSauverViewer(login, { ...config, actif: !config.actif });
    } catch (e) {
      console.error("[Welcome] toggle actif:", String(e));
    }
  }

  // Charger les followers si pas déjà fait.
  $effect(() => {
    if (!followers) {
      chargerFollowers();
    }
  });
</script>

<div class="overlay" role="dialog" aria-modal="true">
  <div class="modal">
    <div class="header">
      <h2>Clips de bienvenue — Attribution</h2>
      <button class="close" onclick={onFermer}>✕</button>
    </div>

    <div class="actions-bar">
      <button
        class="action"
        onclick={onAttribuerAuto}
        disabled={attributing || listeFollowers.length === 0}
      >
        {attributing ? "Attribution en cours..." : "Attribution auto (tous les followers)"}
      </button>
      {#if progress}
        <span class="progress">
          {progress.succes} succès / {progress.echecs} échecs — {progress.login}
        </span>
      {/if}
    </div>

    <div class="body">
      <div class="followers-list">
        <h3>Followers ({listeFollowers.length})</h3>
        {#if listeFollowers.length === 0}
          <p class="empty">Aucun follower. Connecter Twitch + charger la communauté.</p>
        {:else}
          <ul>
            {#each listeFollowers as f (f.login)}
              {@const config = configPourLogin(f.login)}
              <li>
                <button
                  class="follower-row"
                  class:selected={selectedLogin === f.login}
                  onclick={() => onSelectionnerFollower(f.login, f.user_id)}
                >
                  <span class="login">{f.login}</span>
                  {#if config}
                    <span class="badge" class:actif={config.actif} class:inactif={!config.actif}>
                      {config.actif ? "✓" : "○"}
                    </span>
                  {/if}
                </button>
                {#if config}
                  <button
                    class="mini-btn"
                    title={config.actif ? "Désactiver" : "Activer"}
                    onclick={() => onToggleActif(f.login)}
                  >{config.actif ? "⏸" : "▶"}</button>
                  <button
                    class="mini-btn"
                    title="Supprimer"
                    onclick={() => onSupprimer(f.login)}
                  >✕</button>
                {/if}
              </li>
            {/each}
          </ul>
        {/if}
      </div>

      {#if selectedLogin}
        <div class="clip-picker">
          <h3>Clip de {selectedLogin}</h3>
          {#if loadingClips}
            <p class="empty">Chargement des clips...</p>
          {:else if clipsStreamer.length === 0}
            <p class="empty">Aucun clip disponible pour ce streamer.</p>
          {:else}
            <ul class="clips-list">
              {#each clipsStreamer as clip (clip.id)}
                <li>
                  <label class="clip-row">
                    <input
                      type="radio"
                      name="clip"
                      value={clip.id}
                      checked={selectedClipId === clip.id}
                      onchange={() => (selectedClipId = clip.id)}
                    />
                    <img src={clip.thumbnail_url} alt="" class="thumb" />
                    <div class="clip-info">
                      <div class="clip-title">{clip.title}</div>
                      <div class="clip-meta">{Math.round(clip.duration)}s · {clip.created_at.slice(0, 10)}</div>
                    </div>
                  </label>
                </li>
              {/each}
            </ul>
            <button
              class="action"
              onclick={onAttribuerManuel}
              disabled={!selectedClipId}
            >
              Attribuer ce clip
            </button>
          {/if}
        </div>
      {/if}
    </div>
  </div>
</div>

<style>
  .overlay {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.7);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 10000;
  }
  .modal {
    background: var(--fond, #0a0a0a);
    color: var(--texte, #e0e0e0);
    border: 1px solid var(--texte, #e0e0e0);
    border-radius: 0.5rem;
    padding: 1rem;
    width: 90vw;
    max-width: 900px;
    max-height: 85vh;
    display: flex;
    flex-direction: column;
  }
  .header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-bottom: 0.8rem;
  }
  .header h2 {
    margin: 0;
    font-size: 1.1rem;
  }
  .close {
    background: none;
    border: none;
    color: inherit;
    cursor: pointer;
    font-size: 1.2rem;
  }
  .actions-bar {
    display: flex;
    align-items: center;
    gap: 0.8rem;
    margin-bottom: 0.8rem;
  }
  .progress {
    font-size: 0.8rem;
    opacity: 0.8;
  }
  .body {
    display: flex;
    gap: 1rem;
    flex: 1;
    overflow: hidden;
  }
  .followers-list {
    flex: 1;
    overflow-y: auto;
    border-right: 1px solid rgba(255, 255, 255, 0.1);
    padding-right: 0.8rem;
  }
  .followers-list h3 {
    font-size: 0.9rem;
    margin: 0 0 0.4rem;
  }
  .followers-list ul {
    list-style: none;
    margin: 0;
    padding: 0;
  }
  .followers-list li {
    display: flex;
    align-items: center;
    gap: 0.2rem;
    margin-bottom: 0.15rem;
  }
  .follower-row {
    flex: 1;
    display: flex;
    align-items: center;
    justify-content: space-between;
    background: rgba(255, 255, 255, 0.04);
    border: none;
    color: inherit;
    padding: 0.3rem 0.4rem;
    border-radius: 0.25rem;
    cursor: pointer;
    text-align: left;
  }
  .follower-row.selected {
    background: rgba(255, 255, 255, 0.15);
  }
  .follower-row:hover {
    background: rgba(255, 255, 255, 0.1);
  }
  .login {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .badge {
    font-size: 0.75rem;
    flex-shrink: 0;
  }
  .badge.actif {
    color: #53fc18;
  }
  .badge.inactif {
    opacity: 0.4;
  }
  .mini-btn {
    background: none;
    border: none;
    color: inherit;
    cursor: pointer;
    padding: 0.2rem 0.3rem;
    font-size: 0.8rem;
    opacity: 0.7;
  }
  .mini-btn:hover {
    opacity: 1;
  }
  .clip-picker {
    flex: 1;
    overflow-y: auto;
    padding-left: 0.4rem;
  }
  .clip-picker h3 {
    font-size: 0.9rem;
    margin: 0 0 0.4rem;
  }
  .clips-list {
    list-style: none;
    margin: 0 0 0.6rem;
    padding: 0;
  }
  .clips-list li {
    margin-bottom: 0.3rem;
  }
  .clip-row {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    cursor: pointer;
    padding: 0.2rem;
    border-radius: 0.25rem;
  }
  .clip-row:hover {
    background: rgba(255, 255, 255, 0.06);
  }
  .thumb {
    width: 80px;
    height: 45px;
    object-fit: cover;
    border-radius: 0.2rem;
    flex-shrink: 0;
  }
  .clip-info {
    flex: 1;
    min-width: 0;
  }
  .clip-title {
    font-size: 0.85rem;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .clip-meta {
    font-size: 0.75rem;
    opacity: 0.6;
  }
  .empty {
    opacity: 0.5;
    font-style: italic;
    font-size: 0.85rem;
  }
  .action {
    background: rgba(255, 255, 255, 0.1);
    border: 1px solid rgba(255, 255, 255, 0.2);
    color: inherit;
    padding: 0.4rem 0.8rem;
    border-radius: 0.3rem;
    cursor: pointer;
  }
  .action:disabled {
    opacity: 0.4;
    cursor: default;
  }
</style>
