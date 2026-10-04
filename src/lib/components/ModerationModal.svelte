<script lang="ts">
  // Modale "Modération & Rôles" — gestion des statuts/rôles des viewers.
  //   Onglet Twitch (fonctionnel) : section Communauté (lecture seule :
  //     broadcaster, viewers live, followers, subs) + sections Viewers/VIPs/
  //     Mods/Bannis/Messages (action : ban/timeout/unban, VIP/modérateur,
  //     suppression de messages, clear chat).
  //   Onglet YouTube (fonctionnel) : communauté YouTube en lecture seule
  //     (channel, viewers live, stats, members).
  //   Onglets Kick/TikTok/Trovo : "Bientôt disponible" (grisés).
  //
  // Réutilise les données followers/subs du store communaute.ts (chargées
  // réactivement dans App.svelte à la connexion) + recherche par pseudo via
  // Helix /users. Convention : Modal.svelte + onglets .tabs/.tab.
  import { connexions, chatMessages } from "../stores/chat";
  import {
    followers,
    subs,
    viewers,
    broadcaster,
    communauteErreur,
    chargerCommunaute,
    chargerCommunauteYoutube,
    youtubeChannel,
    youtubeMembers,
    youtubeViewers,
    unfollows,
  } from "../stores/communaute";
  import {
    vips,
    moderateurs,
    bannis,
    moderationErreur,
    chargerToutMod,
    resoudreUser,
    bannir,
    debannir,
    ajouterVip,
    retirerVip,
    ajouterModerateur,
    retirerModerateur,
    supprimerMessage,
    clearChat,
    type BannedEntry,
    type ResolvedUser,
  } from "../stores/moderation";
  import {
    channelInfo,
    chatSettings,
    shieldActif,
    raidEnCours,
    unbanRequests,
    automodFile,
    liveErreur,
    chargerLive,
    majChannel,
    rechercherCategories,
    infosJeu,
    rechercherChannels,
    creerMarker,
    lancerPub,
    lancerRaid,
    annulerRaid,
    patchChatSettings,
    envoyerAnnonce,
    setShield,
    resoudreUnban,
    resoudreAutomod,
    type CategoryEntry,
    type ChannelSearchEntry,
  } from "../stores/twitchLive";
  import { poserJacquette } from "../stores/scene";
  import { reconnecterTwitch } from "../stores/chat";
  import { t } from "../stores/langue";
  import { get } from "svelte/store";
  import Modal from "./Modal.svelte";

  let { onFermer = () => {} }: { onFermer?: () => void } = $props();

  // Onglet actif (Twitch et YouTube sont fonctionnels).
  let tab = $state<"twitch" | "kick" | "youtube" | "tiktok" | "trovo">("twitch");

  // Section sélectionnée dans l'onglet Twitch (master-detail).
  // « communaute » = supervision lecture seule (broadcaster, viewers live,
  // followers, subs). Les autres sections = action (ban/vip/mod/messages).
  let sec = $state<"communaute" | "direct" | "viewers" | "vips" | "mods" | "bannis" | "messages" | "unfollows">("communaute");

  // Recherche par pseudo.
  let recherche = $state("");
  let resultatRecherche = $state<ResolvedUser | null>(null);
  let rechercheEnCours = $state(false);
  let rechercheErreur = $state<string | null>(null);

  // État d'action (loading sur boutons).
  let actionEnCours = $state<string | null>(null);

  // Données dérivées.
  let cx = $derived($connexions);
  let followersData = $derived($followers);
  let subsData = $derived($subs);
  let viewersData = $derived($viewers);
  let broadcasterData = $derived($broadcaster);
  let vipsData = $derived($vips);
  let modsData = $derived($moderateurs);
  let bannisData = $derived($bannis);
  let modErr = $derived($moderationErreur);
  let commErr = $derived($communauteErreur);
  let messagesData = $derived($chatMessages);

  // Données YouTube (onglet Communauté YouTube).
  let ytChannel = $derived($youtubeChannel);
  let ytMembers = $derived($youtubeMembers);
  let ytViewers = $derived($youtubeViewers);

  // Unfollows (historique des unfollows détectés au démarrage).
  let unfollowsData = $derived($unfollows);

  // Messages Twitch uniquement (pour la suppression).
  let messagesTwitch = $derived(
    messagesData.filter((m) => m.plateforme === "twitch" && m.message_id)
  );

  // ===== Section « En direct » (Helix — store twitchLive) =====
  let liveErr = $derived($liveErreur);
  let infoData = $derived($channelInfo);
  let chatSet = $derived($chatSettings);
  let shieldData = $derived($shieldActif);
  let raidData = $derived($raidEnCours);
  let unbanData = $derived($unbanRequests);
  let automodData = $derived($automodFile);

  // État local des formulaires (rien n'est persisté — Helix est relu
  // à chaque ouverture de la section).
  let liveMsg = $state<{ texte: string; ok: boolean } | null>(null);
  let liveTitre = $state("");
  let liveCategorie = $state("");
  let liveGameId = $state("");
  let liveBoxArt = $state("");
  let liveTags = $state("");
  let catSugg = $state<CategoryEntry[]>([]);
  let raidCible = $state("");
  let raidSugg = $state<ChannelSearchEntry[]>([]);
  let slowTime = $state(30);
  let followerDur = $state(0);
  let annonceMsg = $state("");
  let annonceCouleur = $state("primary");
  let markerDesc = $state("");
  let shieldConfirm = $state(false);
  let infoInit = $state(false);
  let modesInit = $state(false);
  let catTimer: ReturnType<typeof setTimeout> | undefined;
  let raidTimer: ReturnType<typeof setTimeout> | undefined;

  // Relit Helix à l'ouverture de la section « En direct » (paresseux).
  $effect(() => {
    if (tab === "twitch" && sec === "direct" && cx.twitch) {
      void chargerLive();
    }
  });

  // Préremplit le formulaire infos quand GET /channels arrive (une fois
  // par ouverture — ne pas écraser une saisie en cours).
  $effect(() => {
    if (sec === "direct" && infoData && !infoInit) {
      liveTitre = infoData.title;
      liveCategorie = infoData.game_name;
      liveGameId = infoData.game_id;
      liveTags = infoData.tags.join(", ");
      infoInit = true;
      // Jacquette : GET /games?id= quand le game_id est déjà connu (la
      // recherche de catégories reste la source au choix). Non-fatal.
      if (infoData.game_id) {
        void infosJeu(infoData.game_id)
          .then((j) => { liveBoxArt = j?.box_art_url ?? ""; })
          .catch(() => {});
      }
    }
  });

  // Idem pour les délais des modes de chat (GET /chat/settings).
  $effect(() => {
    if (sec === "direct" && chatSet && !modesInit) {
      slowTime = chatSet.slow_mode_wait_time ?? 30;
      followerDur = chatSet.follower_mode_duration ?? 0;
      modesInit = true;
    }
  });

  // En quittant la section : reset des flags de préremplissage.
  $effect(() => {
    if (sec !== "direct") {
      infoInit = false;
      modesInit = false;
      shieldConfirm = false;
      liveMsg = null;
      catSugg = [];
      raidSugg = [];
      liveBoxArt = "";
    }
  });

  /// Wrapper d'action live : loading + message local (ok/erreur).
  /// need_reauth → banner existant via liveErreur, pas de message brut.
  async function executerLive(id: string, fn: () => Promise<void>): Promise<void> {
    actionEnCours = id;
    liveMsg = null;
    try {
      await fn();
    } catch (e) {
      const msg = String(e);
      if (msg !== "need_reauth") {
        liveMsg = { texte: msg, ok: false };
      }
    } finally {
      actionEnCours = null;
    }
  }

  /// Appliquer titre/catégorie/tags — PATCH partiel : seuls les champs
  /// modifiés sont envoyés (tags omis si identiques ou vides).
  async function onAppliquerInfos(): Promise<void> {
    const opts: { title?: string; gameId?: string; tags?: string[] } = {};
    const titre = liveTitre.trim();
    if (titre !== (infoData?.title ?? "")) opts.title = titre;
    if (liveGameId && liveGameId !== (infoData?.game_id ?? "")) {
      opts.gameId = liveGameId;
    }
    const tags = liveTags
      .split(",")
      .map((tg) => tg.trim())
      .filter((tg) => tg.length > 0);
    const tagsActuels = infoData?.tags ?? [];
    if (tags.length > 0 && tags.join("\n") !== tagsActuels.join("\n")) {
      opts.tags = tags;
    }
    if (Object.keys(opts).length === 0) return;
    await executerLive("infos", async () => {
      await majChannel(opts);
      liveMsg = { texte: get(t)("moderationModal.live.infosOk"), ok: true };
    });
  }

  /// Recherche de catégorie à la saisie (debounce 300 ms).
  function onCategorieInput(): void {
    liveGameId = "";
    liveBoxArt = "";
    clearTimeout(catTimer);
    const q = liveCategorie.trim();
    if (q.length < 2) {
      catSugg = [];
      return;
    }
    catTimer = setTimeout(() => {
      void (async () => {
        try {
          catSugg = await rechercherCategories(q);
        } catch {
          catSugg = [];
        }
      })();
    }, 300);
  }

  /// Choisir une catégorie dans les suggestions.
  function onChoisirCategorie(c: CategoryEntry): void {
    liveCategorie = c.name;
    liveGameId = c.id;
    liveBoxArt = c.box_art_url;
    catSugg = [];
  }

  /// Pose la jacquette du jeu courant sur la scène : download Rust gardé +
  /// upsert par nom de fichier dans le store scène (pas de doublon).
  function onPoserJacquette(): void {
    if (!liveBoxArt || !liveGameId) return;
    const url = liveBoxArt;
    const gameId = liveGameId;
    const nom = liveCategorie.trim() || gameId;
    void executerLive("jacquette", async () => {
      await poserJacquette(url, gameId, nom);
      liveMsg = { texte: get(t)("moderationModal.live.jacquetteOk"), ok: true };
    });
  }

  /// Bascule un mode de chat (PATCH partiel — un seul champ envoyé).
  function onToggleMode(
    mode: "slow_mode" | "subscriber_mode" | "emote_mode" | "follower_mode",
    actif: boolean
  ): void {
    void executerLive(mode, () => patchChatSettings({ [mode]: actif }));
  }

  /// Appliquer le délai du mode lent (3–120 s, borné par Helix).
  function onDelaiLent(): void {
    const v = Math.min(120, Math.max(3, Math.round(Number(slowTime) || 3)));
    void executerLive("slow_time", () =>
      patchChatSettings({ slow_mode_wait_time: v })
    );
  }

  /// Appliquer la durée follower (minutes, 0–129600).
  function onDureeFollowers(): void {
    const v = Math.min(129600, Math.max(0, Math.round(Number(followerDur) || 0)));
    void executerLive("follower_dur", () =>
      patchChatSettings({ follower_mode_duration: v })
    );
  }

  /// Envoyer une annonce colorée.
  function onAnnonce(): void {
    const msg = annonceMsg.trim();
    if (!msg) return;
    void executerLive("annonce", async () => {
      await envoyerAnnonce(msg, annonceCouleur);
      annonceMsg = "";
      liveMsg = { texte: get(t)("moderationModal.live.annonceOk"), ok: true };
    });
  }

  /// Poser un marqueur de stream (description optionnelle).
  function onMarker(): void {
    void executerLive("marker", async () => {
      await creerMarker(markerDesc.trim() || undefined);
      markerDesc = "";
      liveMsg = { texte: get(t)("moderationModal.live.markerOk"), ok: true };
    });
  }

  /// Lancer une pub (durées Helix figées : 30–180 s).
  function onPub(duree: number): void {
    void executerLive(`pub-${duree}`, async () => {
      await lancerPub(duree);
      liveMsg = { texte: get(t)("moderationModal.live.pubOk"), ok: true };
    });
  }

  /// Recherche de chaîne cible pour le raid (debounce 300 ms).
  function onRaidInput(): void {
    clearTimeout(raidTimer);
    const q = raidCible.trim();
    if (q.length < 2) {
      raidSugg = [];
      return;
    }
    raidTimer = setTimeout(() => {
      void (async () => {
        try {
          raidSugg = await rechercherChannels(q);
        } catch {
          raidSugg = [];
        }
      })();
    }, 300);
  }

  /// Choisir une chaîne dans les suggestions raid.
  function onChoisirRaid(c: ChannelSearchEntry): void {
    raidCible = c.login;
    raidSugg = [];
  }

  /// Lancer le raid vers la cible.
  function onLancerRaid(): void {
    const login = raidCible.trim();
    if (!login) return;
    void executerLive("raid", async () => {
      await lancerRaid(login);
      raidSugg = [];
      liveMsg = { texte: get(t)("moderationModal.live.raidOk"), ok: true };
    });
  }

  /// Annuler le raid en cours.
  function onAnnulerRaid(): void {
    void executerLive("raid-cancel", async () => {
      await annulerRaid();
      liveMsg = { texte: get(t)("moderationModal.live.raidAnnuleOk"), ok: true };
    });
  }

  /// Shield Mode : activation → confirmation explicite (vide le chat et
  /// force les modes) ; désactivation immédiate.
  function onShieldToggle(actif: boolean): void {
    if (actif) {
      shieldConfirm = true;
      return;
    }
    void executerLive("shield", () => setShield(false));
  }

  /// Confirmer l'activation du Shield Mode.
  function onShieldConfirmer(): void {
    shieldConfirm = false;
    void executerLive("shield", async () => {
      await setShield(true);
      liveMsg = { texte: get(t)("moderationModal.live.shieldOk"), ok: true };
    });
  }

  /// Approuver/refuser une demande d'unban.
  function onUnbanResoudre(id: string, approuver: boolean): void {
    void executerLive(`unbanreq-${id}`, () => resoudreUnban(id, approuver));
  }

  /// Autoriser/refuser un message retenu par AutoMod.
  function onAutomodResoudre(msgId: string, autoriser: boolean): void {
    void executerLive(`automod-${msgId}`, () =>
      resoudreAutomod(msgId, autoriser)
    );
  }

  // Charger les listes à l'ouverture de la modale.
  $effect(() => {
    if (tab === "twitch" && cx.twitch && commErr !== "need_reauth") {
      void chargerToutMod();
      void chargerCommunaute();
    }
  });

  // Charger la communauté YouTube quand l'onglet YouTube est actif.
  $effect(() => {
    if (tab === "youtube" && cx.youtube) {
      void chargerCommunauteYoutube();
    }
  });

  /// Lance la recherche par pseudo.
  async function onRechercher(): Promise<void> {
    const q = recherche.trim();
    if (!q) {
      resultatRecherche = null;
      rechercheErreur = null;
      return;
    }
    rechercheEnCours = true;
    rechercheErreur = null;
    try {
      resultatRecherche = await resoudreUser(q);
      if (!resultatRecherche) {
        rechercheErreur = get(t)("moderationModal.introuvable");
      }
    } catch (e) {
      rechercheErreur = String(e);
      resultatRecherche = null;
    } finally {
      rechercheEnCours = false;
    }
  }

  /// Wrapper d'action : set actionEnCours + catch erreur.
  async function executerAction(
    id: string,
    fn: () => Promise<void>
  ): Promise<void> {
    actionEnCours = id;
    try {
      await fn();
    } catch (e) {
      const msg = String(e);
      if (msg === "need_reauth") {
        moderationErreur.set("need_reauth");
      } else {
        console.error("[Modération] action:", msg);
      }
    } finally {
      actionEnCours = null;
    }
  }

  /// Bannir un viewer (ban permanent).
  function onBan(userId: string): void {
    void executerAction(`ban-${userId}`, () =>
      bannir(userId, get(t)("moderationModal.raisonBan"), undefined)
    );
  }

  /// Timeout un viewer (durée en secondes).
  function onTimeout(userId: string, duree: number): void {
    void executerAction(`timeout-${userId}`, () =>
      bannir(userId, get(t)("moderationModal.raisonTimeout", { duree }), duree)
    );
  }

  /// Débannir un viewer.
  function onUnban(userId: string): void {
    void executerAction(`unban-${userId}`, () => debannir(userId));
  }

  /// Ajouter VIP.
  function onAjouterVip(userId: string): void {
    void executerAction(`addvip-${userId}`, () => ajouterVip(userId));
  }

  /// Retirer VIP.
  function onRetirerVip(userId: string): void {
    void executerAction(`rmvip-${userId}`, () => retirerVip(userId));
  }

  /// Ajouter modérateur.
  function onAjouterModerateur(userId: string): void {
    void executerAction(`addmod-${userId}`, () => ajouterModerateur(userId));
  }

  /// Retirer modérateur.
  function onRetirerMod(userId: string): void {
    void executerAction(`rmmod-${userId}`, () => retirerModerateur(userId));
  }

  /// Supprimer un message.
  function onSupprimerMessage(messageId: string): void {
    void executerAction(`delmsg-${messageId}`, () =>
      supprimerMessage(messageId)
    );
  }

  /// Vider le chat (commande IRC /clear).
  async function onClearChat(): Promise<void> {
    await executerAction("clear", async () => {
      await clearChat();
    });
  }

  /// Reconnecter Twitch (nouveaux scopes modération).
  function onReconnecter(): void {
    void reconnecterTwitch();
  }

  /// Rafraîchir la communauté (Twitch + YouTube si connecté).
  async function onRafraichirCommunaute(): Promise<void> {
    if (cx.twitch) await chargerCommunaute();
    if (cx.youtube) await chargerCommunauteYoutube();
  }

  /// Détermine si un ban est un timeout (expires_at non null).
  function estTimeout(b: BannedEntry): boolean {
    return b.expires_at !== null;
  }

  /// Formate la durée restante d'un timeout.
  function dureeTimeout(b: BannedEntry): string {
    if (!b.expires_at) return get(t)("moderationModal.permanent");
    try {
      const reste = new Date(b.expires_at).getTime() - Date.now();
      if (reste <= 0) return get(t)("moderationModal.expire");
      const min = Math.floor(reste / 60000);
      if (min < 60) return get(t)("moderationModal.dureeMin", { min });
      const h = Math.floor(min / 60);
      if (h < 24) return get(t)("moderationModal.dureeH", { h, min: min % 60 });
      const j = Math.floor(h / 24);
      return get(t)("moderationModal.dureeJ", { j, h: h % 24 });
    } catch {
      return "?";
    }
  }
