// Guide du streamer — modèle de données des pages du livre feuilletable.
// Une page = une face imprimée (recto ou verso d'une feuille physique).
// Tout le texte est en clés i18n résolues via $t() au rendu — jamais de
// texte en dur (contrat i18n dashboard, fr.json = source de vérité).

/// Blocs de contenu d'une page. Rendus dans l'ordre par GuideLivre :
/// para → <p>, liste → <ul>, etapes → <ol> numérotée, note → encadré
/// « astuce » à liseré doré, sep → fleuron séparateur SVG, sommaire →
/// liste cliquable des chapitres (calculée depuis PAGES, cf. chapitres()).
export type BlocGuide =
  | { type: "para"; cle: string }
  | { type: "liste"; cles: string[] }
  | { type: "etapes"; cles: string[] }
  | { type: "note"; cle: string }
  | { type: "sep" }
  | { type: "sommaire" };

export interface PageGuide {
  /// Id stable — sert aussi de racine des clés i18n : guide.pages.<id>.*.
  id: string;
  /// Clé du titre de page (ex. "guide.pages.principe.titre"). La
  /// couverture porte sa propre mise en page — son titre est rendu par le
  /// bloc dédié, pas par l'en-tête courant.
  titreCle: string;
  /// Clé du titre de chapitre, posée UNIQUEMENT sur la première page du
  /// chapitre (guide.chapitres.<id>). C'est elle qui crée l'entrée du
  /// sommaire — le folio est déduit de la position dans PAGES, jamais
  /// saisi à la main.
  chapitreCle?: string;
  blocs: BlocGuide[];
}

/// Entrée de sommaire : titre de chapitre + folio (1-based) de sa
/// première page.
export interface EntreeSommaire {
  cle: string;
  folio: number;
}

