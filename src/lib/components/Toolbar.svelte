<script lang="ts">
  import { createWidget, createChatWidget, createCameraWidget, createInputViewerWidget, createSpeedrunWidget, createGoalWidget, sceneStore } from "../stores/scene";
  import { obsConnect, obsStatus, obsError, obsHost, obsPort, obsPassword, obsStreaming, obsRecording, toggleStreamObs, toggleRecordObs } from "../stores/obs";
  import { openSection, toggleSection, cadreModalOpen, interactionModalOpen, moderationModalOpen, consoModalOpen, ecranAttenteModalOpen } from "../stores/ui";
  import { connexions, connecterTwitch, deconnecterTwitch, twitchErreur } from "../stores/chat";
  import { connecterKick, deconnecterKick, kickErreur as kickErreurStore, lireSlugSauve } from "../stores/kick";
  import { connecterYoutube, deconnecterYoutube, demarrerChatYoutube, arreterChatYoutube } from "../stores/youtube";
  import { connecterTiktok, deconnecterTiktok, lireUsernameSauve } from "../stores/tiktok";
  import { youtubeErreur as youtubeErreurStore, tiktokErreur as tiktokErreurStore, youtubeChatActif as youtubeChatActifStore } from "../stores/chat";
  import { raccourcisVisible, raccourcisBord, raccourcisMonitorIndex, raccourcisMoniteurs, appliquerRaccourcis, masquerRaccourcis } from "../stores/raccourcis";
  import { fondSidebar, importerFondSidebar, retirerFondSidebar } from "../stores/fondSidebar";
  import type { BordRaccourcis } from "../tauri";
  import { tauri } from "../tauri";
  import { get } from "svelte/store";
  import { onMount } from "svelte";
  import { listen } from "@tauri-apps/api/event";

  let status = $derived($obsStatus);
  let error = $derived($obsError);
  let streaming = $derived($obsStreaming);
  let recording = $derived($obsRecording);
  // Verrou anti double-clic : chaque toggle = une connexion OBS one-shot.
  let outputBusy = $state(false);
  let open = $derived($openSection);
  let cx = $derived($connexions);
  let twitchErr = $derived($twitchErreur);
  let kickErr = $derived($kickErreurStore);
  let ytErr = $derived($youtubeErreurStore);
  let ytChatOn = $derived($youtubeChatActifStore);
  let ttErr = $derived($tiktokErreurStore);

  // Singleton caméra : désactive le bouton de création si un widget caméra existe.
  let hasCameraWidget = $derived($sceneStore.widgets.some((w) => w.type === "camera"));

  // ===== Cadres SVG (section Personnalisation) =====
  let cadreWidget = $derived($sceneStore.cadreWidget ?? null);
  let cadreApp = $derived($sceneStore.cadreApp ?? null);
  let cadreWidgetActif = $derived(!!cadreWidget?.actif);
  let cadreAppActif = $derived(!!cadreApp?.actif);

  // Image de fond de la colonne latérale (persistée localStorage).
  let fondImg = $derived($fondSidebar);
  async function onImportFond() {
    try {
      await importerFondSidebar();
    } catch (e) {
      alert("Import refusé : " + e);
    }
  }

  async function onConnect() {
    try {
      await obsConnect(get(obsHost), get(obsPort), get(obsPassword));
    } catch {
      // obsConnect a mis obsStatus=error + obsError. Skip sync captures.
      return;
    }
    try {
      await tauri.sceneSyncCaptures(
        get(obsHost), parseInt(get(obsPort), 10), get(obsPassword)
      );
    } catch {
    }
  }

  // Sorties OBS : toggle stream / record (état lu au connect + après chaque
  // toggle — pas de polling). Erreur déjà traduite dans obsError par le store.
  async function onToggleStream() {
    if (outputBusy) return;
    outputBusy = true;
    try { await toggleStreamObs(); } catch { /* obsError affiché */ }
    outputBusy = false;
  }
  async function onToggleRecord() {
    if (outputBusy) return;
    outputBusy = true;
    try { await toggleRecordObs(); } catch { /* obsError affiché */ }
    outputBusy = false;
  }

  // ===== Connexions réseau =====
  async function onTwitchConnect() { await connecterTwitch(); }
  async function onTwitchDisconnect() { await deconnecterTwitch(); }
  async function onKickConnect() {
    const slug = kickSlug.trim();
    if (!slug) return;
    await connecterKick(slug);
  }
  async function onKickDisconnect() { await deconnecterKick(); }
  async function onYoutubeConnect() { await connecterYoutube(); }
  async function onYoutubeDisconnect() { await deconnecterYoutube(); }
  async function onYoutubeChatToggle() {
    if (ytChatOn) {
      await arreterChatYoutube();
      youtubeChatActifStore.set(false);
    } else {
      youtubeChatActifStore.set(true);
      await demarrerChatYoutube();
    }
  }
  async function onTiktokConnect() {
    const username = tiktokUsername.trim().replace(/^@/, "");
    if (!username) return;
    await connecterTiktok(username);
  }
  async function onTiktokDisconnect() { await deconnecterTiktok(); }

  let kickSlug = $state("");
  let tiktokUsername = $state("");

  // ===== Barre de raccourcis (fenêtre TOPMOST dockée) =====
  let raccVisible = $derived($raccourcisVisible);
  let raccBord = $derived($raccourcisBord);
  let raccMonIdx = $derived($raccourcisMonitorIndex);
  let raccMons = $derived($raccourcisMoniteurs);

  const BORDS: { id: BordRaccourcis; label: string }[] = [
    { id: "gauche", label: "Gauche" },
    { id: "droite", label: "Droite" },
    { id: "haut", label: "Haut" },
    { id: "bas", label: "Bas" },
  ];

  async function onToggleBarre() {
    if (raccVisible) {
      await masquerRaccourcis();
    } else {
      await appliquerRaccourcis(raccBord, raccMonIdx);
    }
  }
  // Choisir un bord applique immédiatement : la barre saute (et s'affiche si
  // elle était masquée — changer de bord = on veut la voir bouger).
  async function onBord(b: BordRaccourcis) {
    await appliquerRaccourcis(b, raccMonIdx);
  }
  async function onMoniteur(e: Event) {
    const idx = parseInt((e.target as HTMLSelectElement).value, 10);
    if (!isNaN(idx)) await appliquerRaccourcis(raccBord, idx);
  }

  onMount(async () => {
    await listen("popout-closed", () => {});
    if (!kickSlug) kickSlug = (await lireSlugSauve()) ?? "";
    if (!tiktokUsername) tiktokUsername = (await lireUsernameSauve()) ?? "";
  });
