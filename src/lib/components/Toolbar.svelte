<script lang="ts">
  import { createWidget, createChatWidget, createCameraWidget, createInputViewerWidget, createSpeedrunWidget, createGoalWidget, createMusiqueWidget, sceneStore, syncCapturesOBS } from "../stores/scene";
  import { obsConnect, obsStatus, obsError, obsHost, obsPort, obsPassword, obsStreaming, obsRecording, obsStreamBusy, obsRecordBusy, obsMoniteurs, chargerMoniteursObs, ouvrirProjecteurPreview, rafraichirOutputs } from "../stores/obs";
  import { moteurEtat, moteurErreur, chargerEtatMoteur, basculerMoteur, envoyerConnexionObs, sortieNatif, sortieNatifBusy, sortieNatifFichier, sortieCfg, rafraichirSortieNatif, demarrerEnregistrementNatif, arreterEnregistrementNatif, chargerSortieCfg } from "../stores/moteur";
  import { openSection, toggleSection, carteEditionCible, cadreModalOpen, interactionModalOpen, moderationModalOpen, consoModalOpen, ecranAttenteModalOpen, guideLivreOpen, modulesModalOpen, planningModalOpen, statistiquesModalOpen, obsWebSocketModalOpen, confirmObsOutputOpen, sortieNatifModalOpen } from "../stores/ui";
  import { jcActif, toggleJustChatting } from "../stores/justChatting";
  import { connexions, connecterTwitch, deconnecterTwitch, twitchErreur } from "../stores/chat";
  import { connecterKick, deconnecterKick, kickErreur as kickErreurStore, lireSlugSauve } from "../stores/kick";
  import { connecterYoutube, deconnecterYoutube, demarrerChatYoutube, arreterChatYoutube } from "../stores/youtube";
  import { connecterTiktok, deconnecterTiktok, lireUsernameSauve } from "../stores/tiktok";
  import { youtubeErreur as youtubeErreurStore, tiktokErreur as tiktokErreurStore, youtubeChatActif as youtubeChatActifStore } from "../stores/chat";
  import { raccourcisVisible, raccourcisBord, raccourcisMonitorIndex, raccourcisMoniteurs, appliquerRaccourcis, masquerRaccourcis } from "../stores/raccourcis";
  import { fenetreMonitorIndex, fenetreMode, appliquerFenetre } from "../stores/fenetre";
  import { fondSidebar, importerFondSidebar, retirerFondSidebar } from "../stores/fondSidebar";
  import { fondDash, importerFondDash, retirerFondDash } from "../stores/fondDash";
  import { tauri, type BordRaccourcis, type ModeFenetre } from "../tauri";
  import { t } from "../stores/langue";
  import { get } from "svelte/store";
  import { onMount } from "svelte";
  import { listen } from "@tauri-apps/api/event";
  import { appDataDir } from "@tauri-apps/api/path";
  import { save } from "@tauri-apps/plugin-dialog";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import icoWidgets from "../../assets/ico/Widget.png";
  import icoModules from "../../assets/ico/Modules.png";
  import icoPerso from "../../assets/ico/Personnalistation.png";
  import icoObs from "../../assets/ico/Obs.png";
  import icoConnexions from "../../assets/ico/Connexion.png";
  import icoPreview from "../../assets/ico/Previsualiqation.png";
  import icoAttente from "../../assets/ico/Ecran d'attente.png";
  import icoJc from "../../assets/ico/Just Chatting.png";
  import icoInteractions from "../../assets/ico/Interaction live.png";
  import icoErgo from "../../assets/ico/Ergonomie.png";
  import icoDiagnostic from "../../assets/ico/Diagnostic.png";
  import icoGuide from "../../assets/ico/Guide du streamer.png";

  let status = $derived($obsStatus);
  let error = $derived($obsError);
  let streaming = $derived($obsStreaming);
  let recording = $derived($obsRecording);
  // Mode natif : le bouton « Enregistrer » route vers la sortie native.
  // La pastille et le libellé suivent le moteur réellement actif.
  let natifMode = $derived($moteurEtat.mode === "natif");
  let enregistre = $derived(natifMode ? $sortieNatif.actif : recording);
  let open = $derived($openSection);
  let cx = $derived($connexions);
  let twitchErr = $derived($twitchErreur);
  let kickErr = $derived($kickErreurStore);
  let ytErr = $derived($youtubeErreurStore);
  let ytChatOn = $derived($youtubeChatActifStore);
  let ttErr = $derived($tiktokErreurStore);

  // Singleton caméra : désactive le bouton de création si un widget caméra existe.
  let hasCameraWidget = $derived($sceneStore.widgets.some((w) => w.type === "camera"));

  // ===== Cadre SVG unifié (section Cadre & fond) =====
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
      await obsConnect(get(obsHost), get(obsPort), get(obsPassword), "bouton");
    } catch {
      // obsConnect a mis obsStatus=error + obsError. Skip sync captures.
      return;
    }
    // Sync captures + masque caméra (non-fatal, géré en interne).
    await syncCapturesOBS();
  }

  // Sorties OBS : confirmation avant set (démarrer ET arrêter) — la modale
  // ConfirmObsOutputModal relit l'état réel puis appelle setStreamObs/
  // setRecordObs. obsStreamBusy/obsRecordBusy (stores/obs) grisent pendant
  // le RPC.
  function onToggleStream() {
    confirmObsOutputOpen.set("stream");
  }
  /// Enregistrement : OBS ou natif selon le mode actif. En natif, l'utilisateur
  /// choisit le fichier (dialogue) puis le backend démarre hors du thread UI.
  async function onToggleRecord() {
    if (natifMode) {
      if ($sortieNatif.actif) {
        await arreterEnregistrementNatif();
        return;
      }
      const chemin = await choisirFichierSortie();
      if (chemin) await demarrerEnregistrementNatif(chemin);
      return;
    }
    confirmObsOutputOpen.set("record");
  }

  /// Horodatage local `AAAA-MM-JJ_HH-MM-SS` pour le nom par défaut.
  function horodatage(): string {
    const d = new Date();
    const p = (n: number) => String(n).padStart(2, "0");
    return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())}_${p(d.getHours())}-${p(d.getMinutes())}-${p(d.getSeconds())}`;
  }

  /// Dialogue « Enregistrer sous ». Défaut dans `enregistrements/` du dossier
  /// de données de l'app (tout local) ; `null` si l'utilisateur annule.
  /// Extension et filtre suivent le conteneur persisté (bloc `sortie` de
  /// moteur.json) — relecture non fatale, les défauts restent mp4.
  async function choisirFichierSortie(): Promise<string | null> {
    await chargerSortieCfg();
    const conteneur = $sortieCfg.conteneur === "mkv" ? "mkv" : "mp4";
    const nom = `StreamOS-${horodatage()}.${conteneur}`;
    let defaut = nom;
    try {
      defaut = `${await appDataDir()}/enregistrements/${nom}`;
    } catch {
      // Repli : le dialogue s'ouvre sur son dernier dossier connu.
    }
    try {
      return await save({
        title: get(t)("moteur.enregistrerTitre"),
        defaultPath: defaut,
        filters: [{ name: conteneur === "mkv" ? "Matroska" : "MP4", extensions: [conteneur] }],
      });
    } catch (e) {
      moteurErreur.set(String(e));
      return null;
    }
  }

  // Resync des libellés stream/record à l'ouverture de la section OBS :
  // l'utilisateur a pu agir directement dans OBS depuis le dernier affichage.
  // Ponctuel (ouverture de section), pas un timer — pas de polling.
  $effect(() => {
    if (open === "obs" && status === "connected") void rafraichirOutputs();
  });

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

  // ===== Fenêtre principale : écran + mode de démarrage (Ergonomie) =====
  // La liste des moniteurs est partagée avec le bloc barre de raccourcis
  // (monsOptions / raccMons ci-dessus) — même section Ergonomie.
  let fenMonIdx = $derived($fenetreMonitorIndex);
  let fenMode = $derived($fenetreMode);

  const MODES: ModeFenetre[] = ["fenetre", "maximise", "fullscreen"];

  // Libellés traduits des modes ($t scopé interdit dans un {#each} → map).
  let nomsModes = $derived.by(() => {
    const tr = $t;
    return Object.fromEntries(
      MODES.map((m) => [m, tr("toolbar.mode." + m)])
    ) as Record<ModeFenetre, string>;
  });

  // Chaque changement applique immédiatement : la fenêtre saute sur l'écran/
  // mode choisi ET la préférence est persistée pour le prochain démarrage.
  async function onFenMode(m: ModeFenetre) {
    await appliquerFenetre(fenMonIdx, m);
  }
  async function onFenMoniteur(e: Event) {
    const idx = parseInt((e.target as HTMLSelectElement).value, 10);
    if (!isNaN(idx)) await appliquerFenetre(idx, fenMode);
  }

  // ===== Prévisualisation (miniature live :4321 + projecteur OBS) =====
  // Section ACCORDÉON standard : pliée = tout démonté, l'iframe est
  // détruite (zéro coût — c'est le lazy-loading) ; ouverte = miniature +
  // contrôles. La miniature = iframe de la page diffusion en mode ?preview
  // (muette, vidéos persistantes figées — zéro Rust : les frames WS sont
  // déjà broadcast à tout client). Le clic sur la miniature ouvre le
  // projecteur OBS « prévisualisation » sur l'écran choisi
  // (OpenVideoMixProjector).
  // ⚠️ La miniature ne montre que la couche overlay : caméra et captures
  // OBS sont des trous dans la page — le vrai rendu = le projecteur OBS.
  function lirePrevNum(cle: string, defaut: number): number {
    try { const v = parseInt(localStorage.getItem(cle) ?? "", 10); return isNaN(v) ? defaut : v; }
    catch { return defaut; }
  }
  function persisterPrev(cle: string, v: string): void {
    try { localStorage.setItem(cle, v); } catch { /* ignore */ }
  }

  let previewMonIdx = $state(lirePrevNum("previewMonIdx", -1));
  let previewMoniteurs = $derived($obsMoniteurs);
  let previewBusy = $state(false);
  // Feedback d'ouverture du projecteur : l'erreur obs-websocket serait sinon
  // invisible (obsError ne s'affiche que dans la section OBS — accordéon
  // fermé la plupart du temps) → clic qui semble « ne rien faire ».
  let previewMsg = $state<string | null>(null);
  let previewMsgOk = $state(false);
  let previewBoxW = $state(0);
  // Dims canvas de la scène → iframe rendue à taille réelle puis réduite par
  // transform (même rendu que la source OBS, juste dézoomé).
  let previewCanvasW = $derived($sceneStore.canvasW ?? 1920);
  let previewCanvasH = $derived($sceneStore.canvasH ?? 1080);
  let previewScale = $derived(previewBoxW > 0 ? previewBoxW / previewCanvasW : 0);

  // Moniteurs OBS relus à l'ouverture de la section (non-fatal : [] si OBS
  // absent — le select garde « Fenêtré »). Mode natif : le projecteur est
  // une fenêtre détachée — pas de sélecteur d'écran OBS.
  $effect(() => {
    if (open === "preview" && !natifMode) void chargerMoniteursObs();
  });
  function onPreviewMoniteur(e: Event) {
    const idx = parseInt((e.target as HTMLSelectElement).value, 10);
    previewMonIdx = isNaN(idx) ? -1 : idx;
    persisterPrev("previewMonIdx", String(previewMonIdx));
  }
  async function onOuvrirProjecteur() {
    // Même bouton, deux moteurs : OBS → projecteur OBS (commande one-shot
    // qui se connecte elle-même — pas de gate sur le statut) ; natif →
    // fenêtre d'aperçu du moteur Rust.
    if (previewBusy) return;
    previewBusy = true;
    previewMsg = null;
    try {
      if (natifMode) {
        await tauri.apercuFenetreToggle();
        previewMsg = $t("toolbar.previewOuvertNatif");
      } else {
        await ouvrirProjecteurPreview(previewMonIdx);
        previewMsg = $t("toolbar.previewOuvert");
      }
      previewMsgOk = true;
    } catch (e) {
      previewMsg = natifMode ? String(e) : (get(obsError) ?? String(e));
      previewMsgOk = false;
    }
    previewBusy = false;
  }

  onMount(async () => {
    await listen("popout-closed", () => {});
    if (!kickSlug) kickSlug = (await lireSlugSauve()) ?? "";
    if (!tiktokUsername) tiktokUsername = (await lireUsernameSauve()) ?? "";
    await chargerEtatMoteur();
    await envoyerConnexionObs();
    // Reprise d'affichage : si une sortie native tourne déjà (jamais au boot,
    // mais l'UI ne doit pas mentir), le suivi 1 s démarre.
    await rafraichirSortieNatif();
    // Réglages de sortie natifs (moteur.json) : le résumé du bloc moteur les
    // affiche — charge en avance, non-fatal en interne.
    await chargerSortieCfg();
  });

  /// Id codec persisté → nom lisible (contrat `Codec::nom` côté Rust).
  function codecNom(id: string): string {
    return { h264: "H.264", hevc: "HEVC", av1: "AV1" }[id] ?? id;
  }

  /// Bascule OBS ↔ natif. `liveActif` empêche le backend de couper un live
  /// implicitement (refus explicite affiché dans le bloc moteur).
  async function onBasculerMoteur(cible: "obs" | "natif") {
    await basculerMoteur(cible, streaming || recording);
  }

  // ===== Fermeture auto de la carte d'édition au scroll de la sidebar =====
  // La carte ouverte (CarteEdition, au-dessus) compresse l'accordéon : si
  // l'utilisateur scrolle la section ainsi rétrécie (wheel ou drag de la
  // scrollbar fine), c'est qu'il veut atteindre les sections — la carte se
  // referme d'elle-même et lui rend l'espace. Le scroll INTERNE de la carte
  // (.carte-corps, élément sœur de l'aside) ne déclenche RIEN : le scroll ne
  // bulle pas, et le wheel sur la carte n'atteint jamais l'aside.
  let carteOuverteA = 0;
  $effect(() => {
    if ($carteEditionCible !== null) carteOuverteA = performance.now();
  });
  // Garde 350 ms anti « clamp » : ouvrir la carte ferme l'accordéon → le
  // contenu de la sidebar rétrécit → le navigateur RECALE scrollTop (scroll
  // programmatique, pas un geste) — sans garde, la carte se refermerait
  // aussitôt ouverte dès que la sidebar était scrollée plus bas que le
  // nouveau contenu.
  function onSidebarScroll(): void {
    if (performance.now() - carteOuverteA < 350) return;
    if (get(carteEditionCible) !== null) carteEditionCible.set(null);
  }
  // Wheel = intention explicite, même sans scrollbar visible (contenu
  // compressé qui tient). Même garde : une molette en inertie residuelle ne
  // doit pas refermer une carte fraîchement ouverte.
  function onSidebarWheel(): void {
    if (performance.now() - carteOuverteA < 350) return;
    if (get(carteEditionCible) !== null) carteEditionCible.set(null);
  }

</script>

<aside class="sidebar" onscroll={onSidebarScroll} onwheel={onSidebarWheel}>
  <!-- L'édition du fond de l'application et des widgets se fait maintenant
       via la carte d'édition contextuelle (CarteEdition.svelte) en bas du
       panneau latéral — plus de section d'accordéon dédiée. -->
  <!-- Fermeture auto : scroller/wheeler cette zone referme la carte
       d'édition ouverte au-dessus (elle comprime l'accordéon) —
       voir onSidebarScroll/onSidebarWheel dans le script. -->

  <!-- ============ GROUPE : MA SCÈNE ============ -->
  <div class="groupe-label">{$t("toolbar.groupeScene")}</div>

  <!-- Section Widgets : la carte bleue (.carte-edition.cible-widgets) est
       portée par la .section elle-même quand elle est ouverte — elle englobe
       en-tête + contenu, au même format que les autres cartes. -->
  <div
    class="section sec-widgets"
    class:carte-edition={open === "widgets"}
    class:cible-widgets={open === "widgets"}
  >
    <button class="header" class:ouvert={open === "widgets"} onclick={() => toggleSection("widgets")}>
      <img class="ico" src={icoWidgets} alt="" />
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
        <button class="action" onclick={createMusiqueWidget}>{$t("toolbar.creerMusique")}</button>
        <p class="hint">{$t("toolbar.hintEdition")}</p>
      </div>
    {/if}
  </div>

  <!-- Section Modules HUD : LANCEUR DIRECT (pas un accordéon — le clic ouvre
       la modale master-detail des modules HUD de la scène : horloge,
       musique en cours, objectifs du stream). Le positionnement se fait
       aussi au drag/resize direct sur le canvas. -->
  <div class="section sec-modules">
    <button
      class="header"
      onclick={() => modulesModalOpen.set("horloge")}
      title={$t("toolbar.modulesTitle")}
    >
      <img class="ico" src={icoModules} alt="" />
      <span>{$t("toolbar.modules")}</span>
      <span class="chevron">›</span>
    </button>
  </div>

  <!-- Section Cadre & fond (ex-Personnalisation : cadres SVG + image de
       fond du panneau) : carte violette portée par la .section elle-même
       quand elle est ouverte — même mécanisme que la carte bleue de la
       section Widgets. -->
  <div
    class="section sec-perso"
    class:carte-edition={open === "personnalisation"}
    class:cible-perso={open === "personnalisation"}
  >
    <button class="header" class:ouvert={open === "personnalisation"} onclick={() => toggleSection("personnalisation")}>
      <img class="ico" src={icoPerso} alt="" />
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

  <!-- ============ GROUPE : CONNEXIONS ============ -->
  <div class="groupe-label">{$t("toolbar.groupeConnexions")}</div>

  <!-- Section OBS : carte noir/doré portée par la .section quand elle est
       ouverte — même mécanisme que les sections Widgets / Cadre & fond /
       Réglages. Les couleurs d'état de l'en-tête (rouge/vert) restent
       sémantiques et priment sur la teinte de la carte. -->
  <div
    class="section sec-obs"
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
      <img class="ico" src={icoObs} alt="" />
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
          disabled={status !== "connected" || $obsStreamBusy}
        >
          <span class="dot" class:on={streaming}></span>
          {streaming ? $t("toolbar.streamStop") : $t("toolbar.streamStart")}
        </button>
        <button
          class="action output-btn"
          class:actif={enregistre}
          onclick={onToggleRecord}
          disabled={(!natifMode && status !== "connected") || $obsRecordBusy || $sortieNatifBusy}
        >
          <span class="dot" class:on={enregistre}></span>
          {enregistre ? $t("commun.arreterEnregistrement") : $t("commun.demarrerEnregistrement")}
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
        <!-- Moteur de diffusion (lot 1.5) : OBS ou natif. Le backend refuse
             la bascule si un live OBS ou une sortie native tourne (jamais de
             coupure implicite). -->
        <div class="moteur-bloc">
          <span class="moteur-titre">{$t("moteur.titre")}</span>
          <div class="moteur-seg">
            <button
              class="action moteur-btn"
              class:actif={$moteurEtat.mode === "obs"}
              disabled={$moteurEtat.mode === "obs"}
              onclick={() => onBasculerMoteur("obs")}
            >
              {$t("moteur.obs")}
            </button>
            <button
              class="action moteur-btn"
              class:actif={$moteurEtat.mode === "natif"}
              disabled={$moteurEtat.mode === "natif" || !$moteurEtat.natif_disponible}
              title={!$moteurEtat.natif_disponible ? $t("moteur.natifIndisponible") : ""}
              onclick={() => onBasculerMoteur("natif")}
            >
              {$t("moteur.natif")}
            </button>
          </div>
          {#if $moteurEtat.capacites.length > 0}
            <p class="hint">{$t("moteur.capacites", { n: $moteurEtat.capacites.length })}</p>
            <ul class="moteur-caps">
              {#each $moteurEtat.capacites as cap}
                <li>{$t("moteur.cap." + cap)}</li>
              {/each}
            </ul>
          {/if}
          {#if natifMode}
            <!-- Résumé des réglages de sortie persistés (moteur.json) —
                 réglables dans la modale « Sortie ». -->
            <button class="action" onclick={() => sortieNatifModalOpen.set(true)}>
              {$t("sortieModal.ouvrir")}
            </button>
            <p class="hint">
              {$t("moteur.sortieParametres", { fps: $sortieCfg.fps, codec: codecNom($sortieCfg.codec), debit: $sortieCfg.debit_kbps })}
            </p>
            {#if $sortieNatif.actif}
              <p class="hint">
                {$t("moteur.sortieEtat", { kbps: $sortieNatif.kbps, drops: $sortieNatif.frames_perdues })}
              </p>
              {#if $sortieNatif.encodeur}
                <p class="hint">{$t("moteur.sortieEncodeur", { encodeur: $sortieNatif.encodeur })}</p>
              {/if}
              {#if $sortieNatif.chemin}
                <p class="hint moteur-chemin">{$t("moteur.sortieFichier", { chemin: $sortieNatif.chemin })}</p>
              {/if}
            {:else if $sortieNatifFichier && !$sortieNatif.erreur}
              <p class="hint moteur-chemin">{$t("moteur.sortieFichier", { chemin: $sortieNatifFichier })}</p>
            {/if}
            {#if $sortieNatif.degradation}
              <p class="hint moteur-erreur">{$sortieNatif.degradation}</p>
            {/if}
            {#if $sortieNatif.erreur}
              <p class="hint moteur-erreur">{$sortieNatif.erreur}</p>
            {/if}
          {/if}
          {#if $moteurErreur}
            <p class="hint moteur-erreur">{$moteurErreur}</p>
          {/if}
        </div>
        <!-- Fenêtre « Connexion OBS » (WebSocket) : inventaire de ce qui
             transite par le canal + options — pour les curieux, l'utilisateur
             non technique n'y va jamais. -->
        <button class="action" onclick={() => obsWebSocketModalOpen.set(true)}>
          {$t("obsWs.ouvrir")}
        </button>
        <!-- Lien de téléchargement officiel (navigateur système) — discret,
             pied de la carte OBS. -->
        <button class="lien-dl" onclick={onTelechargerObs} title={OBS_DOWNLOAD_URL}>
          {$t("toolbar.obsTelecharger")} ↗
        </button>
      </div>
    {/if}
  </div>

  <!-- Section Plateformes : carte vert foncé portée par la .section quand
       elle est ouverte — même mécanisme que les autres sections-cartes. -->
  <div
    class="section sec-connexions"
    class:carte-edition={open === "connexions"}
    class:cible-connexions={open === "connexions"}
  >
    <button class="header" class:ouvert={open === "connexions"} onclick={() => toggleSection("connexions")}>
      <img class="ico" src={icoConnexions} alt="" />
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

  <!-- ============ GROUPE : EN DIRECT ============ -->
  <div class="groupe-label">{$t("toolbar.groupeDirect")}</div>

  <!-- Section Prévisualisation : accordéon standard — pliée = tout démonté
       (iframe détruite → zéro coût), ouverte = miniature live + projecteur.
       La carte jaune (.carte-edition.cible-preview) est portée par la
       .section quand elle est ouverte, comme les autres sections. -->
  <div
    class="section sec-preview"
    class:carte-edition={open === "preview"}
    class:cible-preview={open === "preview"}
  >
    <button
      class="header"
      class:ouvert={open === "preview"}
      onclick={() => toggleSection("preview")}
      title={$t("toolbar.previewTitle")}
    >
      <img class="ico" src={icoPreview} alt="" />
      <span>{$t("toolbar.preview")}</span>
      <span class="chevron">›</span>
    </button>
    {#if open === "preview"}
      <div class="content">
        <!-- Miniature live : iframe de la page diffusion en mode ?preview
             (son coupé, vidéos figées). pointer-events:none + le clic est
             capté par le bouton → ouvre le projecteur OBS. -->
        <button
          class="preview-thumb"
          bind:clientWidth={previewBoxW}
          onclick={onOuvrirProjecteur}
          disabled={previewBusy}
          title={$t("toolbar.previewOuvrir")}
          style="aspect-ratio: {previewCanvasW} / {previewCanvasH};"
        >
          <iframe
            src="http://127.0.0.1:4321/?preview"
            title={$t("toolbar.preview")}
            tabindex="-1"
            style="width: {previewCanvasW}px; height: {previewCanvasH}px; transform: scale({previewScale});"
          ></iframe>
        </button>
        {#if !natifMode}
          <select onchange={onPreviewMoniteur} disabled={previewBusy}>
            <option value="-1" selected={previewMonIdx === -1}>{$t("toolbar.previewFenetre")}</option>
            {#each previewMoniteurs as m (m.monitorIndex)}
              <option value={m.monitorIndex} selected={previewMonIdx === m.monitorIndex}>
                {m.monitorName || $t("toolbar.previewMoniteur", { n: m.monitorIndex + 1 })} — {m.monitorWidth}×{m.monitorHeight}
              </option>
            {/each}
          </select>
        {/if}
        <button class="action" onclick={onOuvrirProjecteur} disabled={previewBusy}>
          {previewBusy ? $t("toolbar.connexionEnCours") : $t("toolbar.previewOuvrir")}
        </button>
        {#if previewMsg}
          <p class="hint" class:msg-ok={previewMsgOk} class:msg-err={!previewMsgOk}>{previewMsg}</p>
        {/if}
        <p class="hint">{$t(natifMode ? "toolbar.previewNatifHint" : status === "connected" ? "toolbar.previewHint" : "toolbar.previewObsHint")}</p>
      </div>
    {/if}
  </div>

  <!-- Section Écran d'attente : LANCEUR DIRECT (pas un accordéon — le clic
       ouvre la modale de config/contrôle BRB). Le média est affiché sur la
       diffusion uniquement ; le chrono est détenu par Rust. -->
  <div class="section sec-attente">
    <button
      class="header"
      onclick={() => ecranAttenteModalOpen.set(true)}
      title={$t("toolbar.ecranAttenteTitle")}
    >
      <img class="ico" src={icoAttente} alt="" />
      <span>{$t("ecranAttenteModal.titre")}</span>
      <span class="chevron">›</span>
    </button>
  </div>

  <!-- Section Just Chatting : TOGGLE DIRECT (pas un accordéon — le clic
       bascule l'overlay non-destructif de la diffusion : caméra + chat
       seuls, la scène n'est jamais modifiée). La pastille .dot reflète
       l'état runtime — OFF au boot, non persisté. -->
  <div class="section sec-jc">
    <button
      class="header output-btn"
      class:actif={$jcActif}
      onclick={onToggleJC}
      title={$t("toolbar.justChattingTitle")}
    >
      <img class="ico" src={icoJc} alt="" />
      <span>{$t("toolbar.justChatting")}</span>
      <span class="dot" class:on={$jcActif}></span>
    </button>
  </div>

  <!-- ============ GROUPE : INTERACTIONS ============ -->
  <div class="groupe-label">{$t("toolbar.groupeInteractions")}</div>

  <!-- Section Interactions : LANCEUR DIRECT (pas un accordéon — le clic
       ouvre la modale à onglets : Alertes / Bienvenue / Commandes /
       Présentation / Musique + l'outil Position des overlays).
       Tout est centralisé dans la modale, qui s'explique elle-même. -->
  <div class="section sec-interactions">
    <button
      class="header"
      onclick={() => interactionModalOpen.set(true)}
      title={$t("toolbar.interactionsTitle")}
    >
      <img class="ico" src={icoInteractions} alt="" />
      <span>{$t("toolbar.interactions")}</span>
      <span class="chevron">›</span>
    </button>
  </div>

  <!-- ============ GROUPE : MA CHAÎNE ============ -->
  <div class="groupe-label">{$t("toolbar.groupeChaine")}</div>

  <!-- Section Modération : LANCEUR DIRECT (pas un accordéon — le clic
       ouvre la modale à onglets par plateforme fonctionnelle : Twitch /
       YouTube). Gestion des statuts/rôles : ban, timeout, VIP, modérateur,
       suppression de messages, clear chat. -->
  <div class="section sec-moderation">
    <button
      class="header"
      onclick={() => moderationModalOpen.set(true)}
      title={$t("toolbar.moderationTitle")}
    >
      <span>{$t("toolbar.moderation")}</span>
      <span class="chevron">›</span>
    </button>
  </div>

  <!-- Section Planning : LANCEUR DIRECT — ouvre la modale du planning
       Twitch (Helix /schedule), extraite de la modale Interactions :
       gestion de chaîne, pas une interaction viewer. -->
  <div class="section sec-planning">
    <button
      class="header"
      onclick={() => planningModalOpen.set(true)}
      title={$t("toolbar.planningTitle")}
    >
      <span>{$t("toolbar.planning")}</span>
      <span class="chevron">›</span>
    </button>
  </div>

  <!-- Section Statistiques : LANCEUR DIRECT — ouvre la modale d'analyse
       d'audience des lives (télémétrie), extraite de l'ancien Diagnostic :
       lecture post-live de la chaîne. -->
  <div class="section sec-statistiques">
    <button
      class="header"
      onclick={() => statistiquesModalOpen.set(true)}
      title={$t("toolbar.statistiquesTitle")}
    >
      <span>{$t("toolbar.statistiques")}</span>
      <span class="chevron">›</span>
    </button>
  </div>

</aside>

<!-- ============ GROUPE : APPLICATION (pied fixe) ============ -->
<div class="groupe-label groupe-pied">{$t("toolbar.groupeApplication")}</div>

<!-- Pied du panneau latéral : section « Réglages » (accordéon, même
     mécanique que les sections de l'aside — une seule ouverte à la fois).
     Regroupe le fond du CHROME dashboard (topbar + sidebar — le canvas
     reste opaque par-dessus, jamais touché), le placement de la fenêtre
     principale ET la barre de raccourcis TOPMOST (migrée depuis
     l'ancienne section « Raccourcis » de l'aside — même contenu :
     afficher/masquer + bord + moniteur). Sibling de l'aside dans
     .sidebar-col → fixe en bas, HORS de la zone scrollable, toujours
     visible. Carte ardoise portée par la .section quand elle est ouverte ;
     la pastille .dot marque qu'un fond dashboard est posé. -->
<div
  class="section dash-fond sec-ergo"
  class:carte-edition={open === "ergonomie"}
  class:cible-ergo={open === "ergonomie"}
>
  <button class="header" class:ouvert={open === "ergonomie"} onclick={() => toggleSection("ergonomie")}>
    <img class="ico" src={icoErgo} alt="" />
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
      <div class="bord-row">
        {#each MODES as m (m)}
          <button
            class="action small"
            class:actif={fenMode === m}
            onclick={() => onFenMode(m)}
          >{nomsModes[m]}</button>
        {/each}
      </div>
      <select onchange={onFenMoniteur}>
        {#each monsOptions as m (m.index)}
          <option value={m.index} selected={m.index === fenMonIdx}>
            {m.label} — {m.largeur}×{m.hauteur}{m.suffixe}
          </option>
        {/each}
      </select>
      <p class="hint">{$t("toolbar.demarrageHint")}</p>
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
      <p class="hint">{$t("toolbar.raccourcisHint")}</p>
    </div>
  {/if}
</div>

<!-- Section Performances (pied fixe) : LANCEUR DIRECT — le clic ouvre la
     modale de consommation (ex-« Diagnostic » : courbes CPU/RAM par
     processus et par fonction StreamOS + compteurs d'activité, mesures
     réelles 1 Hz). Surveillance technique de l'app, groupe Application. -->
<div class="section sec-performances">
  <button
    class="header"
    onclick={() => consoModalOpen.set(true)}
    title={$t("toolbar.diagnosticTitle")}
  >
    <img class="ico" src={icoDiagnostic} alt="" />
    <span>{$t("toolbar.diagnostic")}</span>
    <span class="chevron">›</span>
  </button>
</div>

<!-- Pied du panneau latéral — lanceur du Guide du streamer : LANCEUR
     DIRECT (pas un accordéon — le clic ouvre l'overlay livre
     feuilletable, même mécanique que Modération / Performances). Sibling
     de l'aside dans .sidebar-col → fixe en bas, HORS de la zone
     scrollable, toujours visible, tout en bas du groupe Application. -->
<div class="section sec-guide">
  <button
    class="header"
    onclick={() => guideLivreOpen.set(true)}
    title={$t("toolbar.guideTitle")}
  >
    <img class="ico" src={icoGuide} alt="" />
    <span>{$t("toolbar.guide")}</span>
    <span class="chevron">›</span>
  </button>
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
    padding-right: 0.375rem;
    scrollbar-width: thin;
    scrollbar-color: var(--bordure-ui-active) transparent;
  }
  /* Scrollbar du panneau : plus fine que la globale (0.375rem), piste
     transparente, pouce aux couleurs de bordure déjà en place. */
  .sidebar::-webkit-scrollbar {
    width: 0.375rem;
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
  /* Couleurs de section PERMANENTES : chaque .sec-* pose indépendamment de
     l'ouverture --accent-carte (sombre, fond/bordure de la carte ouverte —
     .cible-* reste conditionnel) et --accent-crochet (vif, barre gauche de
     l'en-tête au repos — rôles --crochet-*, glace.css). */
  .sec-widgets      { --accent-carte: var(--carte-bleue);    --accent-crochet: var(--crochet-bleu); }
  .sec-connexions   { --accent-carte: var(--carte-verte);    --accent-crochet: var(--crochet-vert); }
  .sec-interactions { --accent-crochet: var(--crochet-rose); }
  .sec-attente      { --accent-crochet: var(--crochet-cyan); }
  .sec-jc           { --accent-crochet: var(--crochet-orange); }
  .sec-moderation   { --accent-crochet: var(--crochet-indigo); }
  /* Performances (pied fixe, lanceur direct) : même ancrage que le pied
     Ergonomie — jamais écrasé quand la liste déborde. */
  .sec-performances { --accent-crochet: var(--crochet-teal); flex-shrink: 0; }
  .sec-planning     { --accent-crochet: var(--crochet-indigo); }
  .sec-statistiques { --accent-crochet: var(--crochet-teal); }
  .sec-obs          { --accent-carte: var(--carte-dore);     --accent-crochet: var(--crochet-dore); }
  /* Bloc « Moteur » (lot 1.5) : compact, à l'intérieur de la carte OBS. */
  .moteur-bloc {
    display: flex;
    flex-direction: column;
    gap: 0.35rem;
    padding-top: 0.45rem;
    border-top: 1px solid var(--bordure-ui);
  }
  .moteur-titre {
    font-size: 0.66rem;
    font-weight: 600;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    opacity: 0.6;
  }
  .moteur-seg {
    display: flex;
    gap: 0.35rem;
  }
  .moteur-btn {
    flex: 1;
  }
  .moteur-btn.actif {
    border-color: var(--bordure-ui-active);
    color: var(--texte);
  }
  .moteur-caps {
    display: flex;
    flex-wrap: wrap;
    gap: 0.25rem;
    margin: 0;
    padding: 0;
    list-style: none;
  }
  .moteur-caps li {
    padding: 0.05rem 0.35rem;
    border: 1px solid var(--bordure-ui);
    border-radius: 999px;
    font-size: 0.64rem;
    opacity: 0.8;
  }
  .moteur-erreur {
    color: var(--t-rouge);
  }
  .moteur-chemin {
    word-break: break-all;
    opacity: 0.7;
  }
  .sec-preview      { --accent-carte: var(--carte-jaune);    --accent-crochet: var(--crochet-jaune); }
  .sec-perso        { --accent-carte: var(--carte-violette); --accent-crochet: var(--crochet-violet); }
  .sec-ergo         { --accent-carte: var(--carte-ardoise);  --accent-crochet: var(--crochet-ardoise); }
  /* Guide (pied fixe, lanceur direct) : crochet ardoise — même famille
     que Réglages/Performances, le bloc Application forme un ensemble. */
  .sec-guide        { --accent-crochet: var(--crochet-ardoise); }
  /* Libellé de groupe dans la sidebar : repère discret (petites
     majuscules espacées, moitié d'opacité) — structure la liste sans
     ajouter de carte ni de couleur au-delà des sections elles-mêmes. */
  .groupe-label {
    padding: 0.5rem 0.55rem 0.15rem;
    font-size: 0.66rem;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.09em;
    opacity: 0.45;
    user-select: none;
  }
  /* Premier groupe : pas de marge haute (le libellé colle au haut du
     panneau comme le faisait la première section). */
  .groupe-label:first-child {
    padding-top: 0.15rem;
  }
  /* Libellé du groupe Application : sibling de l'aside (pied fixe),
     jamais compressé quand la liste déborde. */
  .groupe-pied {
    flex-shrink: 0;
    padding-top: 0.3rem;
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
    /* Crochet permanent : liseré 3px gauche en couleur VIVE de la section
       (--accent-crochet posé par la classe .sec-* permanente — rôles
       --crochet-* niveaux 400-500, lisibles sur --fond-panneau). Fallback
       --accent-ui si une section n'a pas de couleur. Le hover le remplace
       par --accent-ui (.header:hover, spécificité supérieure) ;
       .etat-erreur (rouge, OBS) prime aussi. */
    box-shadow: inset 3px 0 0 var(--accent-crochet, var(--accent-ui));
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
  /* Icônes image (PNG détourés par _ico_trim.ps1) : boîte un cran au-dessus
     des SVG — les glyphes néon ont besoin d'air pour rester lisibles. */
  img.ico {
    width: 1.15rem;
    height: 1.15rem;
    object-fit: contain;
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
  /* Boutons dans la carte : anneau de la couleur de la carte (même token),
     éclairci au hover via color-mix. La recette globale .action (app.css)
     peint sa bordure via le gradient --btn-border — on le réassigne ici
     (border-color seul serait invisible : le border est transparent).
     Hors carte, l'anneau neutre .action est inchangé. */
  .section.carte-edition .action {
    --btn-border: linear-gradient(var(--accent-carte), var(--accent-carte));
    border-color: var(--accent-carte);
  }
  .section.carte-edition .action:hover:not(:disabled) {
    --btn-border: linear-gradient(
      color-mix(in srgb, var(--accent-carte) 70%, var(--texte)),
      color-mix(in srgb, var(--accent-carte) 70%, var(--texte)));
    border-color: color-mix(in srgb, var(--accent-carte) 70%, var(--texte));
  }
  .section.carte-edition .action:disabled,
  .section.carte-edition .action:disabled:hover {
    --btn-border: linear-gradient(var(--accent-carte), var(--accent-carte));
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
  .cible-preview {
    --accent-carte: var(--carte-jaune);
    --accent-carte-glow: color-mix(in srgb, var(--carte-jaune) 45%, transparent);
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
  /* Miniature de prévisualisation : boîte noire au ratio du canvas scène,
     l'iframe (:4321, dims réelles) est réduite par transform depuis le coin
     haut-gauche. pointer-events:none sur l'iframe — le clic monte au bouton
     qui ouvre le projecteur OBS. Chrome bouton global neutralisé (ni blur
     ni sweep/reflet : la surface est un viewport, pas un bouton texte). */
  .preview-thumb {
    display: block;
    width: 100%;
    padding: 0;
    border: 1px solid var(--bordure-ui);
    border-radius: var(--rayon-petit);
    overflow: hidden;
    background: var(--fond);
    cursor: pointer;
    backdrop-filter: none;
    transition: border-color 0.15s ease, box-shadow 0.15s ease;
  }
  .preview-thumb::before,
  .preview-thumb::after {
    content: none;
  }
  .preview-thumb iframe {
    display: block;
    transform-origin: top left;
    pointer-events: none;
    border: 0;
  }
  .preview-thumb:hover:not(:disabled) {
    border-color: var(--bordure-ui-active);
    box-shadow: var(--glow-ui);
  }
  .preview-thumb:disabled {
    opacity: 0.5;
    cursor: not-allowed;
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
  /* Input slug/username : compressible dans la carte (la ligne wrappe
     quand même si nécessaire — le min évite un champ trop étriqué). */
  .plat-input {
    width: 4.4rem;
    min-width: 2.75rem;
    flex-shrink: 1;
    padding: 0.1rem 0.3rem;
    font-size: 0.78rem;
  }
  /* Rangées de boutons de choix (bords barre raccourcis, modes fenêtre). */
  .bord-row {
    display: flex;
    flex-wrap: wrap;
    gap: 0.3rem;
  }
  /* Toggles sorties OBS (stream/record) : pastille + label accentués quand
     la sortie est active — recouvre la recette .action.actif globale
     (anneau --btn-border + texte accentués). border-color posé aussi :
     le toggle Just Chatting est un .header.output-btn dont la bordure
     est une vraie propriété border (pas l'anneau .action). */
  .output-btn {
    display: flex;
    align-items: center;
    gap: 0.45rem;
  }
  .output-btn.actif {
    --btn-border: linear-gradient(var(--accent-ui), var(--accent-ui));
    border-color: var(--accent-ui);
    color: var(--accent-ui);
  }
  /* Le style des selects est global (app.css) — reste la largeur pleine. */
  select {
    width: 100%;
  }
</style>