/// Ordre = ordre des faces du livre (face 1 = recto de la feuille 0).
/// GuideLivre complète à un nombre PAIR de faces : si le set finit sur un
/// recto, le verso de la dernière feuille reçoit une page de garde
/// décorative (géré par le composant, pas ici).
/// Contenu volontairement limité aux fonctionnalités vérifiées à 100 %
/// dans le code. Parcours « principal » : l'ordre suit ce qu'un nouveau
/// streamer FAIT, pas l'ordre de la barre latérale. Une face tient
/// environ titre + 1 paragraphe + 4-5 puces + 1 note — au-delà, ça
/// déborde (overflow hidden, pas de scroll).
export const PAGES: PageGuide[] = [
  {
    id: "couverture",
    titreCle: "guide.pages.couverture.titre",
    blocs: [], // mise en page dédiée dans GuideLivre (titre + ornement + hint)
  },
  {
    id: "sommaire",
    titreCle: "guide.pages.sommaire.titre",
    blocs: [{ type: "sommaire" }],
  },

  // ===== Chapitre 1 — Comprendre =====
  {
    id: "principe",
    titreCle: "guide.pages.principe.titre",
    chapitreCle: "guide.chapitres.comprendre",
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

  // ===== Chapitre 2 — Les trois réglages =====
  {
    id: "premierPas",
    titreCle: "guide.pages.premierPas.titre",
    chapitreCle: "guide.chapitres.reglages",
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
    id: "obs",
    titreCle: "guide.pages.obs.titre",
    blocs: [
      { type: "para", cle: "guide.pages.obs.p1" },
      {
        type: "liste",
        cles: [
          "guide.pages.obs.l1",
          "guide.pages.obs.l2",
          "guide.pages.obs.l3",
        ],
      },
      { type: "para", cle: "guide.pages.obs.p2" },
      { type: "note", cle: "guide.pages.obs.note" },
    ],
  },
  {
    id: "modeles",
    titreCle: "guide.pages.modeles.titre",
    blocs: [
      { type: "para", cle: "guide.pages.modeles.p1" },
      {
        type: "liste",
        cles: [
          "guide.pages.modeles.l1",
          "guide.pages.modeles.l2",
          "guide.pages.modeles.l3",
          "guide.pages.modeles.l4",
        ],
      },
      { type: "para", cle: "guide.pages.modeles.p2" },
      { type: "note", cle: "guide.pages.modeles.note" },
    ],
  },

  // ===== Chapitre 3 — Se repérer =====
  {
    id: "cartes",
    titreCle: "guide.pages.cartes.titre",
    chapitreCle: "guide.chapitres.reperer",
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
    id: "barreLaterale",
    titreCle: "guide.pages.barreLaterale.titre",
    blocs: [
      { type: "para", cle: "guide.pages.barreLaterale.p1" },
      {
        type: "liste",
        cles: [
          "guide.pages.barreLaterale.l1",
          "guide.pages.barreLaterale.l2",
          "guide.pages.barreLaterale.l3",
          "guide.pages.barreLaterale.l4",
          "guide.pages.barreLaterale.l5",
          "guide.pages.barreLaterale.l6",
        ],
      },
      { type: "note", cle: "guide.pages.barreLaterale.note" },
    ],
  },
  {
    id: "scenes",
    titreCle: "guide.pages.scenes.titre",
    blocs: [
      { type: "para", cle: "guide.pages.scenes.p1" },
      {
        type: "liste",
        cles: [
          "guide.pages.scenes.l1",
          "guide.pages.scenes.l2",
          "guide.pages.scenes.l3",
          "guide.pages.scenes.l4",
        ],
      },
      { type: "note", cle: "guide.pages.scenes.note" },
    ],
  },

  // ===== Chapitre 4 — Le chat =====
  {
    id: "plateformes",
    titreCle: "guide.pages.plateformes.titre",
    chapitreCle: "guide.chapitres.chat",
    blocs: [
      { type: "para", cle: "guide.pages.plateformes.p1" },
      {
        type: "liste",
        cles: [
          "guide.pages.plateformes.l1",
          "guide.pages.plateformes.l2",
          "guide.pages.plateformes.l3",
          "guide.pages.plateformes.l4",
        ],
      },
      { type: "para", cle: "guide.pages.plateformes.p2" },
      { type: "note", cle: "guide.pages.plateformes.note" },
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

  // ===== Chapitre 5 — Widgets & médias =====
  {
    id: "widgets",
    titreCle: "guide.pages.widgets.titre",
    chapitreCle: "guide.chapitres.widgets",
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
    id: "medias",
    titreCle: "guide.pages.medias.titre",
    blocs: [
      { type: "para", cle: "guide.pages.medias.p1" },
      {
        type: "liste",
        cles: [
          "guide.pages.medias.l1",
          "guide.pages.medias.l2",
          "guide.pages.medias.l3",
        ],
      },
      { type: "note", cle: "guide.pages.medias.note" },
    ],
  },
  {
    id: "carteEdition",
    titreCle: "guide.pages.carteEdition.titre",
    blocs: [
      { type: "para", cle: "guide.pages.carteEdition.p1" },
      {
        type: "liste",
        cles: [
          "guide.pages.carteEdition.l1",
          "guide.pages.carteEdition.l2",
          "guide.pages.carteEdition.l3",
        ],
      },
      { type: "note", cle: "guide.pages.carteEdition.note" },
    ],
  },

  // ===== Chapitre 6 — Cadres =====
  {
    id: "cadres",
    titreCle: "guide.pages.cadres.titre",
    chapitreCle: "guide.chapitres.cadres",
    blocs: [
      { type: "para", cle: "guide.pages.cadres.p1" },
      { type: "para", cle: "guide.pages.cadres.p2" },
      { type: "note", cle: "guide.pages.cadres.note" },
    ],
  },
  {
    id: "cadresModale",
    titreCle: "guide.pages.cadresModale.titre",
    blocs: [
      { type: "para", cle: "guide.pages.cadresModale.p1" },
      {
        type: "etapes",
        cles: [
          "guide.pages.cadresModale.e1",
          "guide.pages.cadresModale.e2",
          "guide.pages.cadresModale.e3",
        ],
      },
      { type: "note", cle: "guide.pages.cadresModale.note" },
    ],
  },

  // ===== Chapitre 7 — Faire vivre le live =====
  {
    id: "alertes",
    titreCle: "guide.pages.alertes.titre",
    chapitreCle: "guide.chapitres.live",
    blocs: [
      { type: "para", cle: "guide.pages.alertes.p1" },
      {
        type: "liste",
        cles: [
          "guide.pages.alertes.l1",
          "guide.pages.alertes.l2",
          "guide.pages.alertes.l3",
        ],
      },
      { type: "note", cle: "guide.pages.alertes.note" },
    ],
  },
  {
    id: "bandeau",
    titreCle: "guide.pages.bandeau.titre",
    blocs: [
      { type: "para", cle: "guide.pages.bandeau.p1" },
      {
        type: "etapes",
        cles: [
          "guide.pages.bandeau.e1",
          "guide.pages.bandeau.e2",
          "guide.pages.bandeau.e3",
        ],
      },
      { type: "note", cle: "guide.pages.bandeau.note" },
    ],
  },
  {
    id: "attente",
    titreCle: "guide.pages.attente.titre",
    blocs: [
      { type: "para", cle: "guide.pages.attente.p1" },
      {
        type: "etapes",
        cles: [
          "guide.pages.attente.e1",
          "guide.pages.attente.e2",
          "guide.pages.attente.e3",
        ],
      },
      { type: "note", cle: "guide.pages.attente.note" },
    ],
  },
  {
    id: "justChatting",
    titreCle: "guide.pages.justChatting.titre",
    blocs: [
      { type: "para", cle: "guide.pages.justChatting.p1" },
      { type: "para", cle: "guide.pages.justChatting.p2" },
      { type: "note", cle: "guide.pages.justChatting.note" },
    ],
  },

  // ===== Chapitre 8 — Passer en direct =====
  {
    id: "previsualisation",
    titreCle: "guide.pages.previsualisation.titre",
    chapitreCle: "guide.chapitres.direct",
    blocs: [
      { type: "para", cle: "guide.pages.previsualisation.p1" },
      {
        type: "liste",
        cles: [
          "guide.pages.previsualisation.l1",
          "guide.pages.previsualisation.l2",
        ],
      },
      { type: "note", cle: "guide.pages.previsualisation.note" },
    ],
  },
  {
    id: "demarrer",
    titreCle: "guide.pages.demarrer.titre",
    blocs: [
      { type: "para", cle: "guide.pages.demarrer.p1" },
      {
        type: "etapes",
        cles: [
          "guide.pages.demarrer.e1",
          "guide.pages.demarrer.e2",
          "guide.pages.demarrer.e3",
        ],
      },
      { type: "note", cle: "guide.pages.demarrer.note" },
    ],
  },

  // ===== Chapitre 9 — Pour aller plus loin =====
  {
    id: "plusLoin",
    titreCle: "guide.pages.plusLoin.titre",
    chapitreCle: "guide.chapitres.plusLoin",
    blocs: [
      { type: "para", cle: "guide.pages.plusLoin.p1" },
      {
        type: "liste",
        cles: [
          "guide.pages.plusLoin.l1",
          "guide.pages.plusLoin.l2",
          "guide.pages.plusLoin.l3",
          "guide.pages.plusLoin.l4",
          "guide.pages.plusLoin.l5",
          "guide.pages.plusLoin.l6",
        ],
      },
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

/// Entrées du sommaire, dans l'ordre du livre : une par page portant
/// `chapitreCle`. Folio = index + 1 (face 1 = couverture).
export function chapitres(): EntreeSommaire[] {
  return PAGES.flatMap((p, i) =>
    p.chapitreCle ? [{ cle: p.chapitreCle, folio: i + 1 }] : [],
  );
}
