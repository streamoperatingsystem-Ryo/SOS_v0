<script lang="ts">
  // CadreSVG — rend un cadre SVG selon le style choisi (dispatch vers cadres/).
  import { genererSVG } from "../cadres/registre";

  interface Props {
    style: string;
    largeur: number;
    hauteur: number;
    couleur?: string;
    couleurFin?: string;
    strokeWidth?: number;
    idCadre?: string;
    gradientAngle?: number;
    animer?: boolean;
  }

  let {
    style = "carre",
    largeur = 100,
    hauteur = 100,
    couleur = "#ffffff",
    couleurFin = "#000000",
    strokeWidth = 4,
    idCadre = undefined,
    gradientAngle = 135,
    animer = false,
  }: Props = $props();

  let svgString = $derived(
    genererSVG(style, largeur, hauteur, couleur, couleurFin, strokeWidth, idCadre, gradientAngle, animer)
  );
</script>

{#if svgString}{@html svgString}{/if}
