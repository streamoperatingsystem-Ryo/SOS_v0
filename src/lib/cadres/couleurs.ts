// Helpers couleur partagés par les renderers de cadres SVG — les tons
// internes (clair, pâle, profond, accent) se dérivent de couleur/couleurFin
// utilisateur. Aucune table de couleurs figées.

/// "#rrggbb" valide ?
export function hexValide(c: string | undefined | null): c is string {
  return typeof c === "string" && /^#[0-9a-f]{6}$/i.test(c);
}

/// Couleur valide ou fallback.
export function couleurOu(c: string | undefined | null, fallback: string): string {
  return hexValide(c) ? c : fallback;
}

function parseHex(hex: string): [number, number, number] {
  const n = parseInt(hex.slice(1), 16);
  return [(n >> 16) & 0xff, (n >> 8) & 0xff, n & 0xff];
}

function canal(v: number): string {
  return Math.round(Math.min(255, Math.max(0, v))).toString(16).padStart(2, "0");
}

/// Assombrit un hex d'un ratio (0 = inchangé, 1 = noir).
export function assombrir(hex: string, ratio: number): string {
  const [r, g, b] = parseHex(couleurOu(hex, "#000000"));
  const k = 1 - Math.min(1, Math.max(0, Number(ratio) || 0));
  return `#${canal(r * k)}${canal(g * k)}${canal(b * k)}`;
}

/// Éclaircit un hex d'un ratio (0 = inchangé, 1 = blanc).
export function eclaircir(hex: string, ratio: number): string {
  const [r, g, b] = parseHex(couleurOu(hex, "#ffffff"));
  const k = Math.min(1, Math.max(0, Number(ratio) || 0));
  return `#${canal(r + (255 - r) * k)}${canal(g + (255 - g) * k)}${canal(b + (255 - b) * k)}`;
}

/// Rampe « liserés » pour le dégradé principal d'un cadre : dominante
/// sombre avec deux fins passages clairs — la couleur s'exprime par
/// touches au lieu d'un grand dégradé uniforme (le simple 2-stops
/// écrasait le bord en un seul ton). Deux liserés de tons différents :
/// « éclat » (vive éclaircie 45 %) vers 33 %, puis vive pure vers 68 %.
/// Miroir vanilla : stopsLiseres (diffusion.html + chat-popout.html).
export function stopsLiseres(sombre: string, vive: string): string {
  const eclat = eclaircir(vive, 0.45);
  return (
    `<stop offset="0%" stop-color="${sombre}"/>` +
    `<stop offset="30%" stop-color="${sombre}"/>` +
    `<stop offset="33%" stop-color="${eclat}"/>` +
    `<stop offset="36%" stop-color="${sombre}"/>` +
    `<stop offset="64%" stop-color="${sombre}"/>` +
    `<stop offset="68%" stop-color="${vive}"/>` +
    `<stop offset="72%" stop-color="${sombre}"/>` +
    `<stop offset="100%" stop-color="${sombre}"/>`
  );
}

/// Convertit un angle en degrés (0-360) en coordonnées x1/y1/x2/y2 (0-100%)
/// pour un linearGradient SVG. 0° = gauche→droite, 90° = haut→bas,
/// 135° = diagonal haut-gauche→bas-droite (défaut).
export function angleVersCoords(angle: number): { x1: number; y1: number; x2: number; y2: number } {
  const rad = (angle * Math.PI) / 180;
  const dx = Math.cos(rad);
  const dy = Math.sin(rad);
  // Normaliser vers 0-100% : projette le vecteur de direction sur le carré unitaire.
  const scale = 50 / Math.max(Math.abs(dx), Math.abs(dy), 0.001);
  const cx = 50, cy = 50;
  return {
    x1: Math.round(cx - dx * scale),
    y1: Math.round(cy - dy * scale),
    x2: Math.round(cx + dx * scale),
    y2: Math.round(cy + dy * scale),
  };
}
