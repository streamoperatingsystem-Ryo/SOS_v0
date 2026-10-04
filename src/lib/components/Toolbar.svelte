<script lang="ts">
  import { createWidget, createChatWidget, createCameraWidget, createInputViewerWidget, createSpeedrunWidget, createGoalWidget, sceneStore, syncCapturesOBS } from "../stores/scene";
  import { obsConnect, obsStatus, obsError, obsHost, obsPort, obsPassword, obsStreaming, obsRecording, toggleStreamObs, toggleRecordObs } from "../stores/obs";
  import { openSection, toggleSection, cadreModalOpen, interactionModalOpen, moderationModalOpen, consoModalOpen, ecranAttenteModalOpen, guideLivreOpen } from "../stores/ui";
  import { jcActif, toggleJustChatting } from "../stores/justChatting";
  import { connexions, connecterTwitch, deconnecterTwitch, twitchErreur } from "../stores/chat";
  import { connecterKick, deconnecterKick, kickErreur as kickErreurStore, lireSlugSauve } from "../stores/kick";
  import { connecterYoutube, deconnecterYoutube, demarrerChatYoutube, arreterChatYoutube } from "../stores/youtube";
  import { connecterTiktok, deconnecterTiktok, lireUsernameSauve } from "../stores/tiktok";
  import { youtubeErreur as youtubeErreurStore, tiktokErreur as tiktokErreurStore, youtubeChatActif as youtubeChatActifStore } from "../stores/chat";
  import { raccourcisVisible, raccourcisBord, raccourcisMonitorIndex, raccourcisMoniteurs, appliquerRaccourcis, masquerRaccourcis } from "../stores/raccourcis";
  import { fondSidebar, importerFondSidebar, retirerFondSidebar } from "../stores/fondSidebar";
  import { fondDash, importerFondDash, retirerFondDash } from "../stores/fondDash";
  import type { BordRaccourcis } from "../tauri";
  import { t } from "../stores/langue";
  import { get } from "svelte/store";
  import { onMount } from "svelte";
  import { listen } from "@tauri-apps/api/event";
  import { openUrl } from "@tauri-apps/plugin-opener";

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

  // ===== Cadre SVG unifié (section Personnalisation) =====
  let cadre = $derived($sceneStore.cadreWidget ?? null);
  let cadreActif = $derived(!!cadre?.actif);

  // Image de fond de la colonne latérale (persistée localStorage).
  let fondImg = $derived($fondSidebar);
  async function onImportFond() {
    try {
      await importerFondSidebar();
    } catch (e) {
      alert(get(t)("commun.importRefuse", { erreur: String(e) }));
    }
  }

  // Image de fond du CHROME dashboard (topbar + sidebar + marges autour du
  // canvas) — bouton de pied de panneau, distinct du décor d'ambiance
  // (fondSidebar). Le canvas reste opaque par-dessus : jamais touché.
  let fondDashImg = $derived($fondDash);
  async function onImportFondDash() {
    try {
      await importerFondDash();
    } catch (e) {
      alert(get(t)("commun.importRefuse", { erreur: String(e) }));
    }
  }

  async function onConnect() {
    try {
      await obsConnect(get(obsHost), get(obsPort), get(obsPassword));
    } catch {
      // obsConnect a mis obsStatus=error + obsError. Skip sync captures.
      return;
    }
    // Sync captures + masque caméra (non-fatal, géré en interne).
    await syncCapturesOBS();
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

  // Téléchargement OBS : page officielle obsproject.com (stable — les URLs
  // d'installeur GitHub incluent le n° de version et périment). Ouvert dans
  // le navigateur système via plugin-opener (jamais dans la webview).
  const OBS_DOWNLOAD_URL = "https://obsproject.com/download";
  async function onTelechargerObs() {
    try { await openUrl(OBS_DOWNLOAD_URL); } catch (e) { console.error("openUrl:", e); }
  }

  // Just Chatting : toggle de l'overlay diffusion (caméra + chat seuls).
  // Non-fatal : OBS offline → le mode bascule quand même côté diffusion,
  // les syncs captures se rattrapent au prochain toggle/sync.
  async function onToggleJC() {
    try { await toggleJustChatting(); } catch { /* silencieux */ }
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

  const BORDS: BordRaccourcis[] = ["gauche", "droite", "haut", "bas"];

  // Libellés traduits des bords ($t scopé interdit dans un {#each} → map).
  let nomsBords = $derived.by(() => {
    const tr = $t;
    return Object.fromEntries(
      BORDS.map((b) => [b, tr("toolbar.bord." + b)])
    ) as Record<BordRaccourcis, string>;
  });

  // Options du select moniteur (nom natif ou « Moniteur N » + suffixe
  // « (principal) » — précalculé pour rester réactif au changement de langue).
  let monsOptions = $derived.by(() => {
    const tr = $t;
    return raccMons.map((m) => ({
      ...m,
      label: m.nom ?? tr("toolbar.moniteur", { n: m.index + 1 }),
      suffixe: m.primaire ? " " + tr("toolbar.principal") : "",
    }));
  });

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

  <!-- Section Widgets : la carte bleue (.carte-edition.cible-widgets) est
       portée par la .section elle-même quand elle est ouverte — elle englobe
       en-tête + contenu, au même format que les autres cartes. -->
  <div
    class="section"
    class:carte-edition={open === "widgets"}
    class:cible-widgets={open === "widgets"}
  >
    <button class="header" class:ouvert={open === "widgets"} onclick={() => toggleSection("widgets")}>
      <svg class="ico" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="3" width="7" height="7" rx="1"/><rect x="14" y="3" width="7" height="7" rx="1"/><rect x="3" y="14" width="7" height="7" rx="1"/><rect x="14" y="14" width="7" height="7" rx="1"/></svg>
      <span>{$t("toolbar.widgets")}</span>
      <span class="chevron">›</span>
    </button>
    {#if open === "widgets"}
      <div class="content">
        <button class="action" onclick={createWidget}>{$t("toolbar.creerWidget")}</button>
        <button class="action" onclick={createChatWidget}>{$t("toolbar.creerChat")}</button>
        <button
          class="action"
          onclick={createCameraWidget}
          disabled={hasCameraWidget}
        >{$t("toolbar.creerCamera")}</button>
        <button class="action" onclick={createInputViewerWidget}>{$t("toolbar.creerInputViewer")}</button>
        <button class="action" onclick={createSpeedrunWidget}>{$t("toolbar.creerSpeedrun")}</button>
        <button class="action" onclick={createGoalWidget}>{$t("toolbar.creerGoal")}</button>
        <p class="hint">{$t("toolbar.hintEdition")}</p>
      </div>
    {/if}
  </div>

  <!-- Section Connexions : carte vert foncé portée par la .section quand
       elle est ouverte — même mécanisme que les autres sections-cartes. -->
  <div
    class="section"
    class:carte-edition={open === "connexions"}
    class:cible-connexions={open === "connexions"}
  >
    <button class="header" class:ouvert={open === "connexions"} onclick={() => toggleSection("connexions")}>
      <svg class="ico" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"><path d="M10 13a5 5 0 0 0 7.5.5l3-3a5 5 0 0 0-7-7l-1.7 1.7"/><path d="M14 11a5 5 0 0 0-7.5-.5l-3 3a5 5 0 0 0 7 7l1.7-1.7"/></svg>
      <span>{$t("toolbar.connexions")}</span>
      <span class="chevron">›</span>
    </button>
    {#if open === "connexions"}
      <div class="content">
        <div class="connexion-row">
          <span class="dot plat-twitch" class:on={cx.twitch} class:off={!cx.twitch}></span>
          <span class="plat-nom">Twitch</span>
          <div class="connexion-actions">
            {#if cx.twitch}
              <button class="action small" onclick={onTwitchDisconnect}>{$t("commun.deconnecter")}</button>
            {:else}
              <button class="action small" onclick={onTwitchConnect}>{$t("commun.connecter")}</button>
            {/if}
          </div>
        </div>
        <div class="connexion-row">
          <span class="dot plat-youtube" class:on={cx.youtube} class:off={!cx.youtube}></span>
          <span class="plat-nom">YouTube</span>
          <div class="connexion-actions">
            {#if cx.youtube}
              <button class="action small" onclick={onYoutubeDisconnect}>{$t("commun.deconnecter")}</button>
              <button class="action small" class:on={ytChatOn} class:off={!ytChatOn} onclick={onYoutubeChatToggle}>
                {ytChatOn ? $t("toolbar.chatOn") : $t("toolbar.chatOff")}
              </button>
            {:else}
              <button class="action small" onclick={onYoutubeConnect}>{$t("commun.connecter")}</button>
            {/if}
          </div>
        </div>
        <div class="connexion-row">
          <span class="dot plat-kick" class:on={cx.kick} class:off={!cx.kick}></span>
          <span class="plat-nom">Kick</span>
          <div class="connexion-actions">
            {#if cx.kick}
              <button class="action small" onclick={onKickDisconnect}>{$t("commun.deconnecter")}</button>
            {:else}
              <input class="plat-input" type="text" placeholder="slug" bind:value={kickSlug} spellcheck="false" />
              <button class="action small" onclick={onKickConnect}>{$t("commun.connecter")}</button>
            {/if}
          </div>
        </div>
        <div class="connexion-row">
          <span class="dot plat-tiktok" class:on={cx.tiktok} class:off={!cx.tiktok}></span>
          <span class="plat-nom">TikTok</span>
          <div class="connexion-actions">
            {#if cx.tiktok}
              <button class="action small" onclick={onTiktokDisconnect}>{$t("commun.deconnecter")}</button>
            {:else}
              <input class="plat-input" type="text" placeholder="username" bind:value={tiktokUsername} spellcheck="false" />
              <button class="action small" onclick={onTiktokConnect}>{$t("commun.connecter")}</button>
            {/if}
          </div>
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
      title={$t("toolbar.interactionsTitle")}
    >
      <svg class="ico" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"><path d="M21 11.5a8.38 8.38 0 0 1-8.5 8.5 8.5 8.5 0 0 1-3.8-.9L3 21l1.9-5.7a8.38 8.38 0 0 1-.9-3.8 8.5 8.5 0 0 1 8.5-8.5h.5a8.48 8.48 0 0 1 8 8v.5z"/></svg>
      <span>{$t("toolbar.interactions")}</span>
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
      title={$t("toolbar.ecranAttenteTitle")}
    >
      <svg class="ico" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="9"/><path d="M12 7v5l3 3"/></svg>
      <span>{$t("ecranAttenteModal.titre")}</span>
      <span class="chevron">›</span>
    </button>
  </div>

  <!-- Section Just Chatting : TOGGLE DIRECT (pas un accordéon — le clic
       bascule l'overlay non-destructif de la diffusion : caméra + chat
       seuls, la scène n'est jamais modifiée). La pastille .dot reflète
       l'état runtime — OFF au boot, non persisté. -->
  <div class="section">
    <button
      class="header output-btn"
      class:actif={$jcActif}
      onclick={onToggleJC}
      title={$t("toolbar.justChattingTitle")}
    >
      <svg class="ico" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"><rect x="2" y="5" width="12" height="14" rx="2"/><circle cx="8" cy="11" r="2.5"/><path d="M15 10h6.5v6H19l-2 2v-2h-2"/></svg>
      <span>{$t("toolbar.justChatting")}</span>
      <span class="dot" class:on={$jcActif}></span>
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
      title={$t("toolbar.moderationTitle")}
    >
      <svg class="ico" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"><path d="M12 22s8-4 8-10V5l-8-3-8 3v7c0 6 8 10 8 10z"/></svg>
      <span>{$t("toolbar.moderation")}</span>
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
      title={$t("toolbar.diagnosticTitle")}
    >
      <svg class="ico" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"><path d="M22 12h-4l-3 9L9 3l-3 9H2"/></svg>
      <span>{$t("toolbar.diagnostic")}</span>
      <span class="chevron">›</span>
    </button>
  </div>

  <!-- Section OBS : carte noir/doré portée par la .section quand elle est
       ouverte — même mécanisme que les sections Widgets / Personnalisation /
       Raccourcis. Les couleurs d'état de l'en-tête (rouge/vert) restent
       sémantiques et priment sur la teinte de la carte. -->
  <div
    class="section"
    class:carte-edition={open === "obs"}
    class:cible-obs={open === "obs"}
  >
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
        <input bind:value={$obsHost} placeholder={$t("toolbar.hote")} spellcheck="false" />
        <input bind:value={$obsPort} placeholder={$t("toolbar.port")} spellcheck="false" />
        <input
          bind:value={$obsPassword}
          type="password"
          placeholder={$t("toolbar.motDePasse")}
          spellcheck="false"
        />
        <button
          class="action"
          onclick={onConnect}
          disabled={status === "connecting"}
        >
          {status === "connecting" ? $t("toolbar.connexionEnCours") : $t("commun.connecter")}
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
          {streaming ? $t("toolbar.streamStop") : $t("toolbar.streamStart")}
        </button>
        <button
          class="action output-btn"
          class:actif={recording}
          onclick={onToggleRecord}
          disabled={status !== "connected" || outputBusy}
        >
          <span class="dot" class:on={recording}></span>
          {recording ? $t("commun.arreterEnregistrement") : $t("commun.demarrerEnregistrement")}
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
        <!-- Lien de téléchargement officiel (navigateur système) — discret,
             pied de la carte OBS. -->
        <button class="lien-dl" onclick={onTelechargerObs} title={OBS_DOWNLOAD_URL}>
          {$t("toolbar.obsTelecharger")} ↗
        </button>
      </div>
    {/if}
  </div>

  <!-- Section Personnalisation (cadres SVG) : carte violette portée par la
       .section elle-même quand elle est ouverte — même mécanisme que la
       carte bleue de la section Widgets. -->
  <div
    class="section"
    class:carte-edition={open === "personnalisation"}
    class:cible-perso={open === "personnalisation"}
  >
    <button class="header" class:ouvert={open === "personnalisation"} onclick={() => toggleSection("personnalisation")}>
      <svg class="ico" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"><line x1="21" x2="14" y1="6" y2="6"/><line x1="10" x2="3" y1="6" y2="6"/><line x1="21" x2="12" y1="12" y2="12"/><line x1="8" x2="3" y1="12" y2="12"/><line x1="21" x2="16" y1="18" y2="18"/><line x1="4" x2="3" y1="18" y2="18"/><line x1="14" x2="14" y1="4" y2="8"/><line x1="8" x2="8" y1="10" y2="14"/><line x1="16" x2="16" y1="16" y2="20"/></svg>
      <span>{$t("toolbar.personnalisation")}</span>
      <span class="chevron">›</span>
    </button>
    {#if open === "personnalisation"}
      <div class="content">
        <!-- Galerie unique : UN cadre appliqué aux widgets ET au bord du
             canvas (plus d'onglets depuis 2026-09-26). -->
        <button class="action" onclick={() => cadreModalOpen.set(true)}>
          {$t("cadresModal.titre")}
        </button>
        <button class="action" onclick={onImportFond}>
          {fondImg ? $t("toolbar.fondChanger") : $t("toolbar.fond")}
        </button>
        {#if fondImg}
          <button class="action" onclick={retirerFondSidebar}>
            {$t("toolbar.fondRetirer")}
          </button>
        {/if}
        <p class="hint">
          {#if cadreActif}
            {$t("toolbar.cadreActif")}
          {:else}
            {$t("toolbar.cadreInactif")}
          {/if}
        </p>
      </div>
    {/if}
  </div>

  <!-- Section Raccourcis : fenêtre TOPMOST dockée à un bord d'écran.
       Afficher/masquer + choix du bord (appliqué immédiatement) + moniteur.
       Carte jaune portée par la .section quand elle est ouverte. -->
  <div
    class="section"
    class:carte-edition={open === "raccourcis"}
    class:cible-raccourcis={open === "raccourcis"}
  >
    <button class="header" class:ouvert={open === "raccourcis"} onclick={() => toggleSection("raccourcis")}>
      <svg class="ico" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"><path d="M18 3a3 3 0 0 0-3 3v12a3 3 0 0 0 3 3 3 3 0 0 0 3-3 3 3 0 0 0-3-3H6a3 3 0 0 0-3 3 3 3 0 0 0 3 3 3 3 0 0 0 3-3V6a3 3 0 0 0-3-3 3 3 0 0 0-3 3 3 3 0 0 0 3 3h12a3 3 0 0 0 3-3 3 3 0 0 0-3-3z"/></svg>
      <span>{$t("toolbar.raccourcis")}</span>
      <span class="chevron">›</span>
    </button>
    {#if open === "raccourcis"}
      <div class="content">
        <button class="action" onclick={onToggleBarre}>
          {raccVisible ? $t("toolbar.barreMasquer") : $t("toolbar.barreAfficher")}
        </button>
        <div class="bord-row">
          {#each BORDS as b (b)}
            <button
              class="action small"
              class:actif={raccBord === b}
              onclick={() => onBord(b)}
            >{nomsBords[b]}</button>
          {/each}
        </div>
        <select onchange={onMoniteur}>
          {#each monsOptions as m (m.index)}
            <option value={m.index} selected={m.index === raccMonIdx}>
              {m.label} — {m.largeur}×{m.hauteur}{m.suffixe}
            </option>
          {/each}
        </select>
        <p class="hint">
          {$t("toolbar.raccourcisHint")}
        </p>
      </div>
    {/if}
  </div>
</aside>

<!-- Pied du panneau latéral : section « Ergonomie » (accordéon, même
     mécanique que les sections de l'aside — une seule ouverte à la fois).
     Regroupe le fond du CHROME dashboard (topbar + sidebar — le canvas
     reste opaque par-dessus, jamais touché) et le lanceur du Guide du
     streamer (overlay livre feuilletable). Sibling de l'aside dans
     .sidebar-col → fixe en bas, HORS de la zone scrollable, toujours
     visible. Carte ardoise portée par la .section quand elle est ouverte ;
     la pastille .dot marque qu'un fond dashboard est posé. -->
<div
  class="section dash-fond"
  class:carte-edition={open === "ergonomie"}
  class:cible-ergo={open === "ergonomie"}
>
  <button class="header" class:ouvert={open === "ergonomie"} onclick={() => toggleSection("ergonomie")}>
    <svg class="ico" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"><path d="M14.7 6.3a1 1 0 0 0 0 1.4l1.6 1.6a1 1 0 0 0 1.4 0l3.77-3.77a6 6 0 0 1-7.94 7.94l-6.91 6.91a2.12 2.12 0 0 1-3-3l6.91-6.91a6 6 0 0 1 7.94-7.94l-3.76 3.76z"/></svg>
    <span>{$t("toolbar.ergonomie")}</span>
    <span class="dot" class:on={fondDashImg}></span>
    <span class="chevron">›</span>
  </button>
  {#if open === "ergonomie"}
    <div class="content">
      <button class="action" onclick={onImportFondDash} title={$t("toolbar.fondDashTitle")}>
        {fondDashImg ? $t("toolbar.fondChanger") : $t("toolbar.fondDash")}
      </button>
      {#if fondDashImg}
        <button class="action small" onclick={retirerFondDash}>{$t("toolbar.fondRetirer")}</button>
      {/if}
      <button class="action" onclick={() => guideLivreOpen.set(true)} title={$t("toolbar.guideTitle")}>
        {$t("toolbar.guide")}
      </button>
    </div>
  {/if}
</div>

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
    /* Jour entre le contenu et la piste de scroll. */
    padding-right: 6px;
    scrollbar-width: thin;
    scrollbar-color: var(--bordure-ui-active) transparent;
  }
  /* Scrollbar du panneau : plus fine que la globale (6px), piste
     transparente, pouce aux couleurs de bordure déjà en place. */
  .sidebar::-webkit-scrollbar {
    width: 6px;
  }
  .sidebar::-webkit-scrollbar-track {
    background: transparent;
  }
  .sidebar::-webkit-scrollbar-thumb {
    background: var(--bordure-ui-active);
    border-radius: calc(var(--rayon-petit) + 2px);
  }
  .sidebar::-webkit-scrollbar-thumb:hover {
    background: var(--accent-ui);
  }
  /* Pied du panneau : jamais écrasé quand la liste déborde — l'aside
     (flex:1 + min-height:0) absorbe toute la compression. */
  .dash-fond {
    flex-shrink: 0;
  }
  /* Encadré de section : bordure teintée par l'accent utilisateur
     (--bordure-ui = couleur du cadre), plus marquée quand la section est
     ouverte — le contenu déplié reste dans le même cadre. */
  .section {
    /* relative : ancre du filet ::before (bordure haute dégradée). */
    position: relative;
    display: flex;
    flex-direction: column;
    border: 1px solid var(--bordure-ui);
    border-radius: var(--rayon-petit);
    transition: border-color 0.15s ease;
  }
  /* Filet signature : le dégradé du cadre utilisateur remplace la bordure
     haute de la section — même motif que .ctx-menu::before (app.css).
     top:-1px + hauteur 1px = recouvre la bordure 1px. Le masque horizontal
     fond les 2 bouts (border-radius plafonné à 0.5px sur 1px de haut =
     extrémités carrées sinon).
     Survol = inversion fluide : dégradé symétrique c1→c2→c1 dimensionné à
     200 % — animer background-position fait glisser les couleurs et
     l'état final est le miroir du repos (un gradient n'est pas
     transitionnable directement, sa position oui). */
  .section::before {
    content: "";
    position: absolute;
    top: -1px;
    left: -1px;
    right: -1px;
    height: 1px;
    background: linear-gradient(
      90deg,
      var(--cadre-couleur, var(--accent-orange)) 0%,
      var(--cadre-couleur-fin, var(--accent-violet)) 50%,
      var(--cadre-couleur, var(--accent-orange)) 100%
    );
    background-size: 200% 100%;
    background-position: 0% 0;
    transition: background-position 0.45s ease;
    -webkit-mask-image: linear-gradient(90deg, transparent, #000 12%, #000 88%, transparent);
    mask-image: linear-gradient(90deg, transparent, #000 12%, #000 88%, transparent);
    border-radius: inherit;
    pointer-events: none;
    z-index: 1;
  }
  .section:hover::before {
    background-position: 100% 0;
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
    transition: background 0.15s ease, color 0.15s ease, border-color 0.15s ease,
      box-shadow 0.15s ease, text-shadow 0.15s ease;
  }
  /* Hover : fond transparent, pas de bordure propre (elle s'ajoutait à
     celle de la .section = double cadre 2px). Le repère de survol vit dans
     le texte : label/icône/chevron s'illuminent en accent + glow (currentColor
     → le glow suit les états sémantiques rouge/vert d'OBS) + liseré accent à
     gauche (même motif que .etat-erreur). Le filet dégradé de la section
     s'inverse en plus (.section:hover::before). */
  .header:hover {
    background: transparent;
    border-color: transparent;
    backdrop-filter: none;
    color: var(--accent-ui);
    text-shadow: 0 0 10px color-mix(in srgb, currentColor 60%, transparent);
    box-shadow: inset 3px 0 0 var(--accent-ui);
  }
  .header:hover .ico,
  .header:hover .chevron {
    opacity: 1;
  }
  /* Chrome global des boutons neutralisé : ni sweep lumineux ::before
     (animation btn-shine) ni reflet miroir ::after — l'en-tête reste
     une ligne de texte transparente dans tous les états. */
  .header::before,
  .header::after {
    content: none;
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
    background: transparent;
  }
  /* État OBS sur l'en-tête : connecté = texte vert sémantique ; non
     connecté = texte rouge + liseré d'alerte à gauche de la ligne.
     Aucun effet au hover (la pilule .ouvert garde son aspect de repos). */
  .header.etat-erreur {
    color: var(--message-error-color);
    box-shadow: inset 3px 0 0 var(--message-error-color);
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
  /* Toggle Just Chatting : la pastille d'état prend la place du chevron
     (poussée à droite dans l'en-tête). */
  .header .dot {
    margin-left: auto;
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
  /* ===== Sections ouvertes portées en carte (.carte-edition, app.css) =====
     La .section elle-même porte .carte-edition + .cible-* quand elle est
     ouverte : la carte englobe en-tête + contenu, au format des cartes
     CarteEdition. Les règles ci-dessous sont communes à toutes les
     sections-cartes ; chaque variante .cible-* n'assigne que sa couleur. */
  /* La bordure .section (1px solid --bordure-ui) et sa variante ouverte
     .section:has(.header.ouvert) (--bordure-ui-active) écrasent le shorthand
     de .carte-edition à spécificité égale/supérieure : sans cette surcharge
     à (0,4,0), la carte perdrait ses pointillés 2px et son rayon. */
  .section:has(.header.ouvert).carte-edition {
    border: 2px dashed var(--accent-carte);
    border-radius: var(--rayon);
  }
  .section.carte-edition {
    gap: 0.35rem;
    padding: 0.4rem;
  }
  /* En-tête dans la carte ouverte : bordure de la couleur de la carte au
     lieu du neutre --bordure-ui-active (.header.ouvert). Aucune bordure
     ajoutée au hover (règle "hover = rien" appliquée aux en-têtes). */
  .section.carte-edition .header.ouvert {
    border-color: var(--accent-carte);
  }
  /* Boutons dans la carte : bordure de la couleur de la carte (même token),
     éclaircie au hover via color-mix. Hors carte, la bordure neutre
     .action est inchangée. */
  .section.carte-edition .action {
    border-color: var(--accent-carte);
  }
  .section.carte-edition .action:hover:not(:disabled) {
    border-color: color-mix(in srgb, var(--accent-carte) 70%, var(--texte));
  }
  .section.carte-edition .action:disabled,
  .section.carte-edition .action:disabled:hover {
    border-color: var(--accent-carte);
  }
  /* Variantes — même mécanisme que les .cible-* de CarteEdition : seule la
     couleur change. Le fond est monté à 32 % (au lieu des 22 % partagés)
     pour que la teinte se lise. */
  .cible-widgets {
    --accent-carte: var(--carte-bleue);
    --accent-carte-glow: color-mix(in srgb, var(--carte-bleue) 45%, transparent);
    background: color-mix(in srgb, var(--accent-carte) 32%, var(--fond-panneau));
  }
  .cible-perso {
    --accent-carte: var(--carte-violette);
    --accent-carte-glow: color-mix(in srgb, var(--carte-violette) 45%, transparent);
    background: color-mix(in srgb, var(--accent-carte) 32%, var(--fond-panneau));
  }
  .cible-raccourcis {
    --accent-carte: var(--carte-jaune);
    --accent-carte-glow: color-mix(in srgb, var(--carte-jaune) 45%, transparent);
    background: color-mix(in srgb, var(--accent-carte) 32%, var(--fond-panneau));
  }
  /* Noir/doré : ambré très sombre — les pointillés restent lisibles sans
     virer à l'orange vif. */
  .cible-obs {
    --accent-carte: var(--carte-dore);
    --accent-carte-glow: color-mix(in srgb, var(--carte-dore) 55%, transparent);
    background: color-mix(in srgb, var(--accent-carte) 32%, var(--fond-panneau));
  }
  .cible-connexions {
    --accent-carte: var(--carte-verte);
    --accent-carte-glow: color-mix(in srgb, var(--carte-verte) 45%, transparent);
    background: color-mix(in srgb, var(--accent-carte) 32%, var(--fond-panneau));
  }
  /* Ardoise : teinte propre à la section Ergonomie (pied fixe) — la
     palette --carte-* standard étant épuisée, un 6ᵉ token lui est dédié. */
  .cible-ergo {
    --accent-carte: var(--carte-ardoise);
    --accent-carte-glow: color-mix(in srgb, var(--carte-ardoise) 45%, transparent);
    background: color-mix(in srgb, var(--accent-carte) 32%, var(--fond-panneau));
  }
  /* En-tête Ergonomie : la pastille .dot (fond dashboard posé) garde son
     margin-left auto qui la pousse à droite ; le chevron la suit collé —
     sans cette surcharge, son propre margin-left auto créerait un trou
     entre la pastille et lui. */
  .dash-fond .chevron {
    margin-left: 0.3rem;
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
  /* Lien de téléchargement OBS : texte discret souligné, centré en pied de
     carte — pas une .action (c'est un lien externe, pas une action locale). */
  .lien-dl {
    align-self: center;
    background: none;
    border: none;
    padding: 0.1rem 0.2rem;
    font-size: 0.78rem;
    color: var(--texte);
    opacity: 0.6;
    cursor: pointer;
    text-decoration: underline;
    text-underline-offset: 3px;
  }
  .lien-dl:hover {
    opacity: 1;
  }
  /* Ligne plateforme : wrap autorisé — dans la carte (padding réduit la
     largeur dispo), le groupe d'actions bascule à la ligne au lieu de
     déborder du bord droit. */
  .connexion-row {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 0.4rem;
    padding: 0.2rem 0;
  }
  /* Séparateur entre plateformes : liseré discret teinté de la couleur de
     la carte (--accent-carte, fallback neutre --bordure-ui) + un peu d'air
     au-dessus. Délimite visuellement chaque plateforme dans la carte. */
  .connexion-row + .connexion-row {
    border-top: 1px solid color-mix(in srgb, var(--accent-carte, var(--bordure-ui)) 35%, transparent);
    padding-top: 0.45rem;
    margin-top: 0.1rem;
  }
  /* Groupe d'actions de la ligne (boutons ± input slug) : collé à droite
     (margin-left auto), jamais coupé en deux (nowrap interne). Quand il
     wrappe, il descend EN BLOC et reste aligné à droite — pas de bouton
     orphelin au milieu de la carte. */
  .connexion-actions {
    display: flex;
    flex-wrap: nowrap;
    align-items: center;
    gap: 0.4rem;
    margin-left: auto;
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
  /* Input slug/username : compressible dans la carte (la ligne wrappe
     quand même si nécessaire — le min évite un champ trop étriqué). */
  .plat-input {
    width: 70px;
    min-width: 44px;
    flex-shrink: 1;
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
