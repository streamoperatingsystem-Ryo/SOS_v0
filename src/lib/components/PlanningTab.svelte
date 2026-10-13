<script lang="ts">
  // Onglet « Planning » de la modale « Interactions Live » — planning Twitch
  // interactif (Helix /schedule). Twitch = source de vérité, zéro persistance :
  // chargerPlanning() relit à l'ouverture et après chaque mutation.
  //
  // Glisser-déposer (dataTransfer JSON, MIME "application/x-sos-planning") :
  //   - jacquette (colonne droite) → cellule jour/heure : création PONCTUELLE
  //     1 h (le backend retombe sur récurrent si la chaîne n'est pas affiliée) ;
  //   - jacquette → carte segment : PATCH category_id (confirmé si récurrent —
  //     s'applique à toute la série) ;
  //   - segment ponctuel → cellule : PATCH start_time (déplacement).
  //   Les récurrents ne se dragguent pas : changer jour/heure = DELETE +
  //   recréation de la série (formulaire, confirmé).
  import { onMount } from "svelte";
  import { get } from "svelte/store";
  import { t, localeTag } from "../stores/langue";
  import { connexions, reconnecterTwitch } from "../stores/chat";
  import { broadcaster } from "../stores/communaute";
  import {
    planningSegments,
    planningVacances,
    planningJacquettes,
    planningChargement,
    planningErreur,
    chargerPlanning,
    planningCreerSegment,
    planningMajSegment,
    planningSupprimerSegment,
    planningSetVacances,
    planningRechercherCategories,
    planningTimezone,
    type PlanningSegment,
    type PlanningSegmentPatch,
  } from "../stores/planning";

  const MIME = "application/x-sos-planning";
  const HEURES = Array.from({ length: 24 }, (_, i) => i);
  /// Durée par défaut d'un segment créé au glisser-déposer (minutes).
  const DUREE_DEFAUT = 60;
  /// Heure par défaut quand le drop ne donne qu'un jour (vue mois).
  const HEURE_DEFAUT = 20;
  const DUREES = [30, 60, 90, 120, 150, 180, 240, 300, 360];

  // ===== État =====

  type Vue = "semaine" | "mois" | "liste";
  let vue = $state<Vue>("semaine");
  /// Lundi de la semaine affichée / 1er du mois affiché (réancré par nav).
  let ancre = $state<Date>(lundiDe(new Date()));
  /// Segment ouvert dans le panneau d'édition (colonne droite).
  let segSel = $state<PlanningSegment | null>(null);
  /// Clé de la cible de drop survolée (cell:<iso>|<h>, mois:<iso>, seg:<id>).
  let survol = $state<string | null>(null);
  let actionEnCours = $state<string | null>(null);
  let msg = $state<{ texte: string; ok: boolean } | null>(null);
  /// Confirmation inline générique (destructif : série, suppression).
  let confirm = $state<{ texte: string; action: () => Promise<void> } | null>(
    null
  );
  /// Vacances : panneau + champs.
  let vacOuvert = $state(false);
  let vacDebut = $state("");
  let vacFin = $state("");

  // Édition segment (panneau droit).
  let editTitre = $state("");
  let editDate = $state("");
  let editHeure = $state("");
  let editDuree = $state(DUREE_DEFAUT);

  // Recherche de jeu (debounce 300 ms + garde de course).
  let q = $state("");
  let resultats = $state<{ id: string; name: string; box_art_url: string }[]>(
    []
  );
  let rechEnCours = $state(false);
  let catTimer: ReturnType<typeof setTimeout> | undefined;
  let rechSeq = 0;

  // ===== Dérivés des stores =====

  let segments = $derived($planningSegments);
  let vacances = $derived($planningVacances);
  let jacquettes = $derived($planningJacquettes);
  let chargement = $derived($planningChargement);
  let err = $derived($planningErreur);
  let connecte = $derived($connexions.twitch);
  /// "affiliate" | "partner" | "" — Twitch refuse les segments ponctuels
  /// aux chaînes non affiliées (le backend fait le repli récurrent).
  let bType = $derived($broadcaster?.broadcaster_type ?? "");
  let peutPonctuel = $derived(bType === "affiliate" || bType === "partner");

  /// Segments parsés + triés chronologiquement (occurrences).
  let segsParsed = $derived(
    segments
      .map((s) => ({
        s,
        debut: new Date(s.start_time),
        fin: new Date(s.end_time),
      }))
      .filter((x) => !isNaN(x.debut.getTime()))
      .sort((a, b) => a.debut.getTime() - b.debut.getTime())
  );

  /// 7 jours de la semaine affichée (ancre = lundi en vue semaine/liste).
  let jours = $derived(Array.from({ length: 7 }, (_, i) => ajouterJours(lundiDe(ancre), i)));

  /// Grille mois : 6 lignes × 7 jours à partir du lundi précédent le 1er.
  let joursMois = $derived.by(() => {
    const debut = lundiDe(new Date(ancre.getFullYear(), ancre.getMonth(), 1));
    return Array.from({ length: 42 }, (_, i) => ajouterJours(debut, i));
  });

  /// Catégories déjà utilisées au planning (affichées quand la recherche est
  /// vide — draggables comme les résultats de recherche).
  let jeuxUtilises = $derived(
    [
      ...new Map(
        segments
          .filter((s) => s.category_id)
          .map((s) => [s.category_id, { id: s.category_id, name: s.category_name }])
      ).values(),
    ]
  );

  /// Jeux affichés dans la colonne droite : résultats de recherche, sinon les
  /// catégories déjà présentes au planning.
  let jeuxAffichage = $derived(q.trim().length >= 2 ? resultats : jeuxUtilises);

  /// Occurrences groupées par jour pour la vue liste.
  let groupesListe = $derived.by(() => {
    const groupes = new Map<string, typeof segsParsed>();
    for (const x of segsParsed) {
      const k = cleJour(x.debut);
      const g = groupes.get(k) ?? [];
      g.push(x);
      groupes.set(k, g);
    }
    return [...groupes.entries()].map(([jour, items]) => ({ jour, items }));
  });

  /// Libellé de la période affichée (header).
  let titrePeriode = $derived.by(() => {
    const loc = get(localeTag);
    if (vue === "mois") {
      return ancre.toLocaleDateString(loc, { month: "long", year: "numeric" });
    }
    const debut = jours[0];
    const fin = jours[6];
    const d1 = debut.toLocaleDateString(loc, { day: "numeric", month: "short" });
    const d2 = fin.toLocaleDateString(loc, {
      day: "numeric",
      month: "short",
      year: "numeric",
    });
    return `${d1} – ${d2}`;
  });

  // ===== Dates =====

  function lundiDe(d: Date): Date {
    const x = new Date(d.getFullYear(), d.getMonth(), d.getDate());
    x.setDate(x.getDate() - ((x.getDay() + 6) % 7));
    return x;
  }

  function ajouterJours(d: Date, n: number): Date {
    const x = new Date(d);
    x.setDate(x.getDate() + n);
    return x;
  }

  function memeJour(a: Date, b: Date): boolean {
    return (
      a.getFullYear() === b.getFullYear() &&
      a.getMonth() === b.getMonth() &&
      a.getDate() === b.getDate()
    );
  }

  function cleJour(d: Date): string {
    return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`;
  }

  function hhmm(d: Date): string {
    return `${String(d.getHours()).padStart(2, "0")}:${String(d.getMinutes()).padStart(2, "0")}`;
  }

  function jourLabel(d: Date): string {
    return d.toLocaleDateString(get(localeTag), { weekday: "short" });
  }

  function jourComplet(d: Date): string {
    return d.toLocaleDateString(get(localeTag), {
      weekday: "long",
      day: "numeric",
      month: "long",
    });
  }

  function dateInput(d: Date): string {
    return cleJour(d);
  }

  /// Borne basse du GET /schedule (lundi de la semaine, ou lundi précédant
  /// le 1er en vue mois — les occurrences antérieures ne sont pas utiles).
  function borneBasse(): Date {
    if (vue === "mois") {
      return lundiDe(new Date(ancre.getFullYear(), ancre.getMonth(), 1));
    }
    return lundiDe(ancre);
  }

  function recharger(): void {
    void chargerPlanning(borneBasse().toISOString());
  }

  // ===== Navigation =====

  function nav(dir: number): void {
    if (vue === "mois") {
      ancre = new Date(ancre.getFullYear(), ancre.getMonth() + dir, 1);
    } else {
      ancre = ajouterJours(lundiDe(ancre), dir * 7);
    }
    segSel = null;
    recharger();
  }

  function aujourdhui(): void {
    ancre = vue === "mois" ? new Date(new Date().setDate(1)) : lundiDe(new Date());
    recharger();
  }

  function changerVue(v: Vue): void {
    if (vue === v) return;
    vue = v;
    ancre = v === "mois" ? new Date(new Date().setDate(1)) : lundiDe(ancre);
    segSel = null;
    recharger();
  }

  // ===== Helpers UI =====

  async function executer(id: string, fn: () => Promise<void>): Promise<void> {
    actionEnCours = id;
    msg = null;
    try {
      await fn();
    } catch (e) {
      const m = String(e);
      // need_reauth → banner existant via planningErreur, pas de message brut.
      if (m !== "need_reauth") msg = { texte: m, ok: false };
    } finally {
      actionEnCours = null;
    }
  }

  function demanderConfirm(texte: string, action: () => Promise<void>): void {
    confirm = { texte, action };
  }

  async function onConfirmer(): Promise<void> {
    const a = confirm?.action;
    confirm = null;
    if (a) await executer("confirm", a);
  }

  function onReconnecter(): void {
    void reconnecterTwitch();
  }

  function jacquette(id: string): string {
    return jacquettes[id]
      ?.replace("{width}", "52")
      .replace("{height}", "72") ?? "";
  }

  function dureeMin(s: PlanningSegment): number {
    return Math.max(
      0,
      Math.round(
        (new Date(s.end_time).getTime() - new Date(s.start_time).getTime()) /
          60000
      )
    );
  }

  function titreSeg(s: PlanningSegment): string {
    return s.title || s.category_name || get(t)("planning.stream");
  }

  function segsCellule(jour: Date, heure: number) {
    return segsParsed.filter(
      (x) => memeJour(x.debut, jour) && x.debut.getHours() === heure
    );
  }

  function segsDuJour(jour: Date) {
    return segsParsed.filter((x) => memeJour(x.debut, jour));
  }

  // ===== Recherche de jeux =====

  function onSearchInput(): void {
    clearTimeout(catTimer);
    const query = q.trim();
    if (query.length < 2) {
      resultats = [];
      rechEnCours = false;
      return;
    }
    rechEnCours = true;
    catTimer = setTimeout(() => {
      const seq = ++rechSeq;
      void (async () => {
        try {
          const res = await planningRechercherCategories(query);
          if (seq === rechSeq) resultats = res;
        } catch {
          if (seq === rechSeq) resultats = [];
        } finally {
          if (seq === rechSeq) rechEnCours = false;
        }
      })();
    }, 300);
  }

  // ===== Drag & drop =====

  function dragStartCategorie(
    e: DragEvent,
    c: { id: string; name: string }
  ): void {
    e.dataTransfer?.setData(
      MIME,
      JSON.stringify({ type: "categorie", id: c.id, name: c.name })
    );
    if (e.dataTransfer) e.dataTransfer.effectAllowed = "copy";
  }

  function dragStartSegment(e: DragEvent, s: PlanningSegment): void {
    e.dataTransfer?.setData(MIME, JSON.stringify({ type: "segment", id: s.id }));
    if (e.dataTransfer) e.dataTransfer.effectAllowed = "move";
  }

  function lireDrag(
    e: DragEvent
  ): { type: string; id: string; name?: string } | null {
    const raw = e.dataTransfer?.getData(MIME);
    if (!raw) return null;
    try {
      return JSON.parse(raw);
    } catch {
      return null;
    }
  }

  function onDragOver(e: DragEvent, cle: string): void {
    e.preventDefault();
    survol = cle;
  }

  /// Drop sur une cellule jour (+heure en vue semaine, heure par défaut en
  /// vue mois — ajustable ensuite dans le formulaire du segment).
  async function onDropCellule(
    e: DragEvent,
    jour: Date,
    heure?: number
  ): Promise<void> {
    e.preventDefault();
    survol = null;
    const d = lireDrag(e);
    if (!d) return;
    const debut = new Date(
      jour.getFullYear(),
      jour.getMonth(),
      jour.getDate(),
      heure ?? HEURE_DEFAUT,
      0
    );
    if (d.type === "categorie") {
      await executer("creer", async () => {
        await planningCreerSegment({
          startTime: debut.toISOString(),
          isRecurring: false,
          durationMin: DUREE_DEFAUT,
          categoryId: d.id,
          title: d.name,
        });
        await chargerPlanning(borneBasse().toISOString());
        msg = { texte: get(t)("planning.creeOk"), ok: true };
      });
    } else if (d.type === "segment") {
      const seg = segments.find((s) => s.id === d.id);
      // Garde : les récurrents ne sont pas draggables (série entière).
      if (!seg || seg.is_recurring) return;
      // Vue mois : l'heure du segment est conservée.
      const cible =
        heure !== undefined
          ? debut
          : new Date(
              jour.getFullYear(),
              jour.getMonth(),
              jour.getDate(),
              new Date(seg.start_time).getHours(),
              new Date(seg.start_time).getMinutes()
            );
      if (cible.getTime() === new Date(seg.start_time).getTime()) return;
      await executer("deplacer", async () => {
        await planningMajSegment(d.id, {
          start_time: cible.toISOString(),
          timezone: planningTimezone(),
        });
        await chargerPlanning(borneBasse().toISOString());
        msg = { texte: get(t)("planning.deplaceOk"), ok: true };
      });
    }
  }

  /// Drop d'une jacquette sur une carte segment = changer le jeu
  /// (PATCH category_id — confirmé sur un récurrent : toute la série).
  async function onDropSegment(
    e: DragEvent,
    cible: PlanningSegment
  ): Promise<void> {
    e.preventDefault();
    e.stopPropagation();
    survol = null;
    const d = lireDrag(e);
    if (!d || d.type !== "categorie" || d.id === cible.category_id) return;
    const appliquer = async () => {
      await planningMajSegment(cible.id, { category_id: d.id });
      await chargerPlanning(borneBasse().toISOString());
      msg = { texte: get(t)("planning.majOk"), ok: true };
    };
    if (cible.is_recurring) {
      demanderConfirm(get(t)("planning.confirmSerie"), appliquer);
    } else {
      await executer("cat", appliquer);
    }
  }

  // ===== Panneau d'édition =====

  function onSelectSeg(s: PlanningSegment): void {
    segSel = s;
    editTitre = s.title || s.category_name;
    const debut = new Date(s.start_time);
    editDate = dateInput(debut);
    editHeure = hhmm(debut);
    editDuree = dureeMin(s) || DUREE_DEFAUT;
  }

  /// DELETE ancien + POST nouveau — pour les opérations que Twitch ne
  /// permet pas en PATCH (déplacement ou conversion d'un récurrent).
  async function recreer(
    s: PlanningSegment,
    debut: Date,
    isRecurring: boolean,
    duree: number,
    titre: string
  ): Promise<void> {
    // Créer d'abord : si ça échoue, la série actuelle reste intacte.
    await planningCreerSegment({
      startTime: debut.toISOString(),
      isRecurring,
      durationMin: duree,
      categoryId: s.category_id || undefined,
      title: titre || undefined,
    });
    await planningSupprimerSegment(s.id);
  }

  async function onAppliquerSeg(): Promise<void> {
    const s = segSel;
    if (!s) return;
    const debut = new Date(`${editDate}T${editHeure || "00:00"}`);
    if (isNaN(debut.getTime())) return;
    const actuelDebut = new Date(s.start_time);
    const dureeActuelle = dureeMin(s);
    const heureChangee = debut.getTime() !== actuelDebut.getTime();
    const titre = editTitre.trim();
    const patch: PlanningSegmentPatch = {};
    if (titre !== (s.title || "")) patch.title = titre;
    if (editDuree !== dureeActuelle) patch.duration_min = editDuree;

    const appliquer = async () => {
      if (heureChangee && s.is_recurring) {
        // Série recréée au nouveau créneau (confirmé en amont).
        await recreer(s, debut, true, editDuree, titre);
      } else {
        if (heureChangee) {
          patch.start_time = debut.toISOString();
          patch.timezone = planningTimezone();
        }
        if (Object.keys(patch).length === 0) {
          segSel = null;
          return;
        }
        await planningMajSegment(s.id, patch);
      }
      segSel = null;
      await chargerPlanning(borneBasse().toISOString());
      msg = { texte: get(t)("planning.majOk"), ok: true };
    };

    if (s.is_recurring && heureChangee) {
      demanderConfirm(get(t)("planning.confirmSerieRecreate"), appliquer);
    } else {
      await executer("seg-apply", appliquer);
    }
  }

  /// Annuler / rétablir un stream (is_canceled — sur un récurrent, annule
  /// la prochaine occurrence seulement, la série continue).
  async function onToggleAnnule(s: PlanningSegment): Promise<void> {
    await executer("seg-cancel", async () => {
      await planningMajSegment(s.id, { is_canceled: s.canceled_until === null });
      segSel = null;
      await chargerPlanning(borneBasse().toISOString());
      msg = {
        texte: get(t)(
          s.canceled_until === null
            ? "planning.annuleOk"
            : "planning.retabliOk"
        ),
        ok: true,
      };
    });
  }

  function onSupprimer(s: PlanningSegment): void {
    const appliquer = async () => {
      await planningSupprimerSegment(s.id);
      segSel = null;
      await chargerPlanning(borneBasse().toISOString());
      msg = { texte: get(t)("planning.supprOk"), ok: true };
    };
    demanderConfirm(
      get(t)(
        s.is_recurring ? "planning.confirmSupprSerie" : "planning.confirmSuppr"
      ),
      appliquer
    );
  }

  /// Conversion ponctuel ↔ récurrent = DELETE + POST (confirmé — sur un
  /// récurrent, toute la série est remplacée par un seul créneau).
  function onConvertir(s: PlanningSegment, versRecurrent: boolean): void {
    const appliquer = async () => {
      await recreer(
        s,
        new Date(s.start_time),
        versRecurrent,
        dureeMin(s) || DUREE_DEFAUT,
        s.title
      );
      segSel = null;
      await chargerPlanning(borneBasse().toISOString());
      msg = { texte: get(t)("planning.majOk"), ok: true };
    };
    demanderConfirm(
      get(t)(
        versRecurrent
          ? "planning.confirmVersRecurrent"
          : "planning.confirmVersPonctuel"
      ),
      appliquer
    );
  }

  // ===== Vacances =====

  function onToggleVacances(): void {
    vacOuvert = !vacOuvert;
    if (vacOuvert) {
      vacDebut = vacances?.start?.slice(0, 10) ?? "";
      vacFin = vacances?.end?.slice(0, 10) ?? "";
    }
  }

  async function onVacancesAppliquer(): Promise<void> {
    if (!vacDebut || !vacFin) return;
    await executer("vac", async () => {
      await planningSetVacances(true, vacDebut, vacFin);
      await chargerPlanning(borneBasse().toISOString());
      vacOuvert = false;
      msg = { texte: get(t)("planning.vacOk"), ok: true };
    });
  }

  async function onVacancesDesactiver(): Promise<void> {
    await executer("vac", async () => {
      await planningSetVacances(false);
      await chargerPlanning(borneBasse().toISOString());
      vacOuvert = false;
      msg = { texte: get(t)("planning.vacOff"), ok: true };
    });
  }

  // Helix relu à chaque ouverture de l'onglet (zéro persistance).
  onMount(recharger);
</script>

{#if !connecte}
  <p class="hint-centre">{$t("planning.connecterTwitch")}</p>
{:else if err === "need_reauth"}
  <div class="need-reauth">
    <span>{$t("planning.scopesManquants")}</span>
    <button class="action" onclick={onReconnecter}>
      {$t("planning.reconnecter")}
    </button>
  </div>
{:else}
  <div class="planning-layout">
    <!-- Colonne gauche : navigation + vue planning -->
    <div class="planning-main">
      <div class="planning-header">
        <div class="nav-groupe">
          <button class="action" onclick={() => nav(-1)} aria-label={$t("planning.precedent")}>‹</button>
          <button class="action" onclick={aujourdhui}>{$t("planning.aujourdhui")}</button>
          <button class="action" onclick={() => nav(1)} aria-label={$t("planning.suivant")}>›</button>
          <span class="periode">{titrePeriode}</span>
        </div>
        <div class="vues">
          <button class="action" class:actif={vue === "semaine"} onclick={() => changerVue("semaine")}>
            {$t("planning.vueSemaine")}
          </button>
          <button class="action" class:actif={vue === "mois"} onclick={() => changerVue("mois")}>
            {$t("planning.vueMois")}
          </button>
          <button class="action" class:actif={vue === "liste"} onclick={() => changerVue("liste")}>
            {$t("planning.vueListe")}
          </button>
          <button
            class="action vac"
            class:actif={vacOuvert || vacances !== null}
            onclick={onToggleVacances}
            title={$t("planning.vacancesHint")}
          >{$t("planning.vacances")}</button>
        </div>
      </div>

      {#if vacances && !vacOuvert}
        <p class="vac-info">
          {$t("planning.vacancesPeriode", { debut: vacances.start.slice(0, 10), fin: vacances.end.slice(0, 10) })}
        </p>
      {/if}

      {#if vacOuvert}
        <div class="modal-bloc vac-bloc">
          <div class="modal-bloc-titre">{$t("planning.vacances")}</div>
          <div class="vac-row">
            <label class="field-label" for="vac-debut">{$t("planning.vacancesDu")}</label>
            <input id="vac-debut" type="date" bind:value={vacDebut} />
            <label class="field-label" for="vac-fin">{$t("planning.vacancesAu")}</label>
            <input id="vac-fin" type="date" bind:value={vacFin} />
          </div>
          <div class="vac-actions">
            <button
              class="action"
              onclick={onVacancesAppliquer}
              disabled={!vacDebut || !vacFin || actionEnCours === "vac"}
            >{$t("planning.vacancesActiver")}</button>
            {#if vacances}
              <button
                class="action danger"
                onclick={onVacancesDesactiver}
                disabled={actionEnCours === "vac"}
              >{$t("planning.vacancesDesactiver")}</button>
            {/if}
          </div>
          <p class="hint">{$t("planning.vacancesHint")}</p>
        </div>
      {/if}

      {#if confirm}
        <div class="confirm-bloc">
          <p class="confirm-texte">{confirm.texte}</p>
          <div class="confirm-actions">
            <button class="action" onclick={() => (confirm = null)}>
              {$t("planning.annuler")}
            </button>
            <button
              class="action danger"
              onclick={onConfirmer}
              disabled={actionEnCours === "confirm"}
            >{$t("planning.confirmer")}</button>
          </div>
        </div>
      {/if}

      {#if msg}
        <p class="planning-msg" class:ok={msg.ok}>{msg.texte}</p>
      {:else if err}
        <p class="planning-msg">{err}</p>
      {/if}

      {#if chargement && segments.length === 0}
        <p class="hint-centre">{$t("commun.chargement")}</p>
      {:else if vue === "semaine"}
        <div class="grille-semaine">
          {#each jours as jour (cleJour(jour))}
            <div class="jour">
              <div class="jour-tete" class:aujourdhui={memeJour(jour, new Date())}>
                {jourLabel(jour)} {jour.getDate()}
              </div>
              <div class="jour-corps">
                {#each HEURES as h}
                  {@const cle = `cell:${cleJour(jour)}|${h}`}
                  <div
                    class="cellule"
                    class:survol={survol === cle}
                    role="presentation"
                    ondragover={(e) => onDragOver(e, cle)}
                    ondragleave={() => (survol = null)}
                    ondrop={(e) => void onDropCellule(e, jour, h)}
                  >
                    <span class="cellule-heure">{h}h</span>
                    {#each segsCellule(jour, h) as x (x.s.id)}
                      {@const s = x.s}
                      <button
                        class="segment"
                        class:annule={s.canceled_until !== null}
                        class:recurrent={s.is_recurring}
                        class:survol={survol === `seg:${s.id}`}
                        draggable={!s.is_recurring}
                        ondragstart={(e) => dragStartSegment(e, s)}
                        ondragover={(e) => {
                          e.preventDefault();
                          e.stopPropagation();
                          survol = `seg:${s.id}`;
                        }}
                        ondragleave={(e) => e.stopPropagation()}
                        ondrop={(e) => void onDropSegment(e, s)}
                        onclick={() => onSelectSeg(s)}
                        title={titreSeg(s)}
                      >
                        {#if jacquette(s.category_id)}
                          <img class="seg-img" src={jacquette(s.category_id)} alt="" />
                        {/if}
                        <span class="seg-texte">
                          <span class="seg-titre">{titreSeg(s)}</span>
                          <span class="seg-heure">{hhmm(x.debut)}–{hhmm(x.fin)}</span>
                        </span>
                      </button>
                    {/each}
                  </div>
                {/each}
              </div>
            </div>
          {/each}
        </div>
      {:else if vue === "mois"}
        <div class="grille-mois">
          {#each jours.slice(0, 7) as j}
            <div class="mois-tete">{jourLabel(j)}</div>
          {/each}
          {#each joursMois as jour (cleJour(jour))}
            {@const cle = `mois:${cleJour(jour)}`}
            <div
              class="mois-cell"
              class:hors-mois={jour.getMonth() !== ancre.getMonth()}
              class:aujourdhui={memeJour(jour, new Date())}
              class:survol={survol === cle}
              role="presentation"
              ondragover={(e) => onDragOver(e, cle)}
              ondragleave={() => (survol = null)}
              ondrop={(e) => void onDropCellule(e, jour)}
            >
              <span class="mois-date">{jour.getDate()}</span>
              {#each segsDuJour(jour).slice(0, 3) as x (x.s.id)}
                <button
                  class="mini-seg"
                  class:annule={x.s.canceled_until !== null}
                  draggable={!x.s.is_recurring}
                  ondragstart={(e) => dragStartSegment(e, x.s)}
                  onclick={() => onSelectSeg(x.s)}
                  title={titreSeg(x.s)}
                >
                  {hhmm(x.debut)} {titreSeg(x.s)}
                </button>
              {/each}
              {#if segsDuJour(jour).length > 3}
                <span class="mini-plus">+{segsDuJour(jour).length - 3}</span>
              {/if}
            </div>
          {/each}
        </div>
      {:else}
        <div class="liste">
          {#if segsParsed.length === 0}
            <p class="hint-centre">{$t("planning.aucunSegment")}</p>
          {/if}
          {#each groupesListe as g (g.jour)}
            <div class="liste-jour">{jourComplet(g.items[0].debut)}</div>
            {#each g.items as x (x.s.id)}
              {@const s = x.s}
              <button class="liste-seg" class:annule={s.canceled_until !== null} onclick={() => onSelectSeg(s)}>
                {#if jacquette(s.category_id)}
                  <img class="liste-img" src={jacquette(s.category_id)} alt="" />
                {/if}
                <span class="liste-texte">
                  <span class="liste-titre">{titreSeg(s)}</span>
                  <span class="liste-meta">
                    {hhmm(x.debut)}–{hhmm(x.fin)}{#if s.is_recurring} · {$t("planning.toutesLesSemaines")}{/if}{#if s.canceled_until !== null} · {$t("planning.annuleBadge")}{/if}
                  </span>
                </span>
              </button>
            {/each}
          {/each}
        </div>
      {/if}
    </div>

    <!-- Colonne droite : édition du segment sélectionné, sinon recherche de jeux -->
    <div class="planning-side">
      {#if segSel}
        {@const s = segSel}
        <div class="modal-bloc">
          <div class="modal-bloc-titre">
            {$t("planning.segment")}
            {#if s.is_recurring}
              <span class="badge-rec">{$t("planning.toutesLesSemaines")}</span>
            {/if}
            {#if s.canceled_until !== null}
              <span class="badge-annule">{$t("planning.annuleBadge")}</span>
            {/if}
          </div>
          <label class="field-label" for="seg-titre">{$t("planning.segTitre")}</label>
          <input id="seg-titre" type="text" maxlength="140" bind:value={editTitre} />
          <div class="seg-2col">
            <div>
              <label class="field-label" for="seg-date">{$t("planning.segDate")}</label>
              <input id="seg-date" type="date" bind:value={editDate} />
            </div>
            <div>
              <label class="field-label" for="seg-heure">{$t("planning.segHeure")}</label>
              <input id="seg-heure" type="time" bind:value={editHeure} />
            </div>
          </div>
          <label class="field-label" for="seg-duree">{$t("planning.segDuree")}</label>
          <select id="seg-duree" bind:value={editDuree}>
            {#each DUREES.includes(editDuree) ? DUREES : [...DUREES, editDuree].sort((a, b) => a - b) as d}
              <option value={d}>{d} min</option>
            {/each}
          </select>
          {#if s.category_name}
            <div class="seg-jeu">
              {#if jacquette(s.category_id)}
                <img class="jacquette" src={jacquette(s.category_id)} alt="" />
              {/if}
              <span>{s.category_name}</span>
            </div>
            <p class="hint">{$t("planning.segJeuHint")}</p>
          {/if}
          {#if s.is_recurring}
            <p class="hint">{$t("planning.serieHint")}</p>
          {/if}
          <button
            class="action"
            onclick={onAppliquerSeg}
            disabled={actionEnCours === "seg-apply"}
          >{$t("planning.appliquer")}</button>
          <div class="seg-actions">
            <button
              class="action"
              onclick={() => void onToggleAnnule(s)}
              disabled={actionEnCours === "seg-cancel"}
            >
              {s.canceled_until === null ? $t("planning.annulerStream") : $t("planning.retablirStream")}
            </button>
            {#if !s.is_recurring}
              <button
                class="action"
                onclick={() => onConvertir(s, true)}
                disabled={actionEnCours === "confirm"}
              >{$t("planning.rendreRecurrent")}</button>
            {:else}
              <button
                class="action"
                onclick={() => onConvertir(s, false)}
                disabled={!peutPonctuel || actionEnCours === "confirm"}
                title={peutPonctuel ? "" : $t("planning.nonAffilieHint")}
              >{$t("planning.rendrePonctuel")}</button>
            {/if}
            <button class="action danger" onclick={() => onSupprimer(s)}>
              {$t("planning.supprimer")}
            </button>
          </div>
          <button class="action" onclick={() => (segSel = null)}>{$t("planning.retour")}</button>
        </div>
      {:else}
        <div class="modal-bloc">
          <div class="modal-bloc-titre">{$t("planning.jeux")}</div>
          <input
            type="text"
            placeholder={$t("planning.rechercherJeu")}
            bind:value={q}
            oninput={onSearchInput}
          />
          <p class="hint">{$t("planning.glisserHint")}</p>
          <div class="jeux-liste">
            {#if rechEnCours}
              <p class="hint">{$t("commun.chargement")}</p>
            {:else if q.trim().length >= 2 && resultats.length === 0}
              <p class="hint">{$t("planning.aucunResultat")}</p>
            {/if}
            {#each jeuxAffichage as c (c.id)}
              <button
                class="jeu-card"
                draggable="true"
                ondragstart={(e) => dragStartCategorie(e, c)}
                title={c.name}
              >
                {#if jacquette(c.id)}
                  <img class="jeu-img" src={jacquette(c.id)} alt="" />
                {/if}
                <span class="jeu-nom">{c.name}</span>
              </button>
            {/each}
            {#if jeuxAffichage.length === 0 && !rechEnCours && q.trim().length < 2}
              <p class="hint">{$t("planning.aucunJeu")}</p>
            {/if}
          </div>
        </div>
        {#if $broadcaster && !peutPonctuel}
          <p class="hint">{$t("planning.nonAffilieHint")}</p>
        {/if}
      {/if}
    </div>
  </div>
{/if}

<style>
  /* Layout 2 colonnes : planning large + colonne jeux/édition. */
  .planning-layout {
    display: grid;
    grid-template-columns: 1fr 13.125rem;
    gap: 0.8rem;
    flex: 1;
    min-height: 0;
  }
  .planning-main {
    display: flex;
    flex-direction: column;
    gap: 0.5rem;
    min-width: 0;
  }
  .planning-side {
    display: flex;
    flex-direction: column;
    gap: 0.5rem;
    min-width: 0;
    max-height: 60vh;
    overflow-y: auto;
  }
  .planning-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 0.5rem;
    flex-wrap: wrap;
  }
  .nav-groupe {
    display: flex;
    align-items: center;
    gap: 0.3rem;
  }
  .periode {
    font-size: 0.85rem;
    font-weight: 600;
    margin-left: 0.3rem;
    text-transform: capitalize;
  }
  .vues {
    display: flex;
    gap: 0.25rem;
  }
  .vac-info {
    font-size: 0.75rem;
    margin: 0;
    color: var(--message-user-action-color);
  }

  /* Boutons : recette .action globale (app.css) — .actif pour les toggles. */

  /* Bloc = carte visuelle (recette .modal-bloc globale — app.css). */
  .modal-bloc-titre {
    display: flex;
    align-items: center;
    gap: 0.4rem;
  }
  .hint-centre {
    font-size: 0.9rem;
    opacity: 0.6;
    text-align: center;
    margin: 2rem 0;
  }
  .planning-side input[type="text"],
  .planning-side input[type="date"],
  .planning-side input[type="time"] {
    width: 100%;
    background: var(--fond-controle);
    color: var(--texte);
    border: 1px solid var(--bordure-ui);
    padding: 0.3rem 0.45rem;
    font: inherit;
    font-size: 0.85rem;
    color-scheme: dark;
  }
  .planning-side input:focus {
    outline: none;
    border-color: var(--accent-ui);
  }
  .vac-bloc input[type="date"] {
    background: var(--fond-controle);
    color: var(--texte);
    border: 1px solid var(--bordure-ui);
    padding: 0.25rem 0.4rem;
    font: inherit;
    font-size: 0.8rem;
    color-scheme: dark;
  }
  .vac-row {
    display: flex;
    align-items: center;
    gap: 0.4rem;
    flex-wrap: wrap;
  }
  .vac-actions {
    display: flex;
    gap: 0.4rem;
  }

  /* Confirmation inline (même recette que ModerationModal .confirm-bloc). */
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
  .planning-msg {
    font-size: 0.8rem;
    margin: 0;
    color: var(--message-error-color);
  }
  .planning-msg.ok {
    color: var(--message-ok-color);
  }
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

  /* ===== Grille semaine : 7 colonnes × 24 lignes horaires ===== */
  .grille-semaine {
    display: grid;
    grid-template-columns: repeat(7, 1fr);
    gap: 0.25rem;
    min-height: 0;
  }
  .jour {
    display: flex;
    flex-direction: column;
    min-width: 0;
    border: 1px solid var(--bordure-ui);
    border-radius: var(--rayon-petit);
    overflow: hidden;
  }
  .jour-tete {
    font-size: 0.72rem;
    font-weight: 600;
    text-align: center;
    padding: 0.25rem 0.1rem;
    border-bottom: 1px solid var(--bordure-ui);
    background: var(--fond-controle);
    text-transform: capitalize;
    white-space: nowrap;
  }
  .jour-tete.aujourdhui {
    color: var(--accent-ui);
  }
  .jour-corps {
    display: flex;
    flex-direction: column;
    max-height: 54vh;
    overflow-y: auto;
  }
  .cellule {
    position: relative;
    min-height: 1.45rem;
    border-bottom: 1px solid color-mix(in srgb, var(--bordure-ui) 50%, transparent);
    display: flex;
    flex-direction: column;
    gap: 0.15rem;
    padding: 0.1rem;
  }
  .cellule:last-child {
    border-bottom: none;
  }
  .cellule.survol {
    background: color-mix(in srgb, var(--accent-ui) 18%, transparent);
    outline: 1px dashed var(--accent-ui);
    outline-offset: -1px;
  }
  .cellule-heure {
    position: absolute;
    top: 0;
    right: 0.15rem;
    font-size: 0.55rem;
    opacity: 0.35;
    pointer-events: none;
  }
  /* Carte segment (clic = édition ; drag = déplacement si ponctuel ;
     cible de drop jacquette = changer le jeu). */
  .segment {
    display: flex;
    align-items: stretch;
    gap: 0.2rem;
    min-width: 0;
    text-align: left;
    background: color-mix(in srgb, var(--accent-ui) 14%, var(--fond-controle));
    color: var(--texte);
    border: 1px solid color-mix(in srgb, var(--accent-ui) 35%, var(--bordure-ui));
    border-radius: var(--rayon-petit);
    padding: 0.1rem;
    font: inherit;
    cursor: pointer;
    overflow: hidden;
  }
  .segment:hover {
    border-color: var(--accent-ui);
  }
  .segment.recurrent {
    cursor: pointer;
    border-style: dashed;
  }
  .segment.annule {
    opacity: 0.45;
    text-decoration: line-through;
  }
  .segment.survol {
    outline: 1px dashed var(--accent-magenta);
    outline-offset: -1px;
  }
  .seg-img {
    width: 1.3rem;
    height: 1.8rem;
    object-fit: cover;
    border-radius: 2px;
    flex-shrink: 0;
  }
  .seg-texte {
    display: flex;
    flex-direction: column;
    min-width: 0;
    flex: 1;
  }
  .seg-titre {
    font-size: 0.62rem;
    font-weight: 600;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .seg-heure {
    font-size: 0.55rem;
    opacity: 0.6;
    font-variant-numeric: tabular-nums;
  }

  /* ===== Grille mois : calendrier 6×7, chips de segments ===== */
  .grille-mois {
    display: grid;
    grid-template-columns: repeat(7, 1fr);
    gap: 0.25rem;
  }
  .mois-tete {
    font-size: 0.72rem;
    font-weight: 600;
    text-align: center;
    opacity: 0.7;
    text-transform: capitalize;
  }
  .mois-cell {
    display: flex;
    flex-direction: column;
    gap: 0.15rem;
    min-height: 4.2rem;
    border: 1px solid var(--bordure-ui);
    border-radius: var(--rayon-petit);
    padding: 0.2rem;
    min-width: 0;
  }
  .mois-cell.hors-mois {
    opacity: 0.35;
  }
  .mois-cell.aujourdhui {
    border-color: var(--accent-ui);
  }
  .mois-cell.survol {
    background: color-mix(in srgb, var(--accent-ui) 18%, transparent);
    outline: 1px dashed var(--accent-ui);
    outline-offset: -1px;
  }
  .mois-date {
    font-size: 0.68rem;
    font-weight: 600;
    opacity: 0.8;
  }
  .mini-seg {
    font-size: 0.6rem;
    text-align: left;
    background: color-mix(in srgb, var(--accent-ui) 14%, var(--fond-controle));
    color: var(--texte);
    border: 1px solid color-mix(in srgb, var(--accent-ui) 35%, var(--bordure-ui));
    border-radius: var(--rayon-petit);
    padding: 0.08rem 0.25rem;
    cursor: pointer;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-variant-numeric: tabular-nums;
  }
  .mini-seg:hover {
    border-color: var(--accent-ui);
  }
  .mini-seg.annule {
    opacity: 0.45;
    text-decoration: line-through;
  }
  .mini-plus {
    font-size: 0.6rem;
    opacity: 0.5;
  }

  /* ===== Vue liste ===== */
  .liste {
    display: flex;
    flex-direction: column;
    gap: 0.3rem;
  }
  .liste-jour {
    font-size: 0.78rem;
    font-weight: 600;
    text-transform: capitalize;
    opacity: 0.8;
    border-bottom: 1px solid var(--bordure-ui);
    padding: 0.3rem 0 0.15rem;
  }
  .liste-seg {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    text-align: left;
    background: var(--fond-controle);
    color: var(--texte);
    border: 1px solid var(--bordure-ui);
    border-radius: var(--rayon-petit);
    padding: 0.3rem 0.5rem;
    font: inherit;
    cursor: pointer;
  }
  .liste-seg:hover {
    border-color: var(--accent-ui);
  }
  .liste-seg.annule {
    opacity: 0.45;
  }
  .liste-img {
    width: 1.8rem;
    height: 2.4rem;
    object-fit: cover;
    border-radius: 2px;
    flex-shrink: 0;
  }
  .liste-texte {
    display: flex;
    flex-direction: column;
    min-width: 0;
  }
  .liste-titre {
    font-size: 0.82rem;
    font-weight: 600;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .liste-meta {
    font-size: 0.7rem;
    opacity: 0.6;
    font-variant-numeric: tabular-nums;
  }

  /* ===== Colonne droite : jeux + édition ===== */
  .jeux-liste {
    display: flex;
    flex-direction: column;
    gap: 0.3rem;
    overflow-y: auto;
    min-height: 0;
  }
  .jeu-card {
    display: flex;
    align-items: center;
    gap: 0.45rem;
    background: var(--fond-controle);
    color: var(--texte);
    border: 1px solid var(--bordure-ui);
    border-radius: var(--rayon-petit);
    padding: 0.3rem 0.4rem;
    font: inherit;
    font-size: 0.8rem;
    cursor: grab;
    text-align: left;
    min-width: 0;
  }
  .jeu-card:hover {
    border-color: var(--accent-ui);
  }
  .jeu-img {
    width: 1.8rem;
    height: 2.4rem;
    object-fit: cover;
    border-radius: 2px;
    flex-shrink: 0;
  }
  .jeu-nom {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .badge-rec {
    font-size: 0.62rem;
    font-weight: 600;
    padding: 0.05rem 0.35rem;
    border-radius: var(--rayon-petit);
    border: 1px solid color-mix(in srgb, var(--accent-ui) 50%, var(--bordure-ui));
    color: var(--accent-ui);
    white-space: nowrap;
  }
  .badge-annule {
    font-size: 0.62rem;
    font-weight: 600;
    padding: 0.05rem 0.35rem;
    border-radius: var(--rayon-petit);
    border: 1px solid color-mix(in srgb, var(--message-error-color) 50%, var(--bordure-ui));
    color: var(--message-error-color);
    white-space: nowrap;
  }
  .seg-2col {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 0.4rem;
  }
  .seg-2col input {
    width: 100%;
    margin-top: 0.1rem;
  }
  .seg-jeu {
    display: flex;
    align-items: center;
    gap: 0.45rem;
    font-size: 0.8rem;
  }
  .jacquette {
    width: 2rem;
    height: 2.7rem;
    object-fit: cover;
    border: 1px solid var(--bordure-ui);
    border-radius: var(--rayon-petit);
    flex-shrink: 0;
  }
  .seg-actions {
    display: flex;
    gap: 0.3rem;
    flex-wrap: wrap;
  }

  /* Scrollbars discrètes (même recette que les autres onglets). */
  .planning-side::-webkit-scrollbar,
  .jour-corps::-webkit-scrollbar,
  .jeux-liste::-webkit-scrollbar {
    width: 0.375rem;
  }
  .planning-side::-webkit-scrollbar-track,
  .jour-corps::-webkit-scrollbar-track,
  .jeux-liste::-webkit-scrollbar-track {
    background: transparent;
  }
  .planning-side::-webkit-scrollbar-thumb,
  .jour-corps::-webkit-scrollbar-thumb,
  .jeux-liste::-webkit-scrollbar-thumb {
    background: var(--bordure-ui);
    border-radius: 3px;
  }
</style>