</script>

<Modal title={$t("moderationModal.titre")} onClose={onFermer} maxWidth="900px">
  <div class="tabs">
    <button class="tab" class:active={tab === "twitch"} onclick={() => (tab = "twitch")}>
      Twitch
    </button>
    <button class="tab" disabled>Kick</button>
    <button class="tab" class:active={tab === "youtube"} onclick={() => (tab = "youtube")}>
      YouTube
    </button>
    <button class="tab" disabled>TikTok</button>
    <button class="tab" disabled>Trovo</button>
  </div>

  {#if tab === "twitch"}
    {#if !cx.twitch}
      <div class="tab-body">
        <p class="hint-centre">{$t("moderationModal.connecterTwitch")}</p>
      </div>
    {:else if modErr === "need_reauth" || commErr === "need_reauth" || liveErr === "need_reauth"}
      <div class="tab-body">
        <div class="need-reauth">
          <span>{$t("moderationModal.scopesManquants")}</span>
          <button class="action" onclick={onReconnecter}>
            {$t("moderationModal.reconnecter")}
          </button>
        </div>
      </div>
    {:else}
      <div class="tab-body">
        <div class="mod-layout">
          <!-- Colonne gauche : sections -->
          <div class="mod-sections">
            <button
              class="mod-select"
              class:selected={sec === "communaute"}
              onclick={() => (sec = "communaute")}
            >
              <span class="mod-nom">{$t("moderationModal.secCommunaute")}</span>
            </button>
            <button
              class="mod-select"
              class:selected={sec === "direct"}
              onclick={() => (sec = "direct")}
            >
              <span class="mod-nom">{$t("moderationModal.enDirect")}</span>
            </button>
            <button
              class="mod-select"
              class:selected={sec === "viewers"}
              onclick={() => (sec = "viewers")}
            >
              <span class="mod-nom">{$t("moderationModal.secViewers")}</span>
            </button>
            <button
              class="mod-select"
              class:selected={sec === "vips"}
              onclick={() => (sec = "vips")}
            >
              <span class="mod-nom">{$t("moderationModal.secVips")} ({vipsData?.length ?? "…"})</span>
            </button>
            <button
              class="mod-select"
              class:selected={sec === "mods"}
              onclick={() => (sec = "mods")}
            >
              <span class="mod-nom">{$t("moderationModal.secMods")} ({modsData?.length ?? "…"})</span>
            </button>
            <button
              class="mod-select"
              class:selected={sec === "bannis"}
              onclick={() => (sec = "bannis")}
            >
              <span class="mod-nom">{$t("moderationModal.secBannis")} ({bannisData?.length ?? "…"})</span>
            </button>
            <button
              class="mod-select"
              class:selected={sec === "messages"}
              onclick={() => (sec = "messages")}
            >
              <span class="mod-nom">{$t("moderationModal.secMessages")}</span>
            </button>
            <button
              class="mod-select"
              class:selected={sec === "unfollows"}
              onclick={() => (sec = "unfollows")}
            >
              <span class="mod-nom">{$t("moderationModal.secUnfollows")} ({unfollowsData?.length ?? "…"})</span>
            </button>
          </div>

          <!-- Panneau droit : contenu de la section -->
          <div class="mod-detail">
            {#if sec === "communaute"}
              <!-- Broadcaster -->
              {#if broadcasterData}
                <div class="bloc">
                  <div class="bloc-titre">{$t("moderationModal.chaine")}</div>
                  <div class="broadcaster">
                    {#if broadcasterData.profile_image_url}
                      <img
                        class="avatar"
                        src={broadcasterData.profile_image_url}
                        alt={broadcasterData.display_name}
                      />
                    {/if}
                    <div class="bc-info">
                      <span class="bc-name">{broadcasterData.display_name}</span>
                      <span class="bc-type">{broadcasterData.broadcaster_type || $t("moderationModal.aucunType")}</span>
                    </div>
                  </div>
                </div>
              {/if}

              <!-- Viewers live -->
              <div class="bloc">
                <div class="bloc-titre">{$t("moderationModal.viewersLive")}</div>
                <div class="communaute-row">
                  <span class="field-label">{$t("moderationModal.enDirect")}</span>
                  <span class="communaute-val">
                    {#if viewersData !== null}
                      {viewersData}
                    {:else}
                      {$t("moderationModal.horsLigne")}
                    {/if}
                  </span>
                </div>
              </div>

              <!-- Followers -->
              <div class="bloc">
                <div class="bloc-titre">Followers ({followersData?.total ?? "…"})</div>
                {#if followersData}
                  <div class="liste-scroll">
                    {#each followersData.liste as f}
                      <div class="liste-entry">
                        <div class="liste-user">
                          {#if f.profile_image_url}
                            <img class="liste-avatar" src={f.profile_image_url} alt={f.display_name || f.login} />
                          {/if}
                          <span class="liste-login">{f.display_name || f.login}</span>
                        </div>
                        <span class="liste-date">{f.followed_at.slice(0, 10)}</span>
                      </div>
                    {/each}
                  </div>
                {:else}
                  <p class="hint">{$t("moderationModal.chargementFollowers")}</p>
                {/if}
              </div>

              <!-- Subs -->
              <div class="bloc">
                <div class="bloc-titre">
                  Subs ({subsData?.total ?? "…"} · {subsData?.points ?? "…"} pts)
                </div>
                {#if subsData && subsData.liste.length > 0}
                  <div class="liste-scroll">
                    {#each subsData.liste as s}
                      <div class="liste-entry">
                        <div class="liste-user">
                          {#if s.profile_image_url}
                            <img class="liste-avatar" src={s.profile_image_url} alt={s.display_name || s.login} />
                          {/if}
                          <span class="liste-login">{s.display_name || s.login}</span>
                        </div>
                        <span class="liste-tier">
                          {s.tier === "1000" ? "Tier 1" : s.tier === "2000" ? "Tier 2" : s.tier === "3000" ? "Tier 3" : s.tier}
                          {#if s.is_gift} (gift){/if}
                        </span>
                      </div>
                    {/each}
                  </div>
                {:else}
                  <p class="hint">{$t("moderationModal.aucunSub")}</p>
                {/if}
              </div>

              <button class="action btn-mod" onclick={() => void onRafraichirCommunaute()}>
                {$t("moderationModal.rafraichir")}
              </button>
            {:else if sec === "direct"}
              <!-- Mode Bouclier -->
              <div class="bloc">
                <div class="bloc-titre">{$t("moderationModal.live.shield")}</div>
                <div class="communaute-row">
                  <span class="field-label">{$t("moderationModal.live.shieldStatut")}</span>
                  <span class="communaute-val">
                    {#if shieldData === null}
                      …
                    {:else if shieldData}
                      {$t("moderationModal.live.shieldActif")}
                    {:else}
                      {$t("moderationModal.live.shieldInactif")}
                    {/if}
                  </span>
                </div>
                {#if shieldData === false && !shieldConfirm}
                  <button
                    class="action btn-mod"
                    onclick={() => onShieldToggle(true)}
                    disabled={actionEnCours === "shield"}
                  >{$t("moderationModal.live.shieldActiver")}</button>
                {:else if shieldData === true}
                  <button
                    class="action btn-mod ban"
                    onclick={() => onShieldToggle(false)}
                    disabled={actionEnCours === "shield"}
                  >{$t("moderationModal.live.shieldDesactiver")}</button>
                {/if}
                {#if shieldConfirm}
                  <div class="confirm-bloc">
                    <p class="confirm-texte">{$t("moderationModal.live.shieldAvertissement")}</p>
                    <div class="confirm-actions">
                      <button
                        class="btn-mod ban"
                        onclick={onShieldConfirmer}
                        disabled={actionEnCours === "shield"}
                      >{$t("moderationModal.live.shieldConfirmer")}</button>
                      <button class="btn-mod" onclick={() => (shieldConfirm = false)}>
                        {$t("moderationModal.live.shieldAnnuler")}
                      </button>
                    </div>
                  </div>
                {/if}
              </div>

              <!-- Infos du stream (titre, catégorie, tags) -->
              <div class="bloc">
                <div class="bloc-titre">{$t("moderationModal.live.infos")}</div>
                <label class="field-label" for="live-titre">{$t("moderationModal.live.titreStream")}</label>
                <input id="live-titre" type="text" maxlength="140" bind:value={liveTitre} />
                <label class="field-label" for="live-cat">{$t("moderationModal.live.categorie")}</label>
                <div class="recherche-row">
                  <input
                    id="live-cat"
                    type="text"
                    placeholder={$t("moderationModal.live.rechercherCategorie")}
                    bind:value={liveCategorie}
                    oninput={onCategorieInput}
                  />
                  {#if liveBoxArt}
                    <img
                      class="jacquette"
                      src={liveBoxArt.replace("{width}", "52").replace("{height}", "72")}
                      alt={$t("moderationModal.live.jacquette")}
                    />
                  {/if}
                </div>
                {#if catSugg.length > 0}
                  <div class="sugg-liste">
                    {#each catSugg as c}
                      <button class="sugg-item" onclick={() => onChoisirCategorie(c)}>
                        {#if c.box_art_url}
                          <img
                            class="sugg-img"
                            src={c.box_art_url.replace("{width}", "52").replace("{height}", "72")}
                            alt=""
                          />
                        {/if}
                        <span>{c.name}</span>
                      </button>
                    {/each}
                  </div>
                {/if}
                <button
                  class="action btn-mod"
                  onclick={onPoserJacquette}
                  disabled={!liveGameId || !liveBoxArt || actionEnCours === "jacquette"}
                >{$t("moderationModal.live.poserLayout")}</button>
                <label class="field-label" for="live-tags">{$t("moderationModal.live.tags")}</label>
                <input id="live-tags" type="text" bind:value={liveTags} />
                <button
                  class="action btn-mod"
                  onclick={() => void onAppliquerInfos()}
                  disabled={actionEnCours === "infos"}
                >{$t("moderationModal.live.appliquer")}</button>
              </div>

              <!-- Modes du chat -->
              <div class="bloc">
                <div class="bloc-titre">{$t("moderationModal.live.modes")}</div>
                {#if chatSet === null}
                  <p class="hint">{$t("commun.chargement")}</p>
                {:else}
                  {@const cs = chatSet}
                  <div class="mode-row">
                    <label class="toggle">
                      <input
                        type="checkbox"
                        checked={cs.slow_mode}
                        disabled={actionEnCours === "slow_mode"}
                        onchange={() => onToggleMode("slow_mode", !cs.slow_mode)}
                      />
                      <span>{$t("moderationModal.live.modeLent")}</span>
                    </label>
                    <div class="mode-delai">
                      <input type="number" min="3" max="120" bind:value={slowTime} />
                      <span class="hint-inline">{$t("moderationModal.live.delaiLent")}</span>
                      <button
                        class="btn-mod"
                        onclick={onDelaiLent}
                        disabled={actionEnCours === "slow_time"}
                      >✓</button>
                    </div>
                  </div>
                  <div class="mode-row">
                    <label class="toggle">
                      <input
                        type="checkbox"
                        checked={cs.subscriber_mode}
                        disabled={actionEnCours === "subscriber_mode"}
                        onchange={() => onToggleMode("subscriber_mode", !cs.subscriber_mode)}
                      />
                      <span>{$t("moderationModal.live.modeAbonnes")}</span>
                    </label>
                  </div>
                  <div class="mode-row">
                    <label class="toggle">
                      <input
                        type="checkbox"
                        checked={cs.emote_mode}
                        disabled={actionEnCours === "emote_mode"}
                        onchange={() => onToggleMode("emote_mode", !cs.emote_mode)}
                      />
                      <span>{$t("moderationModal.live.modeEmotes")}</span>
                    </label>
                  </div>
                  <div class="mode-row">
                    <label class="toggle">
                      <input
                        type="checkbox"
                        checked={cs.follower_mode}
                        disabled={actionEnCours === "follower_mode"}
                        onchange={() => onToggleMode("follower_mode", !cs.follower_mode)}
                      />
                      <span>{$t("moderationModal.live.modeFollowers")}</span>
                    </label>
                    <div class="mode-delai">
                      <input type="number" min="0" max="129600" bind:value={followerDur} />
                      <span class="hint-inline">{$t("moderationModal.live.dureeFollowers")}</span>
                      <button
                        class="btn-mod"
                        onclick={onDureeFollowers}
                        disabled={actionEnCours === "follower_dur"}
                      >✓</button>
                    </div>
                  </div>
                {/if}
              </div>

              <!-- Annonces colorées -->
              <div class="bloc">
                <div class="bloc-titre">{$t("moderationModal.live.annonces")}</div>
                <div class="recherche-row">
                  <input
                    type="text"
                    maxlength="500"
                    placeholder={$t("moderationModal.live.annonceMessage")}
                    bind:value={annonceMsg}
                    onkeydown={(e) => { if (e.key === "Enter") onAnnonce(); }}
                  />
                </div>
                <div class="chip-row">
                  {#each ["primary", "blue", "green", "orange", "purple"] as c}
                    <button
                      class="chip"
                      class:actif={annonceCouleur === c}
                      onclick={() => (annonceCouleur = c)}
                    >{c}</button>
                  {/each}
                </div>
                <button
                  class="action btn-mod"
                  onclick={onAnnonce}
                  disabled={actionEnCours === "annonce" || !annonceMsg.trim()}
                >{$t("moderationModal.live.annonceEnvoyer")}</button>
              </div>

              <!-- Actions en direct : marqueur, pub, raid -->
              <div class="bloc">
                <div class="bloc-titre">{$t("moderationModal.live.actions")}</div>
                <div class="recherche-row">
                  <input
                    type="text"
                    maxlength="140"
                    placeholder={$t("moderationModal.live.markerDesc")}
                    bind:value={markerDesc}
                  />
                  <button
                    class="action btn-mod"
                    onclick={onMarker}
                    disabled={actionEnCours === "marker"}
                  >{$t("moderationModal.live.marker")}</button>
                </div>
                <span class="field-label">{$t("moderationModal.live.pub")}</span>
                <div class="chip-row">
                  {#each [30, 60, 90, 120, 150, 180] as d}
                    <button
                      class="chip"
                      onclick={() => onPub(d)}
                      disabled={actionEnCours === `pub-${d}`}
                    >{d}s</button>
                  {/each}
                </div>
                <span class="field-label">{$t("moderationModal.live.raid")}</span>
                <div class="recherche-row">
                  <input
                    type="text"
                    placeholder={$t("moderationModal.live.raidCible")}
                    bind:value={raidCible}
                    oninput={onRaidInput}
                  />
                  {#if raidData}
                    <button
                      class="btn-mod danger"
                      onclick={onAnnulerRaid}
                      disabled={actionEnCours === "raid-cancel"}
                    >{$t("moderationModal.live.raidAnnuler")}</button>
                  {:else}
                    <button
                      class="action btn-mod"
                      onclick={onLancerRaid}
                      disabled={actionEnCours === "raid" || !raidCible.trim()}
                    >{$t("moderationModal.live.raidLancer")}</button>
                  {/if}
                </div>
                {#if raidSugg.length > 0}
                  <div class="sugg-liste">
                    {#each raidSugg as c}
                      <button class="sugg-item" onclick={() => onChoisirRaid(c)}>
                        {#if c.thumbnail_url}
                          <img class="sugg-img sugg-avatar" src={c.thumbnail_url} alt="" />
                        {/if}
                        <span>
                          {c.display_name}{#if c.is_live} · {$t("moderationModal.enDirect")}{/if}
                        </span>
                      </button>
                    {/each}
                  </div>
                {/if}
              </div>

              <!-- File AutoMod (EventSub automod.message.hold) -->
              <div class="bloc">
                <div class="bloc-titre">{$t("moderationModal.live.automod", { n: automodData.length })}</div>
                {#if automodData.length === 0}
                  <p class="hint">{$t("moderationModal.live.automodVide")}</p>
                {:else}
                  <div class="liste-scroll">
                    {#each automodData as m}
                      <div class="msg-row">
                        <div class="viewer-info">
                          <span class="msg-pseudo">{m.display_name || m.login}</span>
                          <span class="msg-texte">{m.text}</span>
                          {#if m.category}
                            <span class="viewer-tag">{m.category}</span>
                          {/if}
                        </div>
                        <div class="viewer-actions">
                          <button
                            class="btn-mod mod"
                            disabled={actionEnCours === `automod-${m.msg_id}`}
                            onclick={() => onAutomodResoudre(m.msg_id, true)}
                          >{$t("moderationModal.live.autoriser")}</button>
                          <button
                            class="btn-mod danger"
                            disabled={actionEnCours === `automod-${m.msg_id}`}
                            onclick={() => onAutomodResoudre(m.msg_id, false)}
                          >{$t("moderationModal.live.refuser")}</button>
                        </div>
                      </div>
                    {/each}
                  </div>
                {/if}
              </div>

              <!-- Demandes d'unban -->
              <div class="bloc">
                <div class="bloc-titre">{$t("moderationModal.live.unban")}</div>
                {#if unbanData === null}
                  <p class="hint">{$t("commun.chargement")}</p>
                {:else if unbanData.length === 0}
                  <p class="hint">{$t("moderationModal.live.unbanVide")}</p>
                {:else}
                  <div class="liste-scroll">
                    {#each unbanData as u}
                      <div class="viewer-row">
                        <div class="viewer-info">
                          <span class="viewer-login">{u.display_name || u.login}</span>
                          {#if u.text}
                            <span class="viewer-reason">{u.text}</span>
                          {/if}
                          <span class="liste-date">{u.created_at.slice(0, 10)}</span>
                        </div>
                        <div class="viewer-actions">
                          <button
                            class="btn-mod debannir"
                            disabled={actionEnCours === `unbanreq-${u.id}`}
                            onclick={() => onUnbanResoudre(u.id, true)}
                          >{$t("moderationModal.live.approuver")}</button>
                          <button
                            class="btn-mod danger"
                            disabled={actionEnCours === `unbanreq-${u.id}`}
                            onclick={() => onUnbanResoudre(u.id, false)}
                          >{$t("moderationModal.live.refuser")}</button>
                        </div>
                      </div>
                    {/each}
                  </div>
                {/if}
              </div>

              <!-- Message de retour d'action -->
              {#if liveMsg}
                <p class="live-msg" class:ok={liveMsg.ok}>{liveMsg.texte}</p>
              {/if}
            {:else if sec === "viewers"}
              <!-- Recherche par pseudo -->
              <div class="bloc">
                <div class="bloc-titre">{$t("moderationModal.rechercherViewer")}</div>
                <div class="recherche-row">
                  <input
                    type="text"
                    placeholder={$t("moderationModal.pseudoTwitch")}
                    bind:value={recherche}
                    onkeydown={(e) => { if (e.key === "Enter") void onRechercher(); }}
                  />
                  <button class="action btn-mod" onclick={() => void onRechercher()} disabled={rechercheEnCours}>
                    {rechercheEnCours ? "…" : $t("commun.rechercher")}
                  </button>
                </div>
                {#if rechercheErreur}
                  <p class="hint-erreur">{rechercheErreur}</p>
                {/if}
                {#if resultatRecherche}
                  {@const r = resultatRecherche}
                  <div class="viewer-row">
                    <span class="viewer-login">{r.display_name}</span>
                    <div class="viewer-actions">
                      <button
                        class="btn-mod ban"
                        disabled={actionEnCours === `ban-${r.user_id}`}
                        onclick={() => onBan(r.user_id)}
                      >Ban</button>
                      <button
                        class="btn-mod timeout"
                        disabled={actionEnCours === `timeout-${r.user_id}`}
                        onclick={() => onTimeout(r.user_id, 600)}
                      >10min</button>
                      <button
                        class="btn-mod vip"
                        disabled={actionEnCours === `addvip-${r.user_id}`}
                        onclick={() => onAjouterVip(r.user_id)}
                      >+VIP</button>
                      <button
                        class="btn-mod mod"
                        disabled={actionEnCours === `addmod-${r.user_id}`}
                        onclick={() => onAjouterModerateur(r.user_id)}
                      >+Mod</button>
                    </div>
                  </div>
                {/if}
              </div>

              <!-- Liste followers existante (réutilisée de communaute.ts) -->
              <div class="bloc">
                <div class="bloc-titre">Followers ({followersData?.total ?? "…"})</div>
                {#if followersData}
                  <div class="liste-scroll">
                    {#each followersData.liste as f}
                      <div class="viewer-row">
                        <div class="liste-user">
                          {#if f.profile_image_url}
                            <img class="liste-avatar" src={f.profile_image_url} alt={f.display_name || f.login} />
                          {/if}
                          <span class="viewer-login">{f.display_name || f.login}</span>
                        </div>
                        <div class="viewer-actions">
                          <button
                            class="btn-mod ban"
                            disabled={actionEnCours === `ban-${f.user_id}`}
                            onclick={() => onBan(f.user_id)}
                          >Ban</button>
                          <button
                            class="btn-mod timeout"
                            disabled={actionEnCours === `timeout-${f.user_id}`}
                            onclick={() => onTimeout(f.user_id, 600)}
                          >10min</button>
                          <button
                            class="btn-mod vip"
                            disabled={actionEnCours === `addvip-${f.user_id}`}
                            onclick={() => onAjouterVip(f.user_id)}
                          >+VIP</button>
                          <button
                            class="btn-mod mod"
                            disabled={actionEnCours === `addmod-${f.user_id}`}
                            onclick={() => onAjouterModerateur(f.user_id)}
                          >+Mod</button>
                        </div>
                      </div>
                    {/each}
                  </div>
                {:else}
                  <p class="hint">{$t("moderationModal.chargementFollowers")}</p>
                {/if}
              </div>

              <!-- Liste subs existante -->
              {#if subsData && subsData.liste.length > 0}
                <div class="bloc">
                  <div class="bloc-titre">Subs ({subsData.total})</div>
                  <div class="liste-scroll">
                    {#each subsData.liste as s}
                      <div class="viewer-row">
                        <div class="liste-user">
                          {#if s.profile_image_url}
                            <img class="liste-avatar" src={s.profile_image_url} alt={s.display_name || s.login} />
                          {/if}
                          <span class="viewer-login">{s.display_name || s.login}</span>
                        </div>
                        <span class="viewer-tag">
                          {s.tier === "1000" ? "T1" : s.tier === "2000" ? "T2" : s.tier === "3000" ? "T3" : s.tier}
                        </span>
                        <div class="viewer-actions">
                          <button
                            class="btn-mod ban"
                            disabled={actionEnCours === `ban-${s.user_id}`}
                            onclick={() => onBan(s.user_id)}
                          >Ban</button>
                          <button
                            class="btn-mod timeout"
                            disabled={actionEnCours === `timeout-${s.user_id}`}
                            onclick={() => onTimeout(s.user_id, 600)}
                          >10min</button>
                          <button
                            class="btn-mod vip"
                            disabled={actionEnCours === `addvip-${s.user_id}`}
                            onclick={() => onAjouterVip(s.user_id)}
                          >+VIP</button>
                          <button
                            class="btn-mod mod"
                            disabled={actionEnCours === `addmod-${s.user_id}`}
                            onclick={() => onAjouterModerateur(s.user_id)}
                          >+Mod</button>
                        </div>
                      </div>
                    {/each}
                  </div>
                </div>
              {/if}
            {:else if sec === "vips"}
              <div class="bloc">
                <div class="bloc-titre">{$t("moderationModal.vipsActuels")}</div>
                {#if vipsData === null}
                  <p class="hint">{$t("commun.chargement")}</p>
                {:else if vipsData.length === 0}
                  <p class="hint">{$t("moderationModal.aucunVip")}</p>
                {:else}
                  <div class="liste-scroll">
                    {#each vipsData as v}
                      <div class="viewer-row">
                        <div class="liste-user">
                          {#if v.profile_image_url}
                            <img class="liste-avatar" src={v.profile_image_url} alt={v.display_name} />
                          {/if}
                          <span class="viewer-login">{v.display_name}</span>
                        </div>
                        <div class="viewer-actions">
                          <button
                            class="btn-mod retirer"
                            disabled={actionEnCours === `rmvip-${v.user_id}`}
                            onclick={() => onRetirerVip(v.user_id)}
                          >{$t("moderationModal.retirerVip")}</button>
                        </div>
                      </div>
                    {/each}
                  </div>
                {/if}
              </div>
            {:else if sec === "mods"}
              <div class="bloc">
                <div class="bloc-titre">{$t("moderationModal.modsActuels")}</div>
                {#if modsData === null}
                  <p class="hint">{$t("commun.chargement")}</p>
                {:else if modsData.length === 0}
                  <p class="hint">{$t("moderationModal.aucunMod")}</p>
                {:else}
                  <div class="liste-scroll">
                    {#each modsData as m}
                      <div class="viewer-row">
                        <div class="liste-user">
                          {#if m.profile_image_url}
                            <img class="liste-avatar" src={m.profile_image_url} alt={m.display_name} />
                          {/if}
                          <span class="viewer-login">{m.display_name}</span>
                        </div>
                        <div class="viewer-actions">
                          <button
                            class="btn-mod retirer"
                            disabled={actionEnCours === `rmmod-${m.user_id}`}
                            onclick={() => onRetirerMod(m.user_id)}
                          >{$t("moderationModal.retirerMod")}</button>
                        </div>
                      </div>
                    {/each}
                  </div>
                {/if}
              </div>
            {:else if sec === "bannis"}
              <div class="bloc">
                <div class="bloc-titre">{$t("moderationModal.bannisTimeouts")}</div>
                {#if bannisData === null}
                  <p class="hint">{$t("commun.chargement")}</p>
                {:else if bannisData.length === 0}
                  <p class="hint">{$t("moderationModal.aucunBan")}</p>
                {:else}
                  <div class="liste-scroll">
                    {#each bannisData as b}
                      <div class="viewer-row">
                        <div class="viewer-info">
                          <div class="liste-user">
                            {#if b.profile_image_url}
                              <img class="liste-avatar" src={b.profile_image_url} alt={b.display_name || b.login} />
                            {/if}
                            <span class="viewer-login">{b.display_name || b.login}</span>
                          </div>
                          <span class="viewer-tag" class:timeout={estTimeout(b)}>
                            {estTimeout(b) ? get(t)("moderationModal.timeoutTag", { duree: dureeTimeout(b) }) : "Ban"}
                          </span>
                          {#if b.reason}
                            <span class="viewer-reason">{b.reason}</span>
                          {/if}
                        </div>
                        <div class="viewer-actions">
                          <button
                            class="btn-mod debannir"
                            disabled={actionEnCours === `unban-${b.user_id}`}
                            onclick={() => onUnban(b.user_id)}
                          >{$t("moderationModal.debannir")}</button>
                        </div>
                      </div>
                    {/each}
                  </div>
                {/if}
              </div>
            {:else if sec === "messages"}
              <div class="bloc">
                <div class="bloc-titre">{$t("moderationModal.messagesRecents")}</div>
                <p class="hint">{$t("moderationModal.messagesHint")}</p>
                {#if messagesTwitch.length === 0}
                  <p class="hint">{$t("moderationModal.aucunMessage")}</p>
                {:else}
                  <div class="liste-scroll">
                    {#each messagesTwitch as m}
                      <div class="msg-row">
                        <span class="msg-pseudo">{m.pseudo}</span>
                        <span class="msg-texte">{m.texte}</span>
                        <button
                          class="btn-mod suppr-msg"
                          disabled={actionEnCours === `delmsg-${m.message_id}`}
                          onclick={() => onSupprimerMessage(m.message_id!)}
                          title={$t("moderationModal.supprimerMessageTitle")}
                        >🗑</button>
                      </div>
                    {/each}
                  </div>
                {/if}
              </div>
            {:else if sec === "unfollows"}
              <div class="bloc">
                <div class="bloc-titre">{$t("moderationModal.unfollowsDetectes", { n: unfollowsData?.length ?? "…" })}</div>
                <p class="hint">
                  {$t("moderationModal.unfollowsHint")}
                </p>
                {#if unfollowsData === null}
                  <p class="hint">{$t("commun.chargement")}</p>
                {:else if unfollowsData.length === 0}
                  <p class="hint">{$t("moderationModal.aucunUnfollow")}</p>
                {:else}
                  <div class="liste-scroll">
                    {#each unfollowsData as u}
                      <div class="liste-entry">
                        <div class="liste-user">
                          {#if u.profile_image_url}
                            <img class="liste-avatar" src={u.profile_image_url} alt={u.display_name || u.login} />
                          {/if}
                          <span class="liste-login">{u.display_name || u.login || u.user_id}</span>
                        </div>
                        <span class="liste-date">{u.date_unfollow.slice(0, 10)}</span>
                      </div>
                    {/each}
                  </div>
                {/if}
              </div>
            {/if}
          </div>
        </div>

        <!-- Barre d'actions globales -->
        <div class="mod-actions-globales">
          <button class="btn-mod danger" onclick={() => void onClearChat()} disabled={actionEnCours === "clear"}>
            {$t("moderationModal.viderChat")}
          </button>
          <button class="action btn-mod" onclick={() => void chargerToutMod()}>
            {$t("moderationModal.rafraichirListes")}
          </button>
        </div>
      </div>
    {/if}
  {:else if tab === "youtube"}
    <div class="tab-body">
      {#if !cx.youtube}
        <p class="hint-centre">{$t("moderationModal.connecterYoutube")}</p>
      {:else}
        <!-- Channel info -->
        {#if ytChannel}
          <div class="bloc">
            <div class="bloc-titre">{$t("moderationModal.chaine")}</div>
            <div class="broadcaster">
              {#if ytChannel.profile_image_url}
                <img
                  class="avatar"
                  src={ytChannel.profile_image_url}
                  alt={ytChannel.display_name}
                />
              {/if}
              <div class="bc-info">
                <span class="bc-name">{ytChannel.display_name}</span>
                <span class="bc-type">{$t("moderationModal.abonnes", { n: ytChannel.subscriber_count })}</span>
              </div>
            </div>
          </div>
        {/if}

        <!-- Viewers live -->
        <div class="bloc">
          <div class="bloc-titre">{$t("moderationModal.viewersLive")}</div>
          <div class="communaute-row">
            <span class="field-label">{$t("moderationModal.enDirect")}</span>
            <span class="communaute-val">
              {#if ytViewers !== null && ytViewers !== undefined}
                {ytViewers}
              {:else}
                {$t("moderationModal.horsLigne")}
              {/if}
            </span>
          </div>
        </div>

        <!-- Stats -->
        <div class="bloc">
          <div class="bloc-titre">{$t("moderationModal.statistiques")}</div>
          <div class="communaute-row">
            <span class="field-label">{$t("moderationModal.vuesTotales")}</span>
            <span class="communaute-val">{ytChannel?.view_count ?? "…"}</span>
          </div>
          <div class="communaute-row">
            <span class="field-label">{$t("moderationModal.videos")}</span>
            <span class="communaute-val">{ytChannel?.video_count ?? "…"}</span>
          </div>
        </div>

        <!-- Members -->
        <div class="bloc">
          <div class="bloc-titre">{$t("moderationModal.membersTitre", { n: ytMembers?.total ?? "…" })}</div>
          {#if ytMembers && ytMembers.liste.length > 0}
            <div class="liste-scroll">
              {#each ytMembers.liste as m}
                <div class="liste-entry">
                  <span class="liste-login">{m.display_name}</span>
                  <span class="liste-tier">{m.memberships_level}</span>
                </div>
              {/each}
            </div>
          {:else}
            <p class="hint">
              {$t("moderationModal.membresIndisponibles")}
            </p>
          {/if}
        </div>

        <button class="action btn-mod" onclick={() => void chargerCommunauteYoutube()}>
          {$t("moderationModal.rafraichirYoutube")}
        </button>
      {/if}
    </div>
  {:else}
    <div class="tab-body">
      <p class="hint-centre">{$t("moderationModal.bientot")}</p>
    </div>
  {/if}
</Modal>

<style>
  /* Tabs — mêmes patterns que InteractionViewerModal */
  .tabs {
    display: flex;
    flex-shrink: 0;
    gap: 0.2rem;
    border-bottom: 1px solid var(--bordure-ui);
    padding: 0 1rem;
  }
  .tab {
    background: var(--btn-surface);
    box-shadow: var(--btn-inset);
    color: var(--texte);
    border: none;
    border-bottom: 2px solid transparent;
    padding: 0.4rem 0.8rem;
    cursor: pointer;
    opacity: 0.6;
    transition: background 0.15s ease, box-shadow 0.15s ease, opacity 0.15s ease;
  }
  .tab:hover {
    background: var(--btn-surface-hover);
    box-shadow: var(--btn-inset-hover);
    opacity: 0.8;
  }
  .tab.active {
    opacity: 1;
    border-bottom-color: var(--accent-ui);
  }
  .tab:disabled {
    opacity: 0.3;
    cursor: not-allowed;
  }

  /* Corps */
  .tab-body {
    flex: 1;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: 0.8rem;
    padding: 1rem;
    max-width: 760px;
    width: 100%;
    margin: 0 auto;
  }

  /* Layout master-detail */
  .mod-layout {
    display: grid;
    grid-template-columns: 170px 1fr;
    gap: 0.5rem;
    align-items: start;
  }
  .mod-sections {
    display: flex;
    flex-direction: column;
    gap: 0.25rem;
  }
  .mod-select {
    flex: 1;
    display: flex;
    align-items: center;
    gap: 0.45rem;
    min-width: 0;
    background: var(--btn-surface);
    box-shadow: var(--btn-inset);
    color: var(--texte);
    border: 1px solid var(--bordure-ui);
    border-radius: var(--rayon-petit);
    padding: 0.3rem 0.45rem;
    font: inherit;
    font-size: 0.85rem;
    cursor: pointer;
    text-align: left;
    transition: background 0.15s ease, box-shadow 0.15s ease, border-color 0.15s ease;
  }
  .mod-select:hover {
    background: var(--btn-surface-hover);
    box-shadow: var(--btn-inset-hover);
    border-color: var(--accent-ui);
  }
  .mod-select.selected {
    background:
      linear-gradient(
        180deg,
        color-mix(in srgb, var(--accent-ui) 25%, var(--fond-controle)) 0%,
        color-mix(in srgb, var(--accent-ui) 12%, var(--fond-controle)) 100%
      );
    color: var(--texte);
    border-color: var(--accent-ui);
  }
  .mod-detail {
    display: flex;
    flex-direction: column;
    gap: 0.5rem;
    min-width: 0;
  }

  /* Bloc = carte visuelle */
  .bloc {
    display: flex;
    flex-direction: column;
    gap: 0.5rem;
    border: 1px solid var(--bordure-ui);
    border-radius: var(--rayon);
    padding: 0.6rem 0.9rem;
    background: var(--fond-panneau);
  }
  .bloc-titre {
    font-size: 0.85rem;
    font-weight: 600;
    opacity: 0.85;
    border-bottom: 1px solid var(--bordure-ui);
    padding-bottom: 0.2rem;
  }

  /* Recherche */
  .recherche-row {
    display: flex;
    gap: 0.4rem;
  }
  .recherche-row input {
    flex: 1;
  }
  .mod-detail input[type="text"],
  .mod-detail input[type="number"] {
    background: var(--fond-controle);
    color: var(--texte);
    border: 1px solid var(--bordure-ui);
    padding: 0.3rem 0.45rem;
    font: inherit;
    font-size: 0.85rem;
  }
  .mod-detail input[type="text"]:focus,
  .mod-detail input[type="number"]:focus {
    outline: none;
    border-color: var(--accent-ui);
  }
  .hint-erreur {
    font-size: 0.8rem;
    color: var(--message-error-color);
    margin: 0;
  }

  /* Ligne viewer */
  .viewer-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 0.5rem;
    padding: 0.25rem 0;
    border-bottom: 1px solid var(--bordure-ui);
  }
  .viewer-row:last-child {
    border-bottom: none;
  }
  .viewer-login {
    font-size: 0.85rem;
    font-weight: 500;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .viewer-info {
    display: flex;
    align-items: center;
    gap: 0.4rem;
    min-width: 0;
    flex: 1;
  }
  .viewer-tag {
    font-size: 0.72rem;
    padding: 0.1rem 0.35rem;
    border-radius: var(--rayon-petit);
    background: var(--gris-fonce);
    color: var(--texte);
    white-space: nowrap;
  }
  .viewer-tag.timeout {
    background: color-mix(in srgb, var(--message-user-action-color) 25%, var(--gris-fonce));
  }
  .viewer-reason {
    font-size: 0.72rem;
    opacity: 0.6;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .viewer-actions {
    display: flex;
    gap: 0.2rem;
    flex-shrink: 0;
  }

  /* Boutons d'action */
  .btn-mod {
    background: var(--btn-surface);
    box-shadow: var(--btn-inset);
    color: var(--texte);
    border: 1px solid var(--bordure-ui);
    border-radius: var(--rayon-petit);
    padding: 0.15rem 0.4rem;
    font: inherit;
    font-size: 0.72rem;
    cursor: pointer;
    white-space: nowrap;
    transition: background 0.15s ease, box-shadow 0.15s ease, border-color 0.15s ease;
  }
  .btn-mod:hover:not(:disabled) {
    background: var(--btn-surface-hover);
    box-shadow: var(--btn-inset-hover);
    border-color: var(--accent-ui);
  }
  .btn-mod:disabled {
    opacity: 0.4;
    cursor: not-allowed;
  }
  .btn-mod.ban {
    border-color: color-mix(in srgb, var(--message-error-color) 40%, var(--bordure-ui));
    color: var(--message-error-color);
  }
  .btn-mod.danger {
    border-color: color-mix(in srgb, var(--message-error-color) 40%, var(--bordure-ui));
    color: var(--message-error-color);
  }
  .btn-mod.timeout {
    border-color: color-mix(in srgb, var(--message-user-action-color) 40%, var(--bordure-ui));
    color: var(--message-user-action-color);
  }
  .btn-mod.vip {
    border-color: color-mix(in srgb, var(--accent-magenta) 40%, var(--bordure-ui));
    color: var(--accent-magenta);
  }
  .btn-mod.mod {
    border-color: color-mix(in srgb, var(--message-ok-color) 40%, var(--bordure-ui));
    color: var(--message-ok-color);
  }
  .btn-mod.retirer {
    border-color: color-mix(in srgb, var(--message-user-action-color) 40%, var(--bordure-ui));
    color: var(--message-user-action-color);
  }
  .btn-mod.debannir {
    border-color: color-mix(in srgb, var(--message-ok-color) 40%, var(--bordure-ui));
    color: var(--message-ok-color);
  }
  .btn-mod.suppr-msg {
    border-color: color-mix(in srgb, var(--message-error-color) 40%, var(--bordure-ui));
    color: var(--message-error-color);
    padding: 0.15rem 0.3rem;
  }

  /* Liste scrollable */
  .liste-scroll {
    max-height: 300px;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
  }

  /* Messages */
  .msg-row {
    display: flex;
    align-items: center;
    gap: 0.4rem;
    padding: 0.2rem 0;
    border-bottom: 1px solid var(--bordure-ui);
  }
  .msg-row:last-child {
    border-bottom: none;
  }
  .msg-pseudo {
    font-size: 0.8rem;
    font-weight: 600;
    flex-shrink: 0;
    max-width: 120px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .msg-texte {
    font-size: 0.8rem;
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    opacity: 0.85;
  }

  /* Barre d'actions globales */
  .mod-actions-globales {
    display: flex;
    gap: 0.4rem;
    justify-content: flex-end;
    padding-top: 0.4rem;
    border-top: 1px solid var(--bordure-ui);
  }

  /* Hints */
  .hint {
    font-size: 0.8rem;
    opacity: 0.6;
    margin: 0;
  }
  .hint-centre {
    font-size: 0.9rem;
    opacity: 0.6;
    text-align: center;
    margin: 2rem 0;
  }

  /* Need reauth */
  .need-reauth {
    display: flex;
    flex-direction: column;
    gap: 0.5rem;
    align-items: center;
    padding: 1rem;
  }
  .need-reauth span {
    font-size: 0.85rem;
    opacity: 0.8;
  }

  /* Scrollbar (même style que InteractionViewerModal) */
  .tab-body::-webkit-scrollbar,
  .liste-scroll::-webkit-scrollbar {
    width: 6px;
  }
  .tab-body::-webkit-scrollbar-track,
  .liste-scroll::-webkit-scrollbar-track {
    background: transparent;
  }
  .tab-body::-webkit-scrollbar-thumb,
  .liste-scroll::-webkit-scrollbar-thumb {
    background: var(--bordure-ui);
    border-radius: 3px;
  }
  .tab-body::-webkit-scrollbar-thumb:hover,
  .liste-scroll::-webkit-scrollbar-thumb:hover {
    background: var(--accent-ui);
  }

  /* ===== Section Communauté (lecture seule) ===== */
  .broadcaster {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    padding: 0.3rem 0;
  }
  .avatar {
    width: 2.2rem;
    height: 2.2rem;
    border-radius: 50%;
    border: 1px solid var(--bordure-ui);
    flex-shrink: 0;
  }
  .bc-info {
    display: flex;
    flex-direction: column;
    gap: 0.05rem;
  }
  .bc-name {
    font-weight: 600;
    font-size: 0.9rem;
  }
  .bc-type {
    font-size: 0.7rem;
    opacity: 0.6;
  }
  .communaute-row {
    display: flex;
    justify-content: space-between;
    align-items: baseline;
    padding: 0.2rem 0;
  }
  .field-label {
    font-size: 0.85rem;
  }
  .communaute-val {
    font-size: 0.85rem;
    font-variant-numeric: tabular-nums;
  }
  .liste-entry {
    display: flex;
    justify-content: space-between;
    padding: 0.1rem 0;
    border-bottom: 1px solid var(--bordure-ui);
  }
  .liste-entry:last-child {
    border-bottom: none;
  }
  .liste-login {
    font-weight: 500;
    font-size: 0.85rem;
  }
  .liste-user {
    display: flex;
    align-items: center;
    gap: 0.35rem;
    min-width: 0;
  }
  .liste-avatar {
    width: 1.4rem;
    height: 1.4rem;
    border-radius: 50%;
    border: 1px solid var(--bordure-ui);
    flex-shrink: 0;
  }
  .liste-date,
  .liste-tier {
    opacity: 0.6;
    font-size: 0.78rem;
  }

  /* ===== Section « En direct » ===== */
  /* Toggle — même pattern que InteractionViewerModal */
  .toggle {
    display: flex;
    align-items: center;
    gap: 0.3rem;
    cursor: pointer;
  }
  .toggle input[type="checkbox"] {
    accent-color: var(--accent-ui);
  }
  .mode-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 0.5rem;
    flex-wrap: wrap;
  }
  .mode-delai {
    display: flex;
    align-items: center;
    gap: 0.3rem;
  }
  .mode-delai input {
    width: 4.5rem;
  }
  .hint-inline {
    font-size: 0.72rem;
    opacity: 0.6;
  }
  .chip-row {
    display: flex;
    gap: 0.3rem;
    flex-wrap: wrap;
  }
  .chip {
    background: var(--btn-surface);
    box-shadow: var(--btn-inset);
    color: var(--texte);
    border: 1px solid var(--bordure-ui);
    border-radius: var(--rayon-petit);
    padding: 0.2rem 0.55rem;
    font: inherit;
    font-size: 0.8rem;
    cursor: pointer;
    transition: background 0.15s ease, box-shadow 0.15s ease, border-color 0.15s ease;
  }
  .chip:hover:not(:disabled) {
    background: var(--btn-surface-hover);
    box-shadow: var(--btn-inset-hover);
    border-color: var(--accent-ui);
  }
  .chip:disabled {
    opacity: 0.4;
    cursor: not-allowed;
  }
  /* État actif des chips (couleur d'annonce choisie) — même recette que
     .btn-mod.timeout : teinte --message-user-action-color, jamais un nom
     de couleur en dur (la valeur Helix envoyée ne change pas). */
  .chip.actif {
    border-color: color-mix(in srgb, var(--message-user-action-color) 40%, var(--bordure-ui));
    color: var(--message-user-action-color);
  }
  .sugg-liste {
    display: flex;
    flex-direction: column;
    border: 1px solid var(--bordure-ui);
    border-radius: var(--rayon-petit);
    overflow: hidden;
  }
  .sugg-item {
    display: flex;
    align-items: center;
    gap: 0.45rem;
    background: var(--fond-controle);
    color: var(--texte);
    border: none;
    border-bottom: 1px solid var(--bordure-ui);
    padding: 0.3rem 0.45rem;
    font: inherit;
    font-size: 0.82rem;
    cursor: pointer;
    text-align: left;
  }
  .sugg-item:last-child {
    border-bottom: none;
  }
  .sugg-item:hover {
    background: var(--btn-surface-hover);
  }
  .sugg-img {
    width: 1.6rem;
    height: 2.1rem;
    object-fit: cover;
    border-radius: 2px;
  }
  .sugg-img.sugg-avatar {
    height: 1.6rem;
    border-radius: 50%;
  }
  /* Aperçu de la jaquette du jeu choisi, à côté du champ Catégorie. */
  .jacquette {
    width: 52px;
    height: 72px;
    object-fit: cover;
    border: 1px solid var(--bordure-ui);
    border-radius: var(--rayon-petit);
    flex-shrink: 0;
  }
  .confirm-bloc {
    display: flex;
    flex-direction: column;
    gap: 0.4rem;
    border: 1px solid var(--message-user-action-color);
    border-radius: var(--rayon-petit);
    padding: 0.45rem 0.6rem;
  }
  .confirm-texte {
    font-size: 0.8rem;
    margin: 0;
    color: var(--message-user-action-color);
  }
  .confirm-actions {
    display: flex;
    gap: 0.4rem;
    justify-content: flex-end;
  }
  .live-msg {
    font-size: 0.8rem;
    margin: 0;
    color: var(--message-error-color);
  }
  .live-msg.ok {
    color: var(--message-ok-color);
  }
</style>
