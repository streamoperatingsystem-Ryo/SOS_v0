<script lang="ts">
  // TitreModal — modale d'édition des titres d'un widget.
  // Ouverte par double-clic sur un widget (Widget.svelte) : le bord le plus
  // proche du clic est focus à l'ouverture. Jusqu'à 4 titres simultanés
  // (haut/bas/gauche/droite), style partagé. Le corps du form est dans
  // TitreEditForm (partagé avec TitreFondModal).
  import { titreModalOpen } from "../stores/ui";
  import { sceneStore, setWidgetTitre } from "../stores/scene";
  import { gradientCadre } from "../cadres/registre";
  import { POLICE_TITRE_DEFAUT, TAILLE_TITRE_DEFAUT, ESPACEMENT_TITRE_DEFAUT } from "../fonts";
  import Modal from "./Modal.svelte";
  import TitreEditForm from "./TitreEditForm.svelte";
  import { t } from "../stores/langue";

  let cible = $derived($titreModalOpen);
  let widget = $derived(
    cible ? $sceneStore.widgets.find((w) => w.id === cible.id) ?? null : null
  );

  // Valeurs locales éditables (pas de commit en live — seulement au Valider).
  let haut = $state("");
  let bas = $state("");
  let gauche = $state("");
  let droite = $state("");
  let police = $state(POLICE_TITRE_DEFAUT);
  let taille = $state(TAILLE_TITRE_DEFAUT);
  let gras = $state(false);
  let italique = $state(false);
  let souligne = $state(false);
  let espacement = $state(ESPACEMENT_TITRE_DEFAUT);
  let contourCouleur = $state("#ffffff");
  let contourEpaisseur = $state(2);
  // Dégradé personnalisé du texte (undefined = auto, suit le cadre).
  let couleur = $state<string | undefined>(undefined);
  let couleurFin = $state<string | undefined>(undefined);

  // Gradient du cadre actif pour l'aperçu.
  let cadre = $derived($sceneStore.cadreWidget ?? null);
  let grad = $derived(gradientCadre(cadre));

  // Titre existant (bouton Tout supprimer) = au moins un texte non vide.
  let titreExistant = $derived(
    !!(widget && (widget.titreHaut || widget.titreBas || widget.titreGauche || widget.titreDroite))
  );

  // Initialiser les valeurs locales quand la modale s'ouvre.
  $effect(() => {
    if (widget) {
      haut = widget.titreHaut ?? "";
      bas = widget.titreBas ?? "";
      gauche = widget.titreGauche ?? "";
      droite = widget.titreDroite ?? "";
      police = widget.titrePolice ?? POLICE_TITRE_DEFAUT;
      taille = widget.titreTaille ?? TAILLE_TITRE_DEFAUT;
      gras = widget.titreGras ?? false;
      italique = widget.titreItalique ?? false;
      souligne = widget.titreSouligne ?? false;
      espacement = widget.titreEspacement ?? ESPACEMENT_TITRE_DEFAUT;
      contourCouleur = widget.titreContourCouleur ?? "#ffffff";
      contourEpaisseur = widget.titreContourEpaisseur ?? 2;
      couleur = widget.titreCouleur ?? undefined;
      couleurFin = widget.titreCouleurFin ?? undefined;
    }
  });

  function onValider() {
    if (!cible) return;
    void setWidgetTitre(cible.id, {
      // Champ vide = pas de titre à cette position (undefined retire la clé).
      titreHaut: haut.trim() || undefined,
      titreBas: bas.trim() || undefined,
      titreGauche: gauche.trim() || undefined,
      titreDroite: droite.trim() || undefined,
      titrePolice: police,
      titreTaille: Math.max(12, Math.min(72, Math.round(taille))),
      titreGras: gras,
      titreItalique: italique,
      titreSouligne: souligne,
      titreEspacement: Math.max(-2, Math.min(20, Math.round(espacement))),
      titreContourCouleur: contourCouleur,
      titreContourEpaisseur: Math.max(0, Math.min(6, Math.round(contourEpaisseur))),
      titreCouleur: couleur || undefined,
      titreCouleurFin: couleur ? couleurFin || undefined : undefined,
    });
    titreModalOpen.set(null);
  }

  function onSupprimer() {
    if (!cible) return;
    void setWidgetTitre(cible.id, {
      titreHaut: undefined,
      titreBas: undefined,
      titreGauche: undefined,
      titreDroite: undefined,
      titrePolice: undefined,
      titreTaille: undefined,
      titreGras: undefined,
      titreItalique: undefined,
      titreSouligne: undefined,
      titreEspacement: undefined,
      titreContourCouleur: undefined,
      titreContourEpaisseur: undefined,
      titreCouleur: undefined,
      titreCouleurFin: undefined,
    });
    titreModalOpen.set(null);
  }

  function onAnnuler() {
    titreModalOpen.set(null);
  }
</script>

{#if cible && widget}
  <Modal
    title={$t("titreModal.titreWidget")}
    onClose={onAnnuler}
    crochet="var(--message-error-color)"
    maxWidth="560px"
    hint={$t("titreModal.hintWidget")}
  >
    <div class="titre-modal-body">
      <TitreEditForm
        bind:haut
        bind:bas
        bind:gauche
        bind:droite
        bind:police
        bind:taille
        bind:gras
        bind:italique
        bind:souligne
        bind:espacement
        bind:contourCouleur
        bind:contourEpaisseur
        bind:couleur
        bind:couleurFin
        {grad}
        focusPosition={cible.position ?? ""}
      />
      <div class="modal-actions">
        {#if titreExistant}
          <button class="modal-action danger" onclick={onSupprimer}>{$t("commun.toutSupprimer")}</button>
        {/if}
        <button class="modal-action" onclick={onAnnuler}>{$t("commun.annuler")}</button>
        <button class="modal-action primary" onclick={onValider}>{$t("commun.valider")}</button>
      </div>
    </div>
  </Modal>
{/if}

<style>
  .titre-modal-body {
    padding: 1rem;
    display: flex;
    flex-direction: column;
    gap: 0.6rem;
    overflow-y: auto;
  }
  .modal-actions {
    justify-content: flex-end;
  }
</style>
