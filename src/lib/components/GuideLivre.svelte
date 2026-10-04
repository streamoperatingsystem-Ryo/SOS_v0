<script lang="ts">
  // Guide du streamer — overlay plein écran « livre feuilletable »
  // (référence : passeport Tomb Raider — double page ouverte, feuilles
  // qui basculent en 3D autour de la reliure).
  //
  // Modèle : N feuilles physiques, chacune = une <div> 3D pivotant sur son
  // bord gauche (la reliure). Feuille i : recto = PAGES[2i], verso =
  // PAGES[2i+1]. État `tournees` = nombre de feuilles tournées (0..N).
  // Le SVG ne sert qu'au décor (ornements) — les transforms 3D ne marchent
  // que sur du HTML (SVG ≠ 3D CSS).
  //
  // Navigation : zones de survol sur les tranches gauche/droite (flèche au
  // hover + soulèvement du coin ~7°), flèches clavier ←/→, folios en bas
  // de page. La position de lecture est persistée dans localStorage
  // (reprise là où l'utilisateur s'était arrêté).
  //
  // Dashboard uniquement — jamais monté en diffusion.
  import { onDestroy } from "svelte";
  import { PAGES, type PageGuide } from "../guide/contenu";
  import { guideLivreOpen } from "../stores/ui";
  import { t, langue } from "../stores/langue";
  // Illustration de la page de garde gauche (visible à l'ouverture du
  // livre) — asset bundlé Vite, même pattern que logoSOS (App.svelte).
  import gardeGaucheImg from "../../assets/guide_utilisateurs/FirstLeftPageGuideUSer.png";

  /// Clé localStorage de reprise de lecture (nombre de feuilles tournées).
  const CLE_STORAGE = "sos-guide-page";
  /// Durée du flip + marge — verrou anti double-flip pendant l'animation.
  const DUREE_FLIP_MS = 680;
  /// Nombre de feuilles physiques (2 faces chacune ; le dernier verso peut
  /// être une page de garde décorative si PAGES est impair).
  const N = Math.ceil(PAGES.length / 2);
  const feuilles = Array.from({ length: N }, (_, i) => i);

  /// Lit la position sauvegardée, bornée à [0, N]. Toute erreur (JSON,
  /// storage inaccessible) → 0 = début du livre.
  function lirePageSauvee(): number {
    try {
      const v = Number(localStorage.getItem(CLE_STORAGE));
      return Number.isFinite(v) ? Math.min(Math.max(Math.trunc(v), 0), N) : 0;
    } catch {
      return 0;
    }
  }

  /// Polices des titres par locale : Caveat ne couvre que le latin —
  /// cyrillique (ru) et CJK (zh/ja) basculent sur une police système
  /// basique du pays. La signature « Ryo Shinkawa » et les marqueurs
  /// d'étapes (chiffres latins) restent en Caveat quelle que soit la langue.
  const FONTES_TITRES: Record<string, string> = {
    zh: '"Microsoft YaHei", "PingFang SC", sans-serif',
    ja: '"Yu Gothic", "Meiryo", sans-serif',
    ru: '"Segoe UI", system-ui, sans-serif',
  };
  let fonteTitres = $derived(FONTES_TITRES[$langue] ?? '"Caveat", cursive');

  /// Feuilles tournées — 0 = livre ouvert sur la couverture (droite) avec
  /// la page de garde à gauche ; N = 4ᵉ de couverture à droite.
  let tournees = $state(lirePageSauvee());
  /// Verrou anti double-flip pendant l'animation (~680 ms).
  let enTransition = $state(false);
  /// Feuille actuellement en rotation — reçoit will-change uniquement elle.
  let feuilleEnMouvement = $state<number | null>(null);
  /// Survol des zones tranche → soulèvement du coin de la feuille pincée.
  let survolGauche = $state(false);
  let survolDroite = $state(false);
  /// Clic-overlay façon Modal : fermer seulement si début ET fin du clic
  /// sur l'overlay (pas après un drag / une sélection de texte).
  let downSurOverlay = false;
  let timer: ReturnType<typeof setTimeout> | null = null;

  function tourner(dir: 1 | -1): void {
    if (enTransition) return;
    const nv = tournees + dir;
    if (nv < 0 || nv > N) return;
    enTransition = true;
    // La feuille qui bouge : en avant = celle qu'on tourne (nv-1), en
    // arrière = celle qu'on détourne (nv).
    feuilleEnMouvement = dir === 1 ? nv - 1 : nv;
    tournees = nv;
    try {
      localStorage.setItem(CLE_STORAGE, String(nv));
    } catch {
      // Storage inaccessible : la reprise est un confort, jamais fatal.
    }
    if (timer) clearTimeout(timer);
    timer = setTimeout(() => {
      enTransition = false;
      feuilleEnMouvement = null;
    }, DUREE_FLIP_MS);
  }

  function fermer(): void {
    guideLivreOpen.set(false);
  }

  /// Découpe un texte i18n sur les marqueurs **…** → segments texte/fort.
  /// Sert à surligner les termes-clés (clic gauche, nom de bouton…) sans
  /// {@html} : Svelte échappe toujours chaque segment rendu.
  function segments(cle: string): { texte: string; fort: boolean }[] {
    return $t(cle)
      .split("**")
      .map((t, i) => ({ texte: t, fort: i % 2 === 1 }));
  }

  function onOverlayPointerDown(e: PointerEvent) {
    downSurOverlay = e.target === e.currentTarget;
  }

  function onOverlayPointerUp(e: PointerEvent) {
    if (e.button === 0 && downSurOverlay && e.target === e.currentTarget) {
      fermer();
    }
    downSurOverlay = false;
  }

  function onOverlayPointerCancel() {
    downSurOverlay = false;
  }

  function onKeydown(e: KeyboardEvent) {
    if (e.key === "Escape") {
      e.preventDefault();
      e.stopPropagation();
      fermer();
    } else if (e.key === "ArrowRight") {
      e.preventDefault();
      tourner(1);
    } else if (e.key === "ArrowLeft") {
      e.preventDefault();
      tourner(-1);
    }
  }

  onDestroy(() => {
    if (timer) clearTimeout(timer);
  });
