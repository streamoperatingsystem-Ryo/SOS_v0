<script lang="ts">
  // CadreSVG — rend un cadre SVG selon le style choisi (dispatch vers cadres/).
  import { genererSVG } from "../cadres/registre";

  interface Props {
    style: string;
    variante?: string;
    largeur: number;
    hauteur: number;
    couleur?: string;
    couleurFin?: string;
    strokeWidth?: number;
    idCadre?: string;
  }

  let {
    style = "carre",
    variante = undefined,
    largeur = 100,
    hauteur = 100,
    couleur = "#ffffff",
    couleurFin = "#000000",
    strokeWidth = 4,
    idCadre = undefined,
  }: Props = $props();

  let svgString = $derived(
    genererSVG(style, variante, largeur, hauteur, couleur, couleurFin, strokeWidth, idCadre)
  );
</script>

{#if svgString}{@html svgString}{/if}
