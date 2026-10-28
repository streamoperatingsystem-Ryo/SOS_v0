# Résolution canvas — miroir OBS et rescale des scènes

> `canvasW` / `canvasH` ne sont **pas** une propriété de scène : c'est le
> miroir de la résolution de sortie OBS. Une scène rendue plus large que la
> source navigateur OBS est coupée à droite et en bas.

## Chaîne (une seule vérité)

```
OBS WebSocket (GetVideoSettings → baseWidth/baseHeight)
  └─ obs_connect (lib.rs)         → config::sauver_obs_canvas(obs_canvas.json)
                                   → scenes::adapter_canvas(scène courante)
                                   → save_current (fichier + snapshot WS)
  └─ scenes::appliquer_canvas_obs → à CHAQUE chargement de scène
       (boot, création, bascule, import, preset) : applique obs_canvas.json
```

`obs_canvas.json` = `{ "w": 1360, "h": 768 }`, global (pas par scène).

## Espace de conception (`canvasDesignW/H`)

La scène mémorise la résolution dans laquelle elle a été **dessinée**
(`canvasDesignW/H`, `Option<u32>`, absent des vieilles scènes → migré à la
première adaptation). Les coordonnées stockées restent exprimées dans le
**canvas courant** ; l'espace de conception sert de référence pour les
conversions.

Conséquence : un changement de résolution n'est jamais cumulatif. `adapter_canvas`
**revient d'abord dans l'espace de conception** (inverse de la transformation
courante), puis projette sur le nouveau canvas. `1920 → 1360 → 1920` retrouve
exactement le layout d'origine (f64 : écart < 1e-9 px ; champs entiers :
±1 px au pire, `48 → 34 → 48` exact).

## Règle de rescale (2026-10-28)

`scenes::adapter_canvas(scene, w, h)` :

1. **Mêmes dimensions** → aucune modification.
2. **Écart ≤ 0,1 %** (`EPSILON_CANVAS`) sur les deux axes → **rien** n'est
   touché (ni dimensions, ni géométrie). C'est uniquement du bruit
   d'arrondi OBS (1360 vs 1361 px) : mettre à jour les dimensions sans la
   géométrie casserait l'invariant « coordonnées = canvas courant ».
   ⚠️ Ce n'est **pas** un garde-fou anti-dérive : c'est `canvasDesignW/H`
   qui l'assure. Un seuil large (ex. 10 %) laisserait le canvas en
   désaccord avec la source OBS (bords coupés) et casserait l'invariant.
3. **Au-delà** → retour à l'espace de conception, puis projection :
   - facteur **uniforme** `f = min(w/design_w, h/design_h)` ;
   - contenu `design × f` **centré** dans le nouveau canvas (letterbox) :
     `dx = (w − design_w·f)/2`, `dy = (h − design_h·f)/2` ;
   - widgets : `x' = x·f + dx`, `y' = y·f + dy`, `largeur' = largeur·f`,
     `hauteur' = hauteur·f` (**x/y sont des centres** de widget) ;
   - modules HUD : mêmes formules, mais x/y sont le **coin haut-gauche** ;
   - grandeurs en px canvas : `mediaOffsetX/Y`, `mediaFlou`, `taillePolice`,
     `titreTaille`, `titreContourEpaisseur`, `titreEspacement`,
     `srTaillePolice`, `srSegBordureEpaisseur`, `srSegBordureRayon`,
     `srHauteurLigne`, `bgOffsetX/Y`, `bgFlou`, `bgTitreTaille`,
     `bgTitreContourEpaisseur`, `bgTitreEspacement`, `modules.*.taille` ;
   - **jamais** les valeurs sans dimension : `mediaZoom`, `mediaRot`,
     `rotateX/Y`, `mediaLum/Contraste/Teinte/Pixel`, `bgZoom`, `bgRot`,
     `bgPixel`, durées (`mediaSlidesSec`, `bgSlidesSec`), compteurs
     (`srNbLignes`, `srMetronomeBpm`).
   - `0` reste `0` (`titreContourEpaisseur: 0` = contour désactivé).

Conséquences assumées :

- Un passage 16:9 → 4:3 ne déforme **rien** : le contenu est réduit au
  facteur min et centré, avec des bandes.
- Le rescale n'est **plus destructif** : l'espace de conception est conservé,
  donc revenir à la résolution d'origine restaure le layout.
- Un rescale appliqué au chargement n'est écrit qu'à la sauvegarde suivante
  (aucune écriture surprise au boot) ; le calcul est déterministe, donc un
  rechargement sans sauvegarde donne exactement le même résultat.
- Une édition faite dans le canvas courant (ex. 1360) est ramenée
  proportionnellement dans l'espace de conception : elle survit à un
  changement de résolution.

## Réparer une scène déjà décalée

Symptôme : le canvas affiché est petit (ex. 1360×768) mais les widgets
débordent à droite/en bas — ils gardent les coordonnées d'un ancien canvas
plus grand.

Réparation : **remettre `canvasW/canvasH` à l'ancienne résolution** dans le
fichier de scène (`%APPDATA%\com.streamos.v0\scenes\<id>.json`), **app
fermée**, puis relancer : `appliquer_canvas_obs` rescale au chargement avec
le code de production (aucune formule parallèle). Ne pas rescale à la main :
la moindre divergence avec `echelle_scene` produirait un layout faux.

## Vérification

`cargo test -p streamos-v0 scenes::tests` couvre : 1920×1080 → 1360×768
(facteur, centrage, `titreTaille`, `0` préservé, espace de conception
mémorisé), le bruit OBS (1919×1080 = no-op), l'idempotence, le letterbox 4:3,
les modules HUD, **l'aller-retour exact** 1920 → 1360 → 1920 (et 4:3), et la
conservation de l'espace de conception.
