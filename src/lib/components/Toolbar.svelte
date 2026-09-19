<script lang="ts">
  import { createWidget, createChatWidget, createCameraWidget, createInputViewerWidget, createSpeedrunWidget, sceneStore } from "../stores/scene";
  import { obsConnect, obsStatus, obsError, obsHost, obsPort, obsPassword } from "../stores/obs";
  import { openSection, toggleSection, cadreModalOpen, interactionModalOpen, moderationModalOpen } from "../stores/ui";
  import { connexions, connecterTwitch, deconnecterTwitch, twitchErreur } from "../stores/chat";
  import { connecterKick, deconnecterKick, kickErreur as kickErreurStore, lireSlugSauve } from "../stores/kick";
  import { connecterYoutube, deconnecterYoutube, demarrerChatYoutube, arreterChatYoutube } from "../stores/youtube";
  import { connecterTiktok, deconnecterTiktok, lireUsernameSauve } from "../stores/tiktok";
  import { youtubeErreur as youtubeErreurStore, tiktokErreur as tiktokErreurStore, youtubeChatActif as youtubeChatActifStore } from "../stores/chat";
  import { raccourcisVisible, raccourcisBord, raccourcisMonitorIndex, raccourcisMoniteurs, appliquerRaccourcis, masquerRaccourcis } from "../stores/raccourcis";
  import type { BordRaccourcis } from "../tauri";
  import { tauri } from "../tauri";
  import { get } from "svelte/store";
  import { onMount } from "svelte";
  import { listen } from "@tauri-apps/api/event";

  let status = $derived($obsStatus);
  let error = $derived($obsError);
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
    <button class="header" onclick={() => toggleSection("widgets")}>
      <span class="arrow">{open === "widgets" ? "▼" : "▶"}</span>
      <span>Widgets</span>
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
        <p class="hint">Cliquer un widget ou le fond pour l'éditer</p>
      </div>
    {/if}
  </div>

  <!-- Section Connexions -->
  <div class="section">
    <button class="header" onclick={() => toggleSection("connexions")}>
      <span class="arrow">{open === "connexions" ? "▼" : "▶"}</span>
      <span>Connexions</span>
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
      class="header lanceur"
      onclick={() => interactionModalOpen.set(true)}
      title="Ouvrir les interactions chat (clips de bienvenue, alertes…)"
    >
      <span class="arrow">▶</span>
      <span>Interactions chat</span>
      <span class="lanceur-fleche">▸</span>
    </button>
  </div>

  <!-- Section Modération & Rôles : LANCEUR DIRECT (pas un accordéon — le clic
       ouvre la modale à onglets : Twitch / Kick / YouTube / TikTok / Trovo).
       Gestion des statuts/rôles : ban, timeout, VIP, modérateur, suppression
       de messages, clear chat. -->
  <div class="section">
    <button
      class="header lanceur"
      onclick={() => moderationModalOpen.set(true)}
      title="Ouvrir la modération & rôles (ban, VIP, modérateur…)"
    >
      <span class="arrow">▶</span>
      <span>Modération & Rôles</span>
      <span class="lanceur-fleche">▸</span>
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
      onclick={() => toggleSection("obs")}
    >
      <span class="arrow">{open === "obs" ? "▼" : "▶"}</span>
      <span>OBS</span>
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
    <button class="header" onclick={() => toggleSection("personnalisation")}>
      <span class="arrow">{open === "personnalisation" ? "▼" : "▶"}</span>
      <span>Personnalisation</span>
    </button>
    {#if open === "personnalisation"}
      <div class="content">
        <!-- Galerie unique : le choix widgets / bord de l'application se fait
             dans la modale (onglets), avec l'activation par onglet. -->
        <button class="action" onclick={() => cadreModalOpen.set("widget")}>
          Vos cadres
        </button>
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
    <button class="header" onclick={() => toggleSection("raccourcis")}>
      <span class="arrow">{open === "raccourcis" ? "▼" : "▶"}</span>
      <span>Raccourcis</span>
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
  .section {
    display: flex;
    flex-direction: column;
  }
  .header {
    /* Recette « verre Aero HUD » (cf. app.css → --btn-*). Teinte indigo
       (--dash-accent) par défaut, bordure dégradée dash (border-box).
       padding-box = surface Aero teintée, border-box = dégradé dash. */
    --btn-tint: var(--dash-accent);
    --btn-border: var(--dash-gradient);
    background:
      var(--btn-surface) padding-box,
      var(--btn-border) border-box;
    color: var(--texte);
    border: 1px solid transparent;
    box-shadow: var(--btn-inset);
    padding: 0.4rem 0.6rem;
    font: inherit;
    cursor: pointer;
    text-align: left;
    display: flex;
    align-items: center;
    gap: 0.4rem;
    transition: box-shadow 0.2s ease, background 0.2s ease;
  }
  .header:hover {
    /* Hover : glow indigo atténué + surface qui s'illumine (verre). */
    background:
      var(--btn-surface-hover) padding-box,
      var(--btn-border) border-box;
    box-shadow: var(--btn-inset-hover), var(--dash-glow), 0 0 0 1px color-mix(in srgb, var(--dash-accent) 30%, transparent);
  }
  .header:active {
    /* Pressed : le verre s'enfonce. */
    background:
      linear-gradient(180deg, var(--fond-controle) 0%, color-mix(in srgb, var(--btn-tint) 8%, var(--fond-controle)) 100%) padding-box,
      var(--btn-border) border-box;
    box-shadow: inset 0 2px 4px rgba(0, 0, 0, 0.4);
  }
  /* État OBS sur l'en-tête (variables --dash-*, palette froide assombrie).
     Bordeaux = non connecté (idle/error) : teinte + bordure bordeaux.
     Vert foncé = connecté (texte seulement, le dégradé dash reste).
     Le hover PRÉSERVE la couleur d'état. */
  .header.etat-erreur {
    --btn-tint: var(--dash-danger);
    --btn-border: linear-gradient(var(--dash-danger), var(--dash-danger));
    color: var(--dash-danger);
  }
  .header.etat-erreur:hover {
    color: var(--dash-danger);
    box-shadow: var(--btn-inset-hover), var(--dash-glow-danger);
  }
  .header.etat-ok {
    color: var(--dash-success);
  }
  .header.etat-ok:hover {
    color: var(--dash-success);
  }
  /* Lanceur Interactions chat : ouvre la modale (pas un accordéon).
     Espacement IDENTIQUE aux autres sections (gap 0.4rem à gauche) ;
     seule la flèche de droite est poussée au bord (margin-left:auto). */
  .header.lanceur .lanceur-fleche {
    margin-left: auto;
  }
  .lanceur-fleche {
    font-size: 0.75rem;
    opacity: 0.7;
  }
  /* ===== Slider compact une ligne (label + slider + valeur) ===== */
  .arrow {
    font-size: 0.75rem;
    width: 0.85rem;
    text-align: center;
  }
  .content {
    display: flex;
    flex-direction: column;
    gap: 0.4rem;
    padding: 0.5rem 0.6rem;
    border: 1px solid var(--bordure);
    border-top: none;
  }
  .action {
    /* Recette « verre Aero HUD » — cohérent avec les en-têtes de section. */
    --btn-tint: var(--dash-accent);
    --btn-border: var(--dash-gradient);
    background:
      var(--btn-surface) padding-box,
      var(--btn-border) border-box;
    color: var(--texte);
    border: 1px solid transparent;
    box-shadow: var(--btn-inset);
    padding: 0.35rem 0.6rem;
    font: inherit;
    cursor: pointer;
    text-align: left;
    transition: box-shadow 0.2s ease, background 0.2s ease;
  }
  .action:hover {
    background:
      var(--btn-surface-hover) padding-box,
      var(--btn-border) border-box;
    box-shadow: var(--btn-inset-hover), var(--dash-glow), 0 0 0 1px color-mix(in srgb, var(--dash-accent) 30%, transparent);
  }
  .action:active {
    background:
      linear-gradient(180deg, var(--fond-controle) 0%, color-mix(in srgb, var(--btn-tint) 8%, var(--fond-controle)) 100%) padding-box,
      var(--btn-border) border-box;
    box-shadow: inset 0 2px 4px rgba(0, 0, 0, 0.4);
  }
  .action:disabled {
    opacity: 0.35;
    cursor: not-allowed;
    box-shadow: none;
  }
  .action:disabled:hover {
    background:
      var(--btn-surface) padding-box,
      var(--btn-border) border-box;
    box-shadow: none;
  }
  .hint {
    font-size: 0.8rem;
    opacity: 0.6;
    line-height: 1.3;
  }
  input {
    background: var(--fond-controle);
    color: var(--texte);
    border: 1px solid var(--bordure);
    padding: 0.35rem 0.5rem;
    font: inherit;
    font-size: 0.85rem;
    width: 100%;
  }
  input:focus {
    outline: none;
    border-color: var(--dash-accent);
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
  .dot {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    border: 1px solid var(--bordure);
    background: var(--gris-fonce);
    flex-shrink: 0;
    transition: background-color 0.15s ease, box-shadow 0.15s ease;
  }
  .dot.on {
    background: var(--message-ok-color);
  }
  .dot.off {
    background: var(--gris-fonce);
  }
  /* Couleurs de marque (décoratif) — connectée = couleur plateforme + glow. */
  .dot.plat-twitch.on { background: var(--plat-twitch); box-shadow: 0 0 6px rgba(168, 85, 247, 0.6); }
  .dot.plat-youtube.on { background: var(--plat-youtube); box-shadow: 0 0 6px rgba(244, 63, 94, 0.6); }
  .dot.plat-kick.on { background: var(--plat-kick); box-shadow: 0 0 6px rgba(34, 197, 94, 0.6); }
  .dot.plat-tiktok.on { background: var(--plat-tiktok); box-shadow: 0 0 6px rgba(236, 72, 153, 0.6); }
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
    border: 1px solid var(--dash-accent);
    color: var(--dash-accent);
  }
  select {
    background: var(--fond-controle);
    color: var(--texte);
    border: 1px solid var(--bordure);
    padding: 0.3rem 0.4rem;
    font: inherit;
    font-size: 0.85rem;
    width: 100%;
  }
  select:focus {
    outline: none;
    border-color: var(--dash-accent);
  }
</style>
