# Bandeau premier message

> Section déplacée depuis `AGENTS.md` — les règles figées (CONTRAT/RÈGLE/anti-patterns) restent dans `AGENTS.md`.


## Bandeau premier message — teinte cadre + mascotte animée (2026-10-04)

> Correctifs sur l'overlay `.bandeau-pm-*` (z 9998, `diffusion.html` —
> barre pleine largeur bas/haut, couleurs dérivées du cadre widget).

- **Teinte = extrémité LA PLUS LUMINEUSE du gradient cadre** :
  `bpmResoudreCouleurs` compare `couleur`/`couleurFin` via `bpmLuminance`
  (rec.601, 0-255) et choisit la plus lumineuse comme primaire `p`. Raison :
  le cadre défaut StreamOS est **noir→violet** (`#000000`→`#8b5cf6`) —
  prendre `couleur` rendait la barre entière, le badge, le liseré et le
  glow Streamy noirs (invisibles). `.p` sert aussi à la couleur de texte
  des alertes/commandes (même résolution, cf. section alertes) — elles
  gagnent la même lisibilité.
- **Pop d'entrée de la mascotte** (`bpm-streamy-pop`, 0,7 s avec dépassement,
  délai 0,35 s pour surgir après la glisse de la barre) : l'animation
  n'existe que sous `.bandeau-pm-root.visible` — retirer la classe la
  réarme pour le prochain message, zéro JS. ⚠️ `scaleX(-1)` (miroir) doit
  être **répété dans chaque keyframe** — un keyframe `transform` remplace
  le transform statique. Variante `bpm-streamy-pop-haut` inversée pour
  `pos-haut` (Streamy est sous la barre, elle descend ; transform-origin
  50% 0%).
- **Limite connue** : un second premier message pendant que le bandeau est
  déjà visible ne relance PAS le pop (`.visible` déjà posée, la classe
  n'est pas re-appliquée dans `playBandeauPM`).
