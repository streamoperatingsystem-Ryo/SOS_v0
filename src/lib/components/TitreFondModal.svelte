<script lang="ts">
  // TitreFondModal — modale d'édition des titres du fond de l'application.
  // Ouverte par le bouton "Titre du fond" (CarteEdition, section fond).
  // Jusqu'à 4 titres simultanés (haut/bas/gauche/droite — intérieur du fond,
  // latéraux en vertical), style partagé. Le corps du form est dans
  // TitreEditForm (partagé avec TitreModal).
  import { titreFondModalOpen } from "../stores/ui";
  import { sceneStore, setBgTitre } from "../stores/scene";
  import { gradientCadre } from "../cadres/registre";
  import { POLICE_TITRE_DEFAUT, TAILLE_TITRE_DEFAUT, ESPACEMENT_TITRE_DEFAUT } from "../fonts";
  import Modal from "./Modal.svelte";
  import TitreEditForm from "./TitreEditForm.svelte";
  import { t } from "../stores/langue";

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

  // Gradient du cadre unifié pour l'aperçu.
  let cadre = $derived($sceneStore.cadreWidget ?? null);
  let grad = $derived(gradientCadre(cadre));

  // Titre existant (bouton Tout supprimer) = au moins un texte non vide.
  let titreExistant = $derived(
    !!($sceneStore.bgTitreHaut || $sceneStore.bgTitreBas || $sceneStore.bgTitreGauche || $sceneStore.bgTitreDroite)
  );

  // Initialiser les valeurs locales quand la modale s'ouvre.
  $effect(() => {
    if ($titreFondModalOpen) {
      haut = $sceneStore.bgTitreHaut ?? "";
      bas = $sceneStore.bgTitreBas ?? "";
      gauche = $sceneStore.bgTitreGauche ?? "";
      droite = $sceneStore.bgTitreDroite ?? "";
      police = $sceneStore.bgTitrePolice ?? POLICE_TITRE_DEFAUT;
      taille = $sceneStore.bgTitreTaille ?? TAILLE_TITRE_DEFAUT;
      gras = $sceneStore.bgTitreGras ?? false;
      italique = $sceneStore.bgTitreItalique ?? false;
      souligne = $sceneStore.bgTitreSouligne ?? false;
      espacement = $sceneStore.bgTitreEspacement ?? ESPACEMENT_TITRE_DEFAUT;
      contourCouleur = $sceneStore.bgTitreContourCouleur ?? "#ffffff";
      contourEpaisseur = $sceneStore.bgTitreContourEpaisseur ?? 2;
      couleur = $sceneStore.bgTitreCouleur ?? undefined;
      couleurFin = $sceneStore.bgTitreCouleurFin ?? undefined;
    }
  });

  function onValider() {
    // Champ vide = pas de titre à cette position (undefined retire la clé).
    void setBgTitre({
      bgTitreHaut: haut.trim() || undefined,
      bgTitreBas: bas.trim() || undefined,
      bgTitreGauche: gauche.trim() || undefined,
      bgTitreDroite: droite.trim() || undefined,
      bgTitrePolice: police,
      bgTitreTaille: Math.max(12, Math.min(72, Math.round(taille))),
      bgTitreGras: gras,
      bgTitreItalique: italique,
      bgTitreSouligne: souligne,
      bgTitreEspacement: Math.max(-2, Math.min(20, Math.round(espacement))),
      bgTitreContourCouleur: contourCouleur,
      bgTitreContourEpaisseur: Math.max(0, Math.min(6, Math.round(contourEpaisseur))),
      bgTitreCouleur: couleur || undefined,
      bgTitreCouleurFin: couleur ? couleurFin || undefined : undefined,
    });
    titreFondModalOpen.set(false);
  }

  function onSupprimer() {
    void setBgTitre({
      bgTitreHaut: undefined,
      bgTitreBas: undefined,
      bgTitreGauche: undefined,
      bgTitreDroite: undefined,
      bgTitrePolice: undefined,
      bgTitreTaille: undefined,
      bgTitreGras: undefined,
      bgTitreItalique: undefined,
      bgTitreSouligne: undefined,
      bgTitreEspacement: undefined,
      bgTitreContourCouleur: undefined,
      bgTitreContourEpaisseur: undefined,
      bgTitreCouleur: undefined,
      bgTitreCouleurFin: undefined,
    });
    titreFondModalOpen.set(false);
  }

  function onAnnuler() {
    titreFondModalOpen.set(false);
  }
</script>

{#if $titreFondModalOpen}
  <Modal
    title={$t("titreModal.titreFond")}
    onClose={onAnnuler}
    crochet="var(--crochet-orange)"
    maxWidth="35rem"
    hint={$t("titreModal.hintFond")}
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
      />
      <div class="modal-actions">
        {#if titreExistant}
          <button class="action danger" onclick={onSupprimer}>{$t("commun.toutSupprimer")}</button>
        {/if}
        <button class="action" onclick={onAnnuler}>{$t("commun.annuler")}</button>
        <button class="action primary" onclick={onValider}>{$t("commun.valider")}</button>
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
</style>