</script>

<aside class="sidebar">
  <!-- L'édition du fond de l'application et des widgets se fait maintenant
       via la carte d'édition contextuelle (CarteEdition.svelte) en bas du
       panneau latéral — plus de section d'accordéon dédiée. -->

  <!-- Section Widgets -->
  <div class="section">
    <button class="header" class:ouvert={open === "widgets"} onclick={() => toggleSection("widgets")}>
      <svg class="ico" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="3" width="7" height="7" rx="1"/><rect x="14" y="3" width="7" height="7" rx="1"/><rect x="3" y="14" width="7" height="7" rx="1"/><rect x="14" y="14" width="7" height="7" rx="1"/></svg>
      <span>Widgets</span>
      <span class="chevron">›</span>
    </button>
    {#if open === "widgets"}
      <div class="content">
        <button class="action" onclick={createWidget}>Créer un widget</button>
        <button class="action" onclick={createChatWidget}>Créer un widget chat</button>
        <button
          class="action"
          onclick={createCameraWidget}
          disabled={hasCameraWidget}
        >Créer un widget caméra</button>
        <button class="action" onclick={createInputViewerWidget}>Créer un widget input viewer</button>
        <button class="action" onclick={createSpeedrunWidget}>Créer un widget speedrun</button>
        <button class="action" onclick={createGoalWidget}>Créer un widget objectif</button>
        <p class="hint">Cliquer un widget ou le fond pour l'éditer</p>
      </div>
    {/if}
  </div>

  <!-- Section Connexions -->
  <div class="section">
    <button class="header" class:ouvert={open === "connexions"} onclick={() => toggleSection("connexions")}>
      <svg class="ico" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"><path d="M10 13a5 5 0 0 0 7.5.5l3-3a5 5 0 0 0-7-7l-1.7 1.7"/><path d="M14 11a5 5 0 0 0-7.5-.5l-3 3a5 5 0 0 0 7 7l1.7-1.7"/></svg>
      <span>Connexions</span>
      <span class="chevron">›</span>
    </button>
    {#if open === "connexions"}
      <div class="content">
        <div class="connexion-row">
          <span class="dot plat-twitch" class:on={cx.twitch} class:off={!cx.twitch}></span>
          <span class="plat-nom">Twitch</span>
          {#if cx.twitch}
            <button class="action small" onclick={onTwitchDisconnect}>Déconnecter</button>
          {:else}
            <button class="action small" onclick={onTwitchConnect}>Connecter</button>
          {/if}
        </div>
        <div class="connexion-row">
          <span class="dot plat-youtube" class:on={cx.youtube} class:off={!cx.youtube}></span>
          <span class="plat-nom">YouTube</span>
          {#if cx.youtube}
            <button class="action small" onclick={onYoutubeDisconnect}>Déconnecter</button>
            <button class="action small" class:on={ytChatOn} class:off={!ytChatOn} onclick={onYoutubeChatToggle}>
              {ytChatOn ? "Chat ON" : "Chat OFF"}
            </button>
          {:else}
            <button class="action small" onclick={onYoutubeConnect}>Connecter</button>
          {/if}
        </div>
        <div class="connexion-row">
          <span class="dot plat-kick" class:on={cx.kick} class:off={!cx.kick}></span>
          <span class="plat-nom">Kick</span>
          {#if cx.kick}
            <button class="action small" onclick={onKickDisconnect}>Déconnecter</button>
          {:else}
            <input class="plat-input" type="text" placeholder="slug" bind:value={kickSlug} spellcheck="false" />
            <button class="action small" onclick={onKickConnect}>Connecter</button>
          {/if}
        </div>
        <div class="connexion-row">
          <span class="dot plat-tiktok" class:on={cx.tiktok} class:off={!cx.tiktok}></span>
          <span class="plat-nom">TikTok</span>
          {#if cx.tiktok}
            <button class="action small" onclick={onTiktokDisconnect}>Déconnecter</button>
          {:else}
            <input class="plat-input" type="text" placeholder="username" bind:value={tiktokUsername} spellcheck="false" />
            <button class="action small" onclick={onTiktokConnect}>Connecter</button>
          {/if}
        </div>
        {#if twitchErr}
          <span class="obs-status">{twitchErr}</span>
        {/if}
        {#if kickErr}
          <span class="obs-status">{kickErr}</span>
        {/if}
        {#if ytErr}
          <span class="obs-status">{ytErr}</span>
        {/if}
        {#if ttErr}
          <span class="obs-status">{ttErr}</span>
        {/if}
      </div>
    {/if}
  </div>

  <!-- Section Interactions chat : LANCEUR DIRECT (pas un accordéon — le clic
       ouvre la modale à onglets : Clip de bienvenue / Bandeau / Alertes).
       Tout est centralisé dans la modale, qui s'explique elle-même. -->
  <div class="section">
    <button
      class="header"
      onclick={() => interactionModalOpen.set(true)}
      title="Ouvrir les interactions chat (clips de bienvenue, alertes…)"
    >
      <svg class="ico" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"><path d="M21 11.5a8.38 8.38 0 0 1-8.5 8.5 8.5 8.5 0 0 1-3.8-.9L3 21l1.9-5.7a8.38 8.38 0 0 1-.9-3.8 8.5 8.5 0 0 1 8.5-8.5h.5a8.48 8.48 0 0 1 8 8v.5z"/></svg>
      <span>Interactions chat</span>
      <span class="chevron">›</span>
    </button>
  </div>

  <!-- Section Écran d'attente : LANCEUR DIRECT (pas un accordéon — le clic
       ouvre la modale de config/contrôle BRB). Le média est affiché sur la
       diffusion uniquement ; le chrono est détenu par Rust. -->
  <div class="section">
    <button
      class="header"
      onclick={() => ecranAttenteModalOpen.set(true)}
      title="Ouvrir l'écran d'attente (média + chrono sur la diffusion)"
    >
      <svg class="ico" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="9"/><path d="M12 7v5l3 3"/></svg>
      <span>Écran d'attente</span>
      <span class="chevron">›</span>
    </button>
  </div>

  <!-- Section Modération & Rôles : LANCEUR DIRECT (pas un accordéon — le clic
       ouvre la modale à onglets : Twitch / Kick / YouTube / TikTok / Trovo).
       Gestion des statuts/rôles : ban, timeout, VIP, modérateur, suppression
       de messages, clear chat. -->
  <div class="section">
    <button
      class="header"
      onclick={() => moderationModalOpen.set(true)}
      title="Ouvrir la modération & rôles (ban, VIP, modérateur…)"
    >
      <svg class="ico" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"><path d="M12 22s8-4 8-10V5l-8-3-8 3v7c0 6 8 10 8 10z"/></svg>
      <span>Modération & Rôles</span>
      <span class="chevron">›</span>
    </button>
  </div>

  <!-- Section Diagnostic : LANCEUR DIRECT (pas un accordéon — le clic ouvre
       la modale de consommation : courbes CPU/RAM par processus et par
       fonction StreamOS + compteurs d'activité, mesures réelles 1 Hz). -->
  <div class="section">
    <button
      class="header"
      onclick={() => consoModalOpen.set(true)}
      title="Ouvrir le diagnostic de consommation (CPU, RAM, activité…)"
    >
      <svg class="ico" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"><path d="M22 12h-4l-3 9L9 3l-3 9H2"/></svg>
      <span>Diagnostic</span>
      <span class="chevron">›</span>
    </button>
  </div>

  <!-- Section OBS -->
  <div class="section">
    <!-- En-tête coloré par l'état OBS : rouge = non connecté (OBS pas ouvert,
         WebSocket off ou auth échouée), vert = connecté. L'utilisateur voit le
         problème d'un coup d'œil et clique pour voir le message actionnable. -->
    <button
      class="header"
      class:etat-erreur={status === "error" || status === "idle"}
      class:etat-ok={status === "connected"}
      class:ouvert={open === "obs"}
      onclick={() => toggleSection("obs")}
    >
      <svg class="ico" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="2"/><path d="M4.93 19.07a10 10 0 0 1 0-14.14"/><path d="M19.07 4.93a10 10 0 0 1 0 14.14"/><path d="M7.76 16.24a6 6 0 0 1 0-8.49"/><path d="M16.24 7.76a6 6 0 0 1 0 8.49"/></svg>
      <span>OBS</span>
      <span class="chevron">›</span>
    </button>
    {#if open === "obs"}
      <div class="content">
        <input bind:value={$obsHost} placeholder="Hôte" spellcheck="false" />
        <input bind:value={$obsPort} placeholder="Port" spellcheck="false" />
        <input
          bind:value={$obsPassword}
          type="password"
          placeholder="Mot de passe"
          spellcheck="false"
        />
        <button
          class="action"
          onclick={onConnect}
          disabled={status === "connecting"}
        >
          {status === "connecting" ? "Connexion…" : "Connecter"}
        </button>
        <!-- Sorties OBS : toggles stream + record. La pastille verte (.dot.on)
             reflète outputActive lu au connect / après chaque toggle. -->
        <button
          class="action output-btn"
          class:actif={streaming}
          onclick={onToggleStream}
          disabled={status !== "connected" || outputBusy}
        >
          <span class="dot" class:on={streaming}></span>
          {streaming ? "Arrêter le stream" : "Démarrer le stream"}
        </button>
        <button
          class="action output-btn"
          class:actif={recording}
          onclick={onToggleRecord}
          disabled={status !== "connected" || outputBusy}
        >
          <span class="dot" class:on={recording}></span>
          {recording ? "Arrêter l'enregistrement" : "Démarrer l'enregistrement"}
        </button>
        <span
          class="obs-status"
          class:erreur={status === "error"}
          class:ok={status === "connected"}
        >
          {#if status === "connected"}
            OBS OK
          {:else if status === "error"}
            {error}
          {/if}
        </span>
      </div>
    {/if}
  </div>

  <!-- Section Personnalisation (cadres SVG) -->
  <div class="section">
    <button class="header" class:ouvert={open === "personnalisation"} onclick={() => toggleSection("personnalisation")}>
      <svg class="ico" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"><line x1="21" x2="14" y1="6" y2="6"/><line x1="10" x2="3" y1="6" y2="6"/><line x1="21" x2="12" y1="12" y2="12"/><line x1="8" x2="3" y1="12" y2="12"/><line x1="21" x2="16" y1="18" y2="18"/><line x1="4" x2="3" y1="18" y2="18"/><line x1="14" x2="14" y1="4" y2="8"/><line x1="8" x2="8" y1="10" y2="14"/><line x1="16" x2="16" y1="16" y2="20"/></svg>
      <span>Personnalisation</span>
      <span class="chevron">›</span>
    </button>
    {#if open === "personnalisation"}
      <div class="content">
        <!-- Galerie unique : le choix widgets / bord de l'application se fait
             dans la modale (onglets), avec l'activation par onglet. -->
        <button class="action" onclick={() => cadreModalOpen.set("widget")}>
          Vos cadres
        </button>
        <button class="action" onclick={onImportFond}>
          {fondImg ? "Changer l'image de fond" : "Image de fond"}
        </button>
        {#if fondImg}
          <button class="action" onclick={retirerFondSidebar}>
            Retirer l'image de fond
          </button>
        {/if}
        <p class="hint">
          {#if cadreWidgetActif && cadreAppActif}
            Cadre des widgets + bord de l'application actifs
          {:else if cadreWidgetActif}
            Cadre des widgets actif
          {:else if cadreAppActif}
            Bord de l'application actif
          {:else}
            Aucun cadre actif
          {/if}
        </p>
      </div>
    {/if}
  </div>

  <!-- Section Raccourcis : fenêtre TOPMOST dockée à un bord d'écran.
       Afficher/masquer + choix du bord (appliqué immédiatement) + moniteur. -->
  <div class="section">
    <button class="header" class:ouvert={open === "raccourcis"} onclick={() => toggleSection("raccourcis")}>
      <svg class="ico" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"><path d="M18 3a3 3 0 0 0-3 3v12a3 3 0 0 0 3 3 3 3 0 0 0 3-3 3 3 0 0 0-3-3H6a3 3 0 0 0-3 3 3 3 0 0 0 3 3 3 3 0 0 0 3-3V6a3 3 0 0 0-3-3 3 3 0 0 0-3 3 3 3 0 0 0 3 3h12a3 3 0 0 0 3-3 3 3 0 0 0-3-3z"/></svg>
      <span>Raccourcis</span>
      <span class="chevron">›</span>
    </button>
    {#if open === "raccourcis"}
      <div class="content">
        <button class="action" onclick={onToggleBarre}>
          {raccVisible ? "Masquer la barre" : "Afficher la barre"}
        </button>
        <div class="bord-row">
          {#each BORDS as b (b.id)}
            <button
              class="action small"
              class:actif={raccBord === b.id}
              onclick={() => onBord(b.id)}
            >{b.label}</button>
          {/each}
        </div>
        <select onchange={onMoniteur}>
          {#each raccMons as m (m.index)}
            <option value={m.index} selected={m.index === raccMonIdx}>
              {m.nom ?? `Moniteur ${m.index + 1}`} — {m.largeur}×{m.hauteur}{m.primaire ? " (principal)" : ""}
            </option>
          {/each}
        </select>
        <p class="hint">
          Barre toujours au-dessus, dockée au bord choisi. Visible au-dessus
          des jeux en fenêtré sans bordure — pas en plein écran exclusif.
        </p>
      </div>
    {/if}
  </div>
</aside>

<style>
  .sidebar {
    width: 100%;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 0.25rem;
    overflow-y: auto;
    flex: 1;
    min-height: 0;
  }
  /* Encadré de section : bordure teintée par l'accent utilisateur
     (--bordure-ui = couleur du cadre), plus marquée quand la section est
     ouverte — le contenu déplié reste dans le même cadre. */
  .section {
    display: flex;
    flex-direction: column;
    border: 1px solid var(--bordure-ui);
    border-radius: var(--rayon-petit);
    transition: border-color 0.15s ease;
  }
  .section:has(.header.ouvert) {
    border-color: var(--bordure-ui-active);
  }
  /* Item de liste : icône + label + chevron à droite. Plat au repos
     (transparent, comme la maquette) — la pilule verre n'apparaît qu'au
     survol ; la section ouverte prend une pilule neutre discrète
     (.ouvert). La couleur vit dans les textes sémantiques (états OBS,
     pastilles plateformes). */
  .header {
    background: transparent;
    color: var(--texte);
    border: 1px solid transparent;
    border-radius: var(--rayon-petit);
    padding: 0.4rem 0.55rem;
    font: inherit;
    cursor: pointer;
    text-align: left;
    display: flex;
    align-items: center;
    gap: 0.5rem;
    transition: background 0.15s ease, color 0.15s ease, border-color 0.15s ease;
  }
  .header:hover {
    background: var(--btn-surface-hover);
    border-color: var(--bordure-ui-active);
  }
  .header:active {
    box-shadow: var(--btn-inset-active);
  }
  /* Section ouverte : pilule neutre discrète — repère actif de la liste. */
  .header.ouvert {
    background: color-mix(in srgb, var(--texte) 7%, transparent);
    border-color: var(--bordure-ui-active);
  }
  .header.ouvert:hover {
    background: color-mix(in srgb, var(--texte) 11%, transparent);
  }
  /* État OBS sur l'en-tête : connecté = texte vert sémantique ; non
     connecté = texte rouge + liseré d'alerte à gauche de la ligne. */
  .header.etat-erreur {
    color: var(--message-error-color);
    box-shadow: inset 3px 0 0 var(--message-error-color);
  }
  .header.etat-erreur:hover {
    color: var(--message-error-color);
    box-shadow:
      inset 3px 0 0 var(--message-error-color),
      0 0 8px color-mix(in srgb, var(--message-error-color) 35%, transparent);
  }
  .header.etat-ok {
    color: var(--message-ok-color);
  }
  .header.etat-ok:hover {
    color: var(--message-ok-color);
  }
  .ico {
    opacity: 0.85;
  }
  .chevron {
    margin-left: auto;
    font-size: 0.8rem;
    opacity: 0.55;
    transition: transform 0.15s ease;
  }
  .header.ouvert .chevron {
    transform: rotate(90deg);
  }
  .content {
    display: flex;
    flex-direction: column;
    gap: 0.35rem;
    padding: 0.35rem 0.55rem 0.5rem;
  }
  .action {
    /* Bouton d'action — pilule verre sombre translucide commune. */
    background: var(--btn-surface);
    color: var(--texte);
    border: 1px solid var(--bordure-ui-active);
    box-shadow: var(--btn-inset);
    padding: 0.35rem 0.6rem;
    font: inherit;
    cursor: pointer;
    text-align: left;
    transition: background 0.15s ease, border-color 0.15s ease, box-shadow 0.15s ease;
  }
  .action:hover {
    background: var(--btn-surface-hover);
    border-color: var(--bordure-ui-active);
    box-shadow: var(--btn-inset-hover);
  }
  .action:active {
    box-shadow: var(--btn-inset-active);
  }
  .action:disabled {
    opacity: 0.35;
    cursor: not-allowed;
  }
  .action:disabled:hover {
    background: var(--btn-surface);
    border-color: var(--bordure-ui-active);
    box-shadow: var(--btn-inset);
  }
  .hint {
    font-size: 0.8rem;
    opacity: 0.6;
    line-height: 1.3;
  }
  input {
    background: var(--fond-controle);
    color: var(--texte);
    border: 1px solid var(--bordure-ui);
    padding: 0.35rem 0.5rem;
    font: inherit;
    font-size: 0.85rem;
    width: 100%;
  }
  input:focus {
    outline: none;
    border-color: var(--accent-ui);
  }
  .obs-status {
    font-size: 0.8rem;
    opacity: 0.7;
    word-break: break-word;
  }
  .obs-status.erreur {
    opacity: 1;
    color: var(--message-user-action-color);
    line-height: 1.4;
  }
  .obs-status.ok {
    opacity: 1;
    color: var(--message-ok-color);
  }
  .connexion-row {
    display: flex;
    align-items: center;
    gap: 0.4rem;
    padding: 0.2rem 0;
  }
  /* .dot (pastille statut) : classe partagée définie dans app.css. */
  .plat-nom {
    flex: 1;
    font-size: 0.85rem;
  }
  .action.small {
    padding: 0.15rem 0.4rem;
    font-size: 0.78rem;
  }
  .plat-input {
    width: 70px;
    flex-shrink: 0;
    padding: 0.1rem 0.3rem;
    font-size: 0.78rem;
  }
  /* ===== Section Raccourcis ===== */
  .bord-row {
    display: flex;
    flex-wrap: wrap;
    gap: 0.3rem;
  }
  .action.small.actif {
    border: 1px solid var(--accent-ui);
    color: var(--accent-ui);
  }
  /* Toggles sorties OBS (stream/record) : pastille + label accentués quand
     la sortie est active — même recette que .action.small.actif. */
  .output-btn {
    display: flex;
    align-items: center;
    gap: 0.45rem;
  }
  .output-btn.actif {
    border-color: var(--accent-ui);
    color: var(--accent-ui);
  }
  select {
    background: var(--fond-controle);
    color: var(--texte);
    border: 1px solid var(--bordure-ui);
    padding: 0.3rem 0.4rem;
    font: inherit;
    font-size: 0.85rem;
    width: 100%;
  }
  select:focus {
    outline: none;
    border-color: var(--accent-ui);
  }
</style>
