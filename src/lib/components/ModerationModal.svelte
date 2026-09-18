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
  import { reconnecterTwitch } from "../stores/chat";
  import Modal from "./Modal.svelte";

  let { onFermer = () => {} }: { onFermer?: () => void } = $props();

  // Onglet actif (Twitch et YouTube sont fonctionnels).
  let tab = $state<"twitch" | "kick" | "youtube" | "tiktok" | "trovo">("twitch");

  // Section sélectionnée dans l'onglet Twitch (master-detail).
  // « communaute » = supervision lecture seule (broadcaster, viewers live,
  // followers, subs). Les autres sections = action (ban/vip/mod/messages).
  let sec = $state<"communaute" | "viewers" | "vips" | "mods" | "bannis" | "messages" | "unfollows">("communaute");

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
        rechercheErreur = "Utilisateur introuvable";
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
      bannir(userId, `Banni via StreamOS`, undefined)
    );
  }

  /// Timeout un viewer (durée en secondes).
  function onTimeout(userId: string, duree: number): void {
    void executerAction(`timeout-${userId}`, () =>
      bannir(userId, `Timeout ${duree}s via StreamOS`, duree)
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
    if (!b.expires_at) return "Permanent";
    try {
      const reste = new Date(b.expires_at).getTime() - Date.now();
      if (reste <= 0) return "Expiré";
      const min = Math.floor(reste / 60000);
      if (min < 60) return `${min}min`;
      const h = Math.floor(min / 60);
      if (h < 24) return `${h}h${min % 60}min`;
      const j = Math.floor(h / 24);
      return `${j}j${h % 24}h`;
    } catch {
      return "?";
    }
  }
</script>

<Modal title="Modération & Rôles" onClose={onFermer} maxWidth="900px">
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
        <p class="hint-centre">Connecter Twitch pour modérer.</p>
      </div>
    {:else if modErr === "need_reauth" || commErr === "need_reauth"}
      <div class="tab-body">
        <div class="need-reauth">
          <span>Scopes manquants pour la modération.</span>
          <button class="action" onclick={onReconnecter}>
            Reconnecter Twitch pour la modération
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
              <span class="mod-nom">Communauté</span>
            </button>
            <button
              class="mod-select"
              class:selected={sec === "viewers"}
              onclick={() => (sec = "viewers")}
            >
              <span class="mod-nom">Viewers</span>
            </button>
            <button
              class="mod-select"
              class:selected={sec === "vips"}
              onclick={() => (sec = "vips")}
            >
              <span class="mod-nom">VIPs ({vipsData?.length ?? "…"})</span>
            </button>
            <button
              class="mod-select"
              class:selected={sec === "mods"}
              onclick={() => (sec = "mods")}
            >
              <span class="mod-nom">Modérateurs ({modsData?.length ?? "…"})</span>
            </button>
            <button
              class="mod-select"
              class:selected={sec === "bannis"}
              onclick={() => (sec = "bannis")}
            >
              <span class="mod-nom">Bannis ({bannisData?.length ?? "…"})</span>
            </button>
            <button
              class="mod-select"
              class:selected={sec === "messages"}
              onclick={() => (sec = "messages")}
            >
              <span class="mod-nom">Messages</span>
            </button>
            <button
              class="mod-select"
              class:selected={sec === "unfollows"}
              onclick={() => (sec = "unfollows")}
            >
              <span class="mod-nom">Unfollows ({unfollowsData?.length ?? "…"})</span>
            </button>
          </div>

          <!-- Panneau droit : contenu de la section -->
          <div class="mod-detail">
            {#if sec === "communaute"}
              <!-- Broadcaster -->
              {#if broadcasterData}
                <div class="bloc">
                  <div class="bloc-titre">Chaîne</div>
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
                      <span class="bc-type">{broadcasterData.broadcaster_type || "aucun"}</span>
                    </div>
                  </div>
                </div>
              {/if}

              <!-- Viewers live -->
              <div class="bloc">
                <div class="bloc-titre">Viewers live</div>
                <div class="communaute-row">
                  <span class="field-label">En direct</span>
                  <span class="communaute-val">
                    {#if viewersData !== null}
                      {viewersData}
                    {:else}
                      Hors-ligne
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
                  <p class="hint">Chargement des followers…</p>
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
                  <p class="hint">Aucun sub.</p>
                {/if}
              </div>

              <button class="action" onclick={() => void onRafraichirCommunaute()}>
                Rafraîchir la communauté
              </button>
            {:else if sec === "viewers"}
              <!-- Recherche par pseudo -->
              <div class="bloc">
                <div class="bloc-titre">Rechercher un viewer</div>
                <div class="recherche-row">
                  <input
                    type="text"
                    placeholder="Pseudo Twitch…"
                    bind:value={recherche}
                    onkeydown={(e) => { if (e.key === "Enter") void onRechercher(); }}
                  />
                  <button class="action" onclick={() => void onRechercher()} disabled={rechercheEnCours}>
                    {rechercheEnCours ? "…" : "Rechercher"}
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
                  <p class="hint">Chargement des followers…</p>
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
                <div class="bloc-titre">VIPs actuels</div>
                {#if vipsData === null}
                  <p class="hint">Chargement…</p>
                {:else if vipsData.length === 0}
                  <p class="hint">Aucun VIP.</p>
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
                          >Retirer VIP</button>
                        </div>
                      </div>
                    {/each}
                  </div>
                {/if}
              </div>
            {:else if sec === "mods"}
              <div class="bloc">
                <div class="bloc-titre">Modérateurs actuels</div>
                {#if modsData === null}
                  <p class="hint">Chargement…</p>
                {:else if modsData.length === 0}
                  <p class="hint">Aucun modérateur.</p>
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
                          >Retirer Mod</button>
                        </div>
                      </div>
                    {/each}
                  </div>
                {/if}
              </div>
            {:else if sec === "bannis"}
              <div class="bloc">
                <div class="bloc-titre">Bannis & Timeouts</div>
                {#if bannisData === null}
                  <p class="hint">Chargement…</p>
                {:else if bannisData.length === 0}
                  <p class="hint">Aucun bannissement actif.</p>
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
                            {estTimeout(b) ? `Timeout ${dureeTimeout(b)}` : "Ban"}
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
                          >Débannir</button>
                        </div>
                      </div>
                    {/each}
                  </div>
                {/if}
              </div>
            {:else if sec === "messages"}
              <div class="bloc">
                <div class="bloc-titre">Messages récents (suppression)</div>
                <p class="hint">Cliquez sur 🗑 pour supprimer un message du chat Twitch.</p>
                {#if messagesTwitch.length === 0}
                  <p class="hint">Aucun message Twitch récent avec ID.</p>
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
                          title="Supprimer ce message"
                        >🗑</button>
                      </div>
                    {/each}
                  </div>
                {/if}
              </div>
            {:else if sec === "unfollows"}
              <div class="bloc">
                <div class="bloc-titre">Unfollows détectés ({unfollowsData?.length ?? "…"})</div>
                <p class="hint">
                  Comparaison automatique au démarrage de StreamOS : si un
                  follower présent au précédent démarrage n'est plus dans la
                  liste, il est enregistré ici avec la date de détection.
                </p>
                {#if unfollowsData === null}
                  <p class="hint">Chargement…</p>
                {:else if unfollowsData.length === 0}
                  <p class="hint">Aucun unfollow détecté pour le moment.</p>
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
          <button class="action" onclick={() => void onClearChat()} disabled={actionEnCours === "clear"}>
            Vider le chat
          </button>
          <button class="action" onclick={() => void chargerToutMod()}>
            Rafraîchir les listes
          </button>
        </div>
      </div>
    {/if}
  {:else if tab === "youtube"}
    <div class="tab-body">
      {#if !cx.youtube}
        <p class="hint-centre">Connecter YouTube pour voir la communauté.</p>
      {:else}
        <!-- Channel info -->
        {#if ytChannel}
          <div class="bloc">
            <div class="bloc-titre">Chaîne</div>
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
                <span class="bc-type">{ytChannel.subscriber_count} abonnés</span>
              </div>
            </div>
          </div>
        {/if}

        <!-- Viewers live -->
        <div class="bloc">
          <div class="bloc-titre">Viewers live</div>
          <div class="communaute-row">
            <span class="field-label">En direct</span>
            <span class="communaute-val">
              {#if ytViewers !== null && ytViewers !== undefined}
                {ytViewers}
              {:else}
                Hors-ligne
              {/if}
            </span>
          </div>
        </div>

        <!-- Stats -->
        <div class="bloc">
          <div class="bloc-titre">Statistiques</div>
          <div class="communaute-row">
            <span class="field-label">Vues totales</span>
            <span class="communaute-val">{ytChannel?.view_count ?? "…"}</span>
          </div>
          <div class="communaute-row">
            <span class="field-label">Vidéos</span>
            <span class="communaute-val">{ytChannel?.video_count ?? "…"}</span>
          </div>
        </div>

        <!-- Members -->
        <div class="bloc">
          <div class="bloc-titre">Members ({ytMembers?.total ?? "…"})</div>
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
              Aucun member visible (l'endpoint members retourne 403 avec le
              Device Flow — scope channel-memberships.creator requis).
            </p>
          {/if}
        </div>

        <button class="action" onclick={() => void chargerCommunauteYoutube()}>
          Rafraîchir la communauté YouTube
        </button>
      {/if}
    </div>
  {:else}
    <div class="tab-body">
      <p class="hint-centre">Bientôt disponible</p>
    </div>
  {/if}
</Modal>

<style>
  /* Tabs — mêmes patterns que InteractionViewerModal */
  .tabs {
    display: flex;
    flex-shrink: 0;
    gap: 0.2rem;
    border-bottom: 1px solid var(--bordure);
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
    border-bottom-color: var(--accent-violet);
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
    border: 1px solid var(--bordure);
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
    border-color: var(--accent-violet);
  }
  .mod-select.selected {
    background:
      linear-gradient(
        180deg,
        color-mix(in srgb, var(--accent-violet) 25%, var(--fond-controle)) 0%,
        color-mix(in srgb, var(--accent-violet) 12%, var(--fond-controle)) 100%
      );
    color: var(--texte);
    border-color: var(--accent-violet);
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
    border: 1px solid var(--bordure);
    border-radius: var(--rayon);
    padding: 0.6rem 0.9rem;
    background: var(--fond-panneau);
  }
  .bloc-titre {
    font-size: 0.85rem;
    font-weight: 600;
    opacity: 0.85;
    border-bottom: 1px solid var(--bordure);
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
    border-bottom: 1px solid var(--bordure);
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
    border: 1px solid var(--bordure);
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
    border-color: var(--accent-violet);
  }
  .btn-mod:disabled {
    opacity: 0.4;
    cursor: not-allowed;
  }
  .btn-mod.ban {
    border-color: color-mix(in srgb, var(--message-error-color) 40%, var(--bordure));
    color: var(--message-error-color);
  }
  .btn-mod.timeout {
    border-color: color-mix(in srgb, var(--message-user-action-color) 40%, var(--bordure));
    color: var(--message-user-action-color);
  }
  .btn-mod.vip {
    border-color: color-mix(in srgb, var(--accent-magenta) 40%, var(--bordure));
    color: var(--accent-magenta);
  }
  .btn-mod.mod {
    border-color: color-mix(in srgb, var(--message-ok-color) 40%, var(--bordure));
    color: var(--message-ok-color);
  }
  .btn-mod.retirer {
    border-color: color-mix(in srgb, var(--message-user-action-color) 40%, var(--bordure));
    color: var(--message-user-action-color);
  }
  .btn-mod.debannir {
    border-color: color-mix(in srgb, var(--message-ok-color) 40%, var(--bordure));
    color: var(--message-ok-color);
  }
  .btn-mod.suppr-msg {
    border-color: color-mix(in srgb, var(--message-error-color) 40%, var(--bordure));
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
    border-bottom: 1px solid var(--bordure);
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
    border-top: 1px solid var(--bordure);
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
    background: var(--bordure);
    border-radius: 3px;
  }
  .tab-body::-webkit-scrollbar-thumb:hover,
  .liste-scroll::-webkit-scrollbar-thumb:hover {
    background: var(--accent-violet);
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
    border: 1px solid var(--bordure);
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
    border-bottom: 1px solid var(--bordure);
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
    border: 1px solid var(--bordure);
    flex-shrink: 0;
  }
  .liste-date,
  .liste-tier {
    opacity: 0.6;
    font-size: 0.78rem;
  }
</style>
