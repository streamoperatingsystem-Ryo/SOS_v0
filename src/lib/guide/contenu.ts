// Guide du streamer — modèle de données des pages du livre feuilletable.
// Une page = une face imprimée (recto ou verso d'une feuille physique).
// Tout le texte est en clés i18n résolues via $t() au rendu — jamais de
// texte en dur (contrat i18n dashboard, fr.json = source de vérité).

/// Blocs de contenu d'une page. Rendus dans l'ordre par GuideLivre :
/// para → <p>, liste → <ul>, etapes → <ol> numérotée, note → encadré
/// « astuce » à liseré doré, sep → fleuron séparateur SVG.
export type BlocGuide =
  | { type: "para"; cle: string }
  | { type: "liste"; cles: string[] }
  | { type: "etapes"; cles: string[] }
  | { type: "note"; cle: string }
  | { type: "sep" };

export interface PageGuide {
  /// Id stable — sert aussi de racine des clés i18n : guide.pages.<id>.*.
  id: string;
  /// Clé du titre de page (ex. "guide.pages.principe.titre"). La
  /// couverture porte sa propre mise en page — son titre est rendu par le
  /// bloc dédié, pas par l'en-tête courant.
  titreCle: string;
  blocs: BlocGuide[];
}

/// Ordre = ordre des faces du livre (face 1 = recto de la feuille 0).
/// GuideLivre complète à un nombre PAIR de faces : si le set finit sur un
/// recto, le verso de la dernière feuille reçoit une page de garde
/// décorative (géré par le composant, pas ici).
/// Contenu volontairement limité aux fonctionnalités vérifiées à 100 %
/// dans le code — pas de sommaire exhaustif (cf. PLAN-GUIDE-STREAMER §7).
export const PAGES: PageGuide[] = [
  {
    id: "couverture",
    titreCle: "guide.pages.couverture.titre",
    blocs: [], // mise en page dédiée dans GuideLivre (titre + ornement + hint)
  },
  {
    id: "principe",
    titreCle: "guide.pages.principe.titre",
    blocs: [
      { type: "para", cle: "guide.pages.principe.p1" },
      { type: "para", cle: "guide.pages.principe.p2" },
      {
        type: "liste",
        cles: [
          "guide.pages.principe.l1",
          "guide.pages.principe.l2",
          "guide.pages.principe.l3",
        ],
      },
      { type: "note", cle: "guide.pages.principe.note" },
    ],
  },
  {
    id: "premierPas",
    titreCle: "guide.pages.premierPas.titre",
    blocs: [
      { type: "para", cle: "guide.pages.premierPas.p1" },
      {
        type: "etapes",
        cles: [
          "guide.pages.premierPas.e1",
          "guide.pages.premierPas.e2",
          "guide.pages.premierPas.e3",
          "guide.pages.premierPas.e4",
        ],
      },
      { type: "note", cle: "guide.pages.premierPas.note" },
    ],
  },
  {
    id: "cartes",
    titreCle: "guide.pages.cartes.titre",
    blocs: [
      { type: "para", cle: "guide.pages.cartes.p1" },
      { type: "para", cle: "guide.pages.cartes.intro" },
      {
        type: "liste",
        cles: [
          "guide.pages.cartes.l1",
          "guide.pages.cartes.l2",
          "guide.pages.cartes.l3",
        ],
      },
      { type: "para", cle: "guide.pages.cartes.p2" },
      { type: "note", cle: "guide.pages.cartes.note" },
    ],
  },
  {
    id: "chat",
    titreCle: "guide.pages.chat.titre",
    blocs: [
      { type: "para", cle: "guide.pages.chat.p1" },
      {
        type: "liste",
        cles: [
          "guide.pages.chat.l1",
          "guide.pages.chat.l2",
          "guide.pages.chat.l3",
        ],
      },
      { type: "para", cle: "guide.pages.chat.p2" },
      { type: "note", cle: "guide.pages.chat.note" },
    ],
  },
  {
    id: "widgets",
    titreCle: "guide.pages.widgets.titre",
    blocs: [
      { type: "para", cle: "guide.pages.widgets.p1" },
      {
        type: "liste",
        cles: [
          "guide.pages.widgets.l1",
          "guide.pages.widgets.l2",
          "guide.pages.widgets.l3",
        ],
      },
    ],
  },
  {
    id: "cadres",
    titreCle: "guide.pages.cadres.titre",
    blocs: [
      { type: "para", cle: "guide.pages.cadres.p1" },
      { type: "para", cle: "guide.pages.cadres.p2" },
      { type: "note", cle: "guide.pages.cadres.note" },
    ],
  },
  {
    id: "fin",
    titreCle: "guide.pages.fin.titre",
    blocs: [
      { type: "sep" },
      { type: "para", cle: "guide.pages.fin.p1" },
      { type: "sep" },
    ],
  },
];