</script>

<svelte:window onkeydown={onKeydown} />

<div
  class="livre-overlay"
  onpointerdown={onOverlayPointerDown}
  onpointerup={onOverlayPointerUp}
  onpointercancel={onOverlayPointerCancel}
  role="presentation"
>
  <button class="livre-fermer" onclick={fermer} aria-label={$t("guide.fermer")}>✕</button>

  <div class="livre-scene" role="dialog" aria-modal="true" aria-label={$t("toolbar.guide")}>
    <div
      class="livre"
      class:survol-gauche={survolGauche}
      class:survol-droite={survolDroite}
      style="--n-feuilles: {N}; --fonte-titre: {fonteTitres}"
    >
      <!-- Bases fixes sous les piles : page de garde (gauche, visible à
           t=0 — illustration pleine page) et 4ᵉ de couverture (droite,
           visible à t=N — ornement décoratif). -->
      <div class="page-base gauche">
        <img class="garde-img" src={gardeGaucheImg} alt="" />
      </div>
      <div class="page-base droite">{@render garde()}</div>

      {#each feuilles as i (i)}
        <div
          class="feuille"
          class:tournee={i < tournees}
          class:courante={i === tournees}
          class:derniere-tournee={i === tournees - 1}
          class:en-mouvement={feuilleEnMouvement === i}
          style="--i: {i}"
        >
          <div class="face recto">{@render face(PAGES[2 * i], 2 * i + 1, "recto")}</div>
          <div class="face verso">{@render face(PAGES[2 * i + 1], 2 * i + 2, "verso")}</div>
        </div>
      {/each}

      <!-- Ombre de reliure : fixe par-dessus les deux demi-pages. -->
      <div class="reliure" aria-hidden="true"></div>
    </div>

    <!-- Zones « pince la tranche » : flèche au hover, masquées aux
         extrémités (pas de page à tourner). -->
    {#if tournees > 0}
      <button
        class="zone-tourne gauche"
        aria-label={$t("guide.pagePrecedente")}
        onclick={() => tourner(-1)}
        onmouseenter={() => (survolGauche = true)}
        onmouseleave={() => (survolGauche = false)}
      >
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M15 5l-7 7 7 7"/></svg>
      </button>
    {/if}
    {#if tournees < N}
      <button
        class="zone-tourne droite"
        aria-label={$t("guide.pageSuivante")}
        onclick={() => tourner(1)}
        onmouseenter={() => (survolDroite = true)}
        onmouseleave={() => (survolDroite = false)}
      >
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M9 5l7 7-7 7"/></svg>
      </button>
    {/if}
  </div>
</div>

<!-- Ornement doré générique (décor uniquement — aucune artwork externe). -->
{#snippet ornement()}
  <svg class="ornement" viewBox="0 0 160 24" fill="none" stroke="currentColor" stroke-width="1.2" stroke-linecap="round" aria-hidden="true">
    <path d="M8 12h52" /><path d="M100 12h52" />
    <path d="M80 3.5c6.5 4.2 6.5 12.8 0 17c-6.5-4.2-6.5-12.8 0-17z" />
    <circle cx="80" cy="12" r="1.6" fill="currentColor" stroke="none" />
  </svg>
{/snippet}

<!-- Page de garde décorative : verso orphelin, couverture intérieure. -->
{#snippet garde()}
  <div class="garde-deco">{@render ornement()}</div>
{/snippet}

<!-- Texte i18n avec marqueurs **fort** → <strong> (mots-clés surlignés). -->
{#snippet texte(cle: string)}
  {#each segments(cle) as s}{#if s.fort}<strong class="mot-cle">{s.texte}</strong>{:else}{s.texte}{/if}{/each}
{/snippet}

<!-- Une face imprimée : couverture (mise en page dédiée), page de garde
     décorative (verso orphelin), ou page de contenu (titre + filet +
     blocs + folio). -->
{#snippet face(pg: PageGuide | undefined, folio: number, cote: "recto" | "verso")}
  {#if !pg}
    <div class="face-gardes">{@render garde()}</div>
  {:else if pg.id === "couverture"}
    <div class="couverture">
      <div class="couv-ornement">{@render ornement()}</div>
      <h1 class="couv-titre">{$t(pg.titreCle)}</h1>
      <p class="couv-sous">{$t("guide.pages.couverture.sous")}</p>
      <p class="couv-hint">{$t("guide.tournezPages")}</p>
    </div>
    <!-- Signature de l'auteur en bas de couverture — nom propre, jamais
         traduit (règle i18n « ne pas traduire »). -->
    <span class="couv-signature">Ryo Shinkawa</span>
  {:else}
    <div class="page-contenu">
      <h2 class="page-titre">{@render texte(pg.titreCle)}</h2>
      <div class="page-filet" aria-hidden="true">{@render ornement()}</div>
      {#each pg.blocs as bloc}
        {#if bloc.type === "para"}
          <p class="para">{@render texte(bloc.cle)}</p>
        {:else if bloc.type === "liste"}
          <ul class="liste">
            {#each bloc.cles as cle}<li>{@render texte(cle)}</li>{/each}
          </ul>
        {:else if bloc.type === "etapes"}
          <ol class="etapes">
            {#each bloc.cles as cle}<li>{@render texte(cle)}</li>{/each}
          </ol>
        {:else if bloc.type === "note"}
          <aside class="note">{@render texte(bloc.cle)}</aside>
        {:else}
          <div class="sep" aria-hidden="true">{@render ornement()}</div>
        {/if}
      {/each}
      <span class="folio" class:folio-gauche={cote === "verso"}>{folio}</span>
    </div>
  {/if}
{/snippet}

<style>
  /* ===== Overlay — mêmes conventions que .modal-overlay ===== */
  .livre-overlay {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.7);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 10000;
  }
  /* Recette .modal-close du thème, en coin haut droit de l'écran. */
  .livre-fermer {
    position: absolute;
    top: 1rem;
    right: 1rem;
    z-index: 2;
    background: var(--btn-surface);
    box-shadow: var(--btn-inset);
    color: var(--texte);
    border: 1px solid var(--bordure-ui);
    font-size: 0.85rem;
    cursor: pointer;
    padding: 0.2rem 0.5rem;
    line-height: 1;
    transition: background 0.15s ease, box-shadow 0.15s ease, border-color 0.15s ease, color 0.15s ease;
  }
  .livre-fermer:hover {
    background: var(--btn-surface-hover);
    box-shadow: var(--btn-inset-hover);
    border-color: var(--message-error-color);
    color: var(--message-error-color);
  }

  /* ===== Scène 3D : la perspective vit sur le parent, preserve-3d sur
         le livre et les feuilles → les feuilles basculent en vraie 3D. */
  .livre-scene {
    position: relative;
    display: flex;
    perspective: 1600px;
  }
  .livre {
    position: relative;
    width: min(1080px, 92vw);
    height: min(680px, 82vh);
    transform-style: preserve-3d;
    filter: drop-shadow(0 24px 48px rgba(0, 0, 0, 0.55));
  }
  /* Couverture cuir : déborde légèrement du bloc de pages, partout. */
  .livre::before {
    content: "";
    position: absolute;
    inset: -14px -18px;
    background: linear-gradient(155deg, var(--livre-couverture-claire), var(--livre-couverture) 55%);
    border-radius: 10px 14px 14px 10px;
    box-shadow:
      inset 0 0 0 1px color-mix(in srgb, var(--livre-dore) 18%, transparent),
      inset 0 2px 6px color-mix(in srgb, var(--livre-dore) 12%, transparent),
      inset 0 -6px 14px rgba(0, 0, 0, 0.45);
    z-index: 0;
  }

  /* ===== Bases fixes : visibles seulement aux extrémités de lecture. */
  .page-base {
    position: absolute;
    top: 0;
    bottom: 0;
    width: 50%;
    background:
      radial-gradient(120% 90% at 50% 0%, transparent 60%, color-mix(in srgb, var(--livre-papier-ombre) 55%, transparent)),
      var(--livre-papier-ombre);
    overflow: hidden;
  }
  .page-base.gauche {
    left: 0;
    border-radius: 6px 0 0 6px;
    /* Tranche de pages simulée sur le bord extérieur. */
    box-shadow:
      inset 6px 0 0 -3px var(--livre-papier),
      inset -18px 0 24px -18px rgba(0, 0, 0, 0.5);
  }
  .page-base.droite {
    right: 0;
    border-radius: 0 6px 6px 0;
    box-shadow:
      inset -6px 0 0 -3px var(--livre-papier),
      inset 18px 0 24px -18px rgba(0, 0, 0, 0.5);
  }

  /* ===== Feuilles : moitié droite, pivot sur le bord gauche (reliure).
         Non tournée : pile de droite, i petit au-dessus.
         Tournée : pile de gauche, i grand au-dessus. */
  .feuille {
    position: absolute;
    left: 50%;
    top: 0;
    bottom: 0;
    width: 50%;
    transform-origin: left center;
    transform-style: preserve-3d;
    transition: transform 0.65s cubic-bezier(0.35, 0.1, 0.3, 1);
    z-index: calc(var(--n-feuilles) - var(--i));
  }
  .feuille.tournee {
    transform: rotateY(-180deg);
    z-index: calc(var(--i) + 1);
  }
  /* Coin « pincé » : la feuille sur le point d'être tournée se soulève. */
  .livre.survol-droite .feuille.courante { transform: rotateY(-7deg); }
  .livre.survol-gauche .feuille.derniere-tournee { transform: rotateY(-173deg); }
  .feuille.en-mouvement { will-change: transform; }

  /* ===== Faces : parchemin, ombre de pli côté reliure, verso pré-tourné. */
  .face {
    position: absolute;
    inset: 0;
    overflow: hidden;
    padding: 2rem 2.2rem 2.4rem;
    color: var(--livre-encre);
    backface-visibility: hidden;
    -webkit-backface-visibility: hidden;
  }
  .face.recto {
    background:
      linear-gradient(90deg, rgba(0, 0, 0, 0.16), transparent 9%),
      radial-gradient(140% 100% at 50% 0%, transparent 62%, color-mix(in srgb, var(--livre-papier-ombre) 60%, transparent)),
      var(--livre-papier);
    border-radius: 0 6px 6px 0;
    box-shadow: inset -6px 0 0 -3px var(--livre-papier-ombre);
  }
  .face.verso {
    transform: rotateY(180deg);
    background:
      linear-gradient(-90deg, rgba(0, 0, 0, 0.16), transparent 9%),
      radial-gradient(140% 100% at 50% 0%, transparent 62%, color-mix(in srgb, var(--livre-papier-ombre) 60%, transparent)),
      var(--livre-papier);
    border-radius: 6px 0 0 6px;
    box-shadow: inset 6px 0 0 -3px var(--livre-papier-ombre);
  }

  /* Ombre de reliure au centre du livre — fixe, au-dessus des pages. */
  .reliure {
    position: absolute;
    left: 50%;
    top: 0;
    bottom: 0;
    width: 56px;
    transform: translateX(-50%);
    background: linear-gradient(90deg, transparent, rgba(0, 0, 0, 0.32) 46%, rgba(0, 0, 0, 0.38) 54%, transparent);
    pointer-events: none;
    z-index: calc(var(--n-feuilles) + 2);
  }

  /* ===== Zones de navigation sur les tranches externes du livre. */
  .zone-tourne {
    position: absolute;
    top: 0;
    bottom: 0;
    width: 72px;
    display: flex;
    align-items: center;
    justify-content: center;
    background: transparent;
    border: none;
    padding: 0;
    cursor: pointer;
    color: var(--texte);
    z-index: 60;
    /* Chrome global des boutons neutralisé (app.css) : backdrop-filter
       blur(6px) givrait tout le texte sous la zone — bande floue sur les
       tranches du livre — et les pseudo-éléments shine/reflet n'ont pas
       de sens sur une zone invisible. */
    backdrop-filter: none;
  }
  .zone-tourne::before,
  .zone-tourne::after {
    content: none;
  }
  /* Zones décalées vers l'extérieur : la flèche cerclée repose presque
     entièrement hors de la page, sur le fond assombri. */
  .zone-tourne.gauche { left: -30px; }
  .zone-tourne.droite { right: -30px; }
  .zone-tourne svg {
    width: 32px;
    height: 32px;
    box-sizing: border-box;
    padding: 6px;
    border: 2px solid var(--livre-couverture-claire);
    border-radius: 50%;
    /* Pastille « cachet de cire » : dégradé cuir clair → cuir + liseré
       lumineux en haut ; la flèche bordeaux reste lisible sur le fond
       sombre comme sur la page claire. */
    background: linear-gradient(155deg, var(--livre-couverture-claire) 0%, var(--livre-couverture) 100%);
    box-shadow:
      inset 0 1px 2px rgba(255, 244, 214, 0.45),
      inset 0 -2px 4px rgba(0, 0, 0, 0.3);
    /* Flèches toujours visibles (le lecteur doit deviner qu'on peut
       tourner) : ivoire parchemin sur la pastille cuir — le survol ne
       fait qu'accentuer. */
    color: var(--livre-papier);
    opacity: 0.7;
    transform: scale(0.9);
    transition: opacity 0.18s ease, transform 0.18s ease;
    filter: drop-shadow(0 2px 6px rgba(0, 0, 0, 0.6));
  }
  .zone-tourne:hover svg,
  .zone-tourne:focus-visible svg {
    opacity: 0.95;
    transform: scale(1);
  }

  /* ===== Contenu imprimé — palette --livre-* (artwork), titres Caveat. */
  .ornement {
    width: 9rem;
    height: auto;
    color: var(--livre-dore);
  }
  .garde-deco,
  .face-gardes {
    height: 100%;
    display: flex;
    align-items: center;
    justify-content: center;
    opacity: 0.55;
  }
  /* Illustration de garde (page de gauche à t=0) : contain = l'image
     entière visible au plus grand, le parchemin fait office de marie-
     louise autour ; ombre portée légère pour décoller les personnages
     du papier (PNG à fond transparent). */
  .garde-img {
    width: 100%;
    height: 100%;
    object-fit: contain;
    padding: 1rem;
    box-sizing: border-box;
    filter: drop-shadow(0 8px 14px rgba(0, 0, 0, 0.28));
  }
  .couverture {
    height: 100%;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 1.1rem;
    text-align: center;
  }
  .couv-ornement .ornement { width: 13rem; }
  .couv-titre {
    font-family: var(--fonte-titre, "Caveat", cursive);
    font-size: 3.4rem;
    font-weight: 400;
    margin: 0;
    color: var(--livre-encre);
  }
  .couv-sous {
    font-size: 0.95rem;
    color: var(--livre-encre-douce);
    margin: 0;
    font-style: italic;
  }
  .couv-hint {
    margin: 1.4rem 0 0;
    font-size: 0.8rem;
    letter-spacing: 0.12em;
    text-transform: uppercase;
    color: var(--livre-dore);
  }
  /* Signature de l'auteur, pied de couverture — Caveat donne l'effet
     manuscrit (même police que les titres, zéro ajout de font). */
  .couv-signature {
    position: absolute;
    bottom: 1.1rem;
    left: 0;
    right: 0;
    text-align: center;
    font-family: "Caveat", cursive;
    font-size: 1.5rem;
    color: var(--livre-encre-douce);
    pointer-events: none;
  }
  .page-titre {
    font-family: var(--fonte-titre, "Caveat", cursive);
    font-size: 2.1rem;
    font-weight: 400;
    margin: 0 0 0.4rem;
    color: var(--livre-encre);
  }
  .page-filet {
    display: flex;
    justify-content: flex-start;
    margin: -0.5rem 0 1rem -0.4rem;
    opacity: 0.85;
  }
  .page-filet .ornement { width: 7rem; }
  .para {
    font-size: 0.95rem;
    line-height: 1.55;
    margin: 0 0 0.9rem;
  }
  /* Lettrine manuscrite sur le premier paragraphe — pur CSS, zéro JS. */
  .page-contenu > .para:first-of-type::first-letter {
    font-family: var(--fonte-titre, "Caveat", cursive);
    font-size: 2.4em;
    line-height: 0.9;
    padding-right: 0.12em;
    color: var(--livre-dore);
  }
  .liste,
  .etapes {
    font-size: 0.92rem;
    line-height: 1.5;
    margin: 0 0 0.9rem;
    padding-left: 1.3rem;
  }
  /* Mots-clés surlignés (**…** dans les textes i18n) : ruban + gras,
     lisible sur le parchemin sans casser l'ambiance livre. */
  .mot-cle {
    font-weight: 700;
    color: var(--livre-ruban);
  }
  .liste li,
  .etapes li { margin-bottom: 0.45rem; }
  .liste li::marker { color: var(--livre-dore); }
  .etapes li::marker {
    font-family: "Caveat", cursive;
    font-size: 1.25em;
    color: var(--livre-dore);
  }
  .note {
    font-size: 0.86rem;
    line-height: 1.5;
    margin: 1rem 0 0;
    padding: 0.6rem 0.8rem;
    border-left: 3px solid var(--livre-dore);
    background: color-mix(in srgb, var(--livre-papier-ombre) 45%, transparent);
    color: var(--livre-encre-douce);
    font-style: italic;
  }
  .sep {
    display: flex;
    justify-content: center;
    margin: 1.1rem 0;
    opacity: 0.8;
  }
  .folio {
    position: absolute;
    bottom: 1rem;
    right: 1.6rem;
    font-size: 0.72rem;
    color: var(--livre-encre-douce);
    letter-spacing: 0.08em;
  }
  .folio.folio-gauche {
    right: auto;
    left: 1.6rem;
  }

  /* Empreinte de la touche entrée via clavier : mêmes affordances. */
  .zone-tourne:focus-visible {
    outline: 2px solid var(--bordure-ui-active);
    outline-offset: -2px;
  }
</style>
