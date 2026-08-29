// Store guides d'alignement (dashboard uniquement).
// Pendant le drag/resize d'un widget, des guides visuels apparaissent sur le
// canvas : 4 traits aux bords du widget + labels de distance en px + viseur
// central (cyan → jaune quand proche du centre). Jamais en diffusion.
//
// Portage du guides-store.js du projet référence (RUST_SOS_2026v0.1).
import { writable } from "svelte/store";

export type WidgetRect = {
  x: number;
  y: number;
  width: number;
  height: number;
};

export type GuidesEtat = {
  afficherGuides: boolean;
  visible: boolean;
  widgetRect: WidgetRect | null;
};

export const guidesEtat = writable<GuidesEtat>({
  afficherGuides: true,
  visible: false,
  widgetRect: null,
});

export function afficherGuidesDrag(widgetRect: WidgetRect): void {
  guidesEtat.update((e) => ({ ...e, visible: true, widgetRect }));
}

export function masquerGuides(): void {
  guidesEtat.update((e) => ({ ...e, visible: false, widgetRect: null }));
}

export function toggleAfficherGuides(): void {
  guidesEtat.update((e) => ({ ...e, afficherGuides: !e.afficherGuides }));
}
