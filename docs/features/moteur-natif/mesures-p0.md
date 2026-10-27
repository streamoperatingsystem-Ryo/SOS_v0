# Mesures P0 — baseline OBS et verdict go/no-go

> **Lots 0.1 (protocole) et 0.3 (verdict).** Les relevés du **2026-10-27**
> sont consignés dans « Feuille de relevé » ; le protocole reste la référence
> pour toute nouvelle machine ou tout nouveau spike.
> ⚠️ Une case non mesurée vaut `N/A`, **jamais `0`**, jamais une estimation.

## But

Le moteur natif n'a qu'une justification mesurable : consommer **moins
qu'OBS à fonctionnalités égales**. Ce lot produit la référence contre laquelle
cette promesse sera jugée, *avant* d'écrire le moteur.

**Gate P0 (plan §9.4)** : go si **2 cibles sur 3** sont tenues, sinon arrêt et
réévaluation.

| # | Cible | Seuil | Mesure |
|---|---|---|---|
| 1 | CPU | ≤ 0,7 × OBS | PDH, moyenne + p95 sur 29 min |
| 2 | RAM | ≤ 0,5 × OBS | RSS, moyenne + p95 sur 29 min |
| 3 | Drops | = 0 | frames perdues / total présentées |

La latence capture→encode est **relevée mais non bloquante** : elle informe le
gouverneur, elle ne décide pas du go.

## Deux mesures distinctes — ne pas les confondre

| Mesure | Comparaison | Statut |
|---|---|---|
| **P0-a — pipeline seul** | OBS : **une** capture d'écran, **sans** browser source ↔ spike : capture + NVENC | réalisable **maintenant** |
| **P0-b — fonctionnalités équivalentes** | OBS avec chat/alertes (browser sources) ↔ moteur natif avec compositeur | **impossible avant P2** (compositeur) |

> ⚠️ Le spike actuel ne compose rien : il capture un moniteur et encode.
> Comparer ce spike à OBS **avec** browser sources serait biaisé en notre
> faveur. **P0-a valide la chaîne ; P0-b décide du go/no-go.**
> Tant que P0-b n'est pas mesurable, le verdict reste **provisoire**.

## Scène de référence

### P0-a — pipeline seul

Une seule source, identique des deux côtés :

- 1 capture d'écran (moniteur), résolution native ;
- **aucune** browser source, aucune caméra, aucun overlay ;
- enregistrement local H.264, 30 min.

### P0-b — fonctionnalités équivalentes (après P2)

- 1 capture de jeu, 1 caméra, 1 chat, 1 alerte ;
- OBS **avec** ses browser sources ;
- 1080p60, 30 min d'enregistrement local.

## Outillage

| Grandeur | Outil | Remarque |
|---|---|---|
| CPU, RSS | `tools/mesure-p0.ps1` | **même script des deux côtés** (PDH anglais, CSV invariant) |
| GPU | PDH `\GPU Engine(*)\Utilization Percentage`, agrégé par PID | vendor-agnostic (déjà utilisé par `conso.rs`) |
| Drops | OBS : fenêtre **Stats** (`frames missed due to rendering lag`, `skipped frames due to encoding lag`) ; spike : ses propres compteurs | métrique commune = frames perdues / total |
| Latence | PresentMon | **proxy** : mesure la présentation du jeu, pas la chaîne capture→encode. À documenter comme tel |

Le moteur n'étant pas écrit, le « spike » du lot 0.2 est la seule source de
chiffres côté Rust. Les deux côtés sont mesurés **avec les mêmes outils**.

## Protocole (reproductible)

1. **Redémarrer la machine.** Fermer navigateurs, Discord, mises à jour, tout
   ce qui consomme. Noter ce qui tourne malgré tout.
2. Charger la scène de référence, vérifier 1080p60.
3. Démarrer l'enregistrement local, **30 min sans interruption**.
4. **Ignorer les 60 premières secondes** (montée en charge, compilation de
   shaders, premier remplissage des files).
5. Relever **moyenne et p95** sur les 29 minutes restantes.
6. **Répéter 3 fois**, retenir la **médiane** des 3 runs (pas la meilleure :
   une machine qui throttole doit le montrer).
7. Rejouer le protocole à l'identique avec le spike Rust (lot 0.2).

Les 30 minutes ne sont pas décoratives : c'est la durée nécessaire pour que la
dérive thermique et la fragmentation des files se manifestent.

## Commandes (P0-a)

```powershell
# Spike — OBLIGATOIREMENT en release (le debug tombe à ~12 fps)
cd src-tauri
cargo build --release -p sos-moteur-natif --example encode_nvenc

$spike = Start-Process -FilePath "target\release\examples\encode_nvenc.exe" `
  -ArgumentList '0','C:\temp\spike.h264','0','60','6000','h264_nvenc','1800' `
  -PassThru

# Mesure — même harnais que OBS, TOUJOURS en -Arbre
powershell -ExecutionPolicy Bypass -File ..\tools\mesure-p0.ps1 `
  -IdProc $spike.Id -Arbre -DureeSecondes 1800 -Sortie ..\mesures\spike-run1.csv

# Côté OBS (pendant l'enregistrement, scène de référence chargée)
$obs = (Get-Process obs64).Id
powershell -ExecutionPolicy Bypass -File tools\mesure-p0.ps1 `
  -IdProc $obs -Arbre -DureeSecondes 1800 -Sortie mesures\obs-run1.csv
```

> ⚠️ **`-Arbre` n'est pas optionnel.** Sans lui, le harnais ne mesure que le
> PID racine : il ignore le `ffmpeg` enfant du spike (tout l'encodage) et les
> `obs-browser-page.exe` d'OBS. C'est l'erreur qui a produit le tableau
> indicatif erroné du 2026-10-27 (42 Mo de RAM au lieu de 183 Mo).
>
> ⚠️ En debug, la conversion NV12 prend ~77 ms/frame : la mesure ne veut rien
> dire. Toujours `--release`.

## Feuille de relevé — relevés du 2026-10-27

Une ligne par machine (matrice §6.4 du plan : « PC principal », « PC ancien »,
puis toute machine supplémentaire).

| Machine | GPU | Pilote | Capture | OBS |
|---|---|---|---|---|
| PC principal | NVIDIA GTX 1070 Ti (iGPU Intel UHD 750 présente) | NVIDIA 32.0.15.8266 (2026-06-09) | WGC, moniteur 0 = `\\.\DISPLAY1` 1360×768 | 32.2.2 |
| PC ancien | `N/A` — non mesuré | | | |

Machine : Intel i9-11900K (8 cœurs / 16 threads), 31,8 Go de RAM, Windows.
Côté OBS : scène `SCENES P0` = **une seule** capture d'écran (aucune browser
source active), base et sortie **1360×768**, 60 fps, NVENC H.264 CBR 6000,
p5 / hq / lookahead 8 / b-frames 2. Côté spike : `encode_nvenc`, moniteur 0,
1360×768, 60 fps, 6000 kbps, `h264_nvenc` p4 / ll / sans b-frames.

Puis, par machine et par côté (médiane de **6 blocs de 5 min**, 60 premières
secondes ignorées ; `p95` calculé sur l'ensemble des échantillons retenus) :

| Machine | Côté | CPU moy | CPU p95 | RSS moy | RSS p95 | GPU moy | Drops | Latence p95 | Verdict |
|---|---|---|---|---|---|---|---|---|---|
| PC principal | OBS | 0,75 % | 1,25 % | 849,5 Mo | 871,7 Mo | 49,2 % | 0 (Stats 0/0) | `N/A` | référence |
| PC principal | spike | 2,07 % | 2,62 % | 183,5 Mo | 187,9 Mo | 32,3 % | 0 structurel, 188 frames en retard | `N/A` | voir verdict |
| PC principal | spike zéro-copie (2026-10-28) | 0,05 % | 0,20 % | 59,6 Mo | 59,8 Mo | 28,8 % | non mesurable, 14 en retard (0,013 %) | `N/A` | voir verdict révisé |
| PC ancien | OBS | `N/A` | | | | | | | non mesuré |
| PC ancien | spike | `N/A` | | | | | | | non mesuré |

Stabilité intra-run (contrôle de dérive, 6 tranches de 5 min) :

- OBS : CPU 0,70–0,84 %, RAM 847,6–864,6 Mo (première tranche = remplissage
  des buffers), GPU 48,1–52,2 % ;
- spike : CPU 2,02–2,21 %, RAM 182,2–184,0 Mo, GPU 25,5–33,7 %.

Aucune tendance : ni throttling thermique, ni fragmentation de file, ni
croissance mémoire sur 30 min. C'est ce qui autorise la lecture par blocs.

### Relevé du 2026-10-28 — spike zéro-copie (lot 0.2g-γ)

Même protocole, même machine, même scène : seule la chaîne côté spike change.
`nvenc_encode` (SDK NVENC) consomme directement une texture NV12 D3D11
**persistante**, réécrite à chaque frame par le video processor ; plus de
readback BGRA, plus de conversion CPU, plus de pipe ffmpeg (aucun processus
enfant).

- 108 000 frames en 1 799 987 ms = **60,0 fps exact**, 872 Mo (3 875 kbps) ;
- **89 989/108 000 frames capturées (83,3 %)** — l'ancien spike était à
  83,2 % : conditions comparables ; 18 011 frames sans mise à jour
  (ré-encodage du contenu précédent), **14 en retard** (0,013 %) ;
- décodage intégral du flux : `ffmpeg -v error … -f null -` → exit 0.

| Bloc (5 min) | CPU moy | RAM moy | GPU moy |
|---|---|---|---|
| 1 | 0,05 % | 59,2 Mo | 31,31 % |
| 2 | 0,05 % | 59,7 Mo | 30,18 % |
| 3 | 0,03 % | 59,8 Mo | 29,60 % |
| 4 | 0,05 % | 59,8 Mo | 29,03 % |
| 5 | 0,05 % | 59,8 Mo | 27,90 % |
| 6 (231 éch., tronqué) | 0,05 % | 59,5 Mo | 23,65 % |
| **Médiane** | **0,05 %** | **59,8 Mo** | **29,3 %** |

Réserve de mesure : 0,05 % de 16 cœurs ≈ 8 ms/s, alors que
`TotalProcessorTime` avance par ticks de ~15,6 ms (≈ 0,1 % de 16 cœurs) — la
valeur défendable est le **p95 = 0,20 %** (0,16 × OBS, seuil 0,7 ×).

Le **verdict** se calcule ligne par ligne, jamais de mémoire :

- CPU : `spike ≤ 0,7 × OBS`
- RAM : `spike ≤ 0,5 × OBS`
- Drops : `spike = 0`

## Méthode réellement appliquée — écarts au protocole, assumés

Les dix points suivants changent la lecture des chiffres. Ils sont écrits ici
pour qu'aucun relevé ultérieur ne les « redécouvre » comme des anomalies.

1. **Mesure de l'arbre de processus** (`tools/mesure-p0.ps1 -Arbre`), pas du
   seul PID racine. Le spike délègue l'encodage à un enfant `ffmpeg`
   (≈126 Mo) ; OBS traîne cinq `obs-browser-page.exe` (≈285 Mo). Mesurer le
   racine seul sous-estimait la RAM du spike d'un facteur ≈4 (42 Mo au lieu
   de 183 Mo). Contrôle croisé : la fenêtre Stats d'OBS affiche 530,6 Mo et
   0,44 % de CPU pour `obs64` seul, contre 852 Mo / 0,76 % pour l'arbre.
2. **NVENC des deux côtés.** Le relevé indicatif antérieur (60 s) comparait
   un spike NVENC à un OBS en **x264** (2,60 % de CPU) : il mesurait l'écart
   logiciel/matériel, pas le pipeline. En NVENC, OBS tombe à 0,75 %.
3. **Blocs de 5 min au lieu de 3 runs de 30 min.** Justifié par l'absence de
   dérive mesurée ci-dessus. Réserve : ce sont 6 blocs **consécutifs d'un
   même run**, pas 3 runs indépendants. Sur une machine sujette au throttling,
   le protocole 30 min reste obligatoire.
4. **Drops.** Métrique de référence = fenêtre Stats d'OBS :
   `Images manquées (retard de rendu)` = **0 / 213 765**,
   `Images ignorées (retard d'encodage)` = **0 / 166 885**. L'arithmétique du
   journal (`Total drawn frames` 166 887 − `Total frames output` 166 869 = 18)
   ne compte **pas** des drops : ce sont des frames de bord au démarrage et à
   l'arrêt (`warning: 2 frames left in the queue on closing`).
5. **Drops côté spike : non mesurables, donc `N/A`.** Le spike ne peut pas
   perdre de frame : il pousse exactement `durée × fps` frames, quitte à
   pousser en retard. Son compteur `drops estimés` vaut structurellement 0
   (il compare `temps écoulé × fps` à un nombre de frames fixé d'avance) et ne
   prouve rien. Le seul compteur honnête est **`en retard`** (frames à plus
   d'un intervalle de l'échéance) : **188 / 108 000 = 0,17 %**, retard max
   **964 ms**. La cible « drops = 0 » n'est donc **pas vérifiable** en P0-a :
   elle ne discrimine pas.
6. **GPU = somme des instances PDH `GPU Engine` des PID** (3D + Copy +
   VideoEncode confondus), plafonnée à 100 %. Ce n'est **pas** une utilisation
   globale de la carte : c'est comparable entre les deux côtés, non
   interprétable en absolu.
7. **Charge d'encodage non identique.** Spike : 0,674 Go en 30 min
   (≈3,0 Mbit/s effectifs — `-b:v` seul en NVENC plafonne un VBR, ce n'est pas
   du CBR). OBS : 2,0 GiB en 46 min (≈5,8 Mbit/s, vrai CBR). OBS encode donc
   ≈2× plus de bits. Presets différents également (p4/ll contre p5/hq).
   Conséquence : la comparaison CPU/RAM reste valable (l'encodage est sur le
   GPU), la comparaison **GPU est indicative seulement**.
8. **Latence capture→encode : `N/A`.** PresentMon n'a pas été utilisé ; la
   colonne reste vide plutôt que remplie d'un proxy non mesuré.
9. **Une seule machine.** Un résultat NVIDIA ne dit rien de l'iGPU Intel ni
   d'un PC ancien — c'est justement la cible « PC ancien » du plan.
10. **Contenu affiché non instrumenté.** Il n'a pas été capturé ni versionné ;
    les deux côtés ont tourné sur le même écran, mais rien ne le prouve
    a posteriori. Le spike a re-capturé 89 902 frames sur 108 000 (83 %), ce
    qui indique un écran animé, pas une image fixe.

## Verdict global (lot 0.3)

### P0-a — verdict **provisoire** (chaîne capture→encode)

Relevé du **2026-10-27**, PC principal, médiane de 6 blocs de 5 min par côté :

| Cible | PC principal | PC ancien | Tenue ? |
|---|---|---|---|
| CPU ≤ 0,7 × | **2,76 ×** (2,07 % / 0,75 %) | `N/A` | **NON** |
| RAM ≤ 0,5 × | **0,22 ×** (183,5 Mo / 849,5 Mo) | `N/A` | **OUI** |
| Drops = 0 | OBS 0 ; spike **non mesurable** (0 structurel, 188 frames en retard) | `N/A` | **N/A** |

**Verdict P0-a (provisoire, 2026-10-27) : NO-GO pour l'implémentation
actuelle.** 1 cible sur 3 est tenue (RAM), la deuxième n'est pas mesurable,
la première est ratée d'un facteur ≈4 par rapport au seuil.

Ce que ce no-go dit — et ce qu'il ne dit pas :

- Il ne dit **pas** que l'architecture prévue est mauvaise. Il dit que **le
  spike tel qu'il est écrit** consomme plus de CPU qu'OBS.
- La cause est identifiée et **structurelle au spike**, pas à l'encodeur :
  le spike convertit **BGRA→NV12 sur le CPU** puis pousse **1,5 Mo par frame
  dans un pipe** (≈180 Mo/s de copies), alors qu'OBS convertit en NV12 **sur
  le GPU** (`NV12 texture support enabled`) et ne remonte que le bitstream.
  Écart mesuré : 1,32 point de CPU sur 16 cœurs ≈ 21 % d'un cœur
  ≈ **3,5 ms de travail CPU par frame** en trop.
- Le chemin qui supprime ce coût est le **zéro-copie GPU** (lot 0.2f),
  aujourd'hui bloqué par la lecture du plan UV NV12 (`Map` de la
  sous-ressource 1 → `E_INVALIDARG` sur ce pilote).

**Condition de ré-examen** : débloquer 0.2f (ou composer sur GPU) puis
rejouer **exactement** ce protocole. Tant que le CPU du spike reste à
≈3,5 ms/frame au-dessus d'OBS, le mode natif est disqualifié sur les PC
anciens — c'est-à-dire précisément là où il devait gagner.

**Condition remplie le 2026-10-28** — par le SDK NVENC (pas par le MFT,
écarté en `d10cacc`) : voir le relevé zéro-copie ci-dessus et le verdict
révisé qui suit.

**Ce que ce verdict ne tranche pas** : P0-b (fonctionnalités équivalentes),
qui exige le compositeur (phase P2) et reste la mesure **décisionnelle** du
plan. Un P0-a défavorable n'autorise pas à conclure sur P0-b, et
inversement.

#### Verdict P0-a révisé — chemin zéro-copie (2026-10-28)

| Cible | PC principal (zéro-copie) | PC ancien | Tenue ? |
|---|---|---|---|
| CPU ≤ 0,7 × | **0,07 ×** (0,05 % / 0,75 % ; p95 0,20 % → 0,16 ×) | `N/A` | **OUI** |
| RAM ≤ 0,5 × | **0,07 ×** (59,6 Mo / 849,5 Mo) | `N/A` | **OUI** |
| Drops = 0 | OBS 0 ; spike **non mesurable** (14 en retard, 0,013 %) | `N/A` | **N/A** |

**Verdict P0-a révisé (2026-10-28) : 2 cibles sur 3 tenues — le no-go du
2026-10-27 ne s'applique plus.** Il qualifiait l'ancien spike (readback
BGRA → conversion CPU NV12 → pipe ffmpeg), pas la chaîne zéro-copie. La
cible CPU est tenue d'un facteur ≈10 en moyenne (≈4 sur le p95, 0,20 %).

Ce que ce verdict ne dit pas : la mesure ne couvre que la chaîne
capture→encode (P0-a), sur une seule machine (NVIDIA). P0-b
(fonctionnalités équivalentes, compositeur P2) reste la mesure
**décisionnelle**.

### P0-b — verdict **décisionnel** (fonctionnalités équivalentes)

À remplir **après P2** (compositeur), même tableau, même méthode.

**Go** si 2 cibles sur 3 tenues **sur les machines de la matrice**, en P0-b.
Un P0-a favorable mais un P0-b défavorable = **STOP** : c'est le P0-b qui
décide, parce que c'est lui qui compare des fonctionnalités équivalentes.

## Pièges — à ne pas refaire

1. **Comparer OBS nu à un spike chargé.** La référence inclut les browser
   sources ; c'est la comparaison honnête.
2. **Comparer deux scènes différentes.** Une seule scène, figée, versionnée.
3. **Ne relever que la moyenne.** Un p95 à 95 % avec une moyenne à 40 % est un
   moteur qui hoquette — et un stream qui saccade.
4. **Un seul run.** Le premier run est toujours le meilleur (cache froid) ou le
   pire (compilation) : 3 runs, médiane.
5. **Reporter un chiffre non mesuré.** `N/A`, jamais `0` — même règle que le
   diagnostic (§10.1 du plan).
6. **Mesurer pendant un stream.** Le réseau ajoute une variable qu'on ne
   contrôle pas.
7. **Conclure depuis une seule machine.** La matrice en exige plusieurs : un
   résultat NVIDIA ne dit rien d'Intel iGPU, qui est la cible « PC ancien ».

## Sortie attendue

| Lot | Livrable |
|---|---|
| 0.1 | ce fichier : protocole + feuilles de relevé vides |
| 0.2 | spike `spikes/moteur-spike/` + ses chiffres dans les tableaux |
| 0.3 | verdict explicite **go** ou **no-go**, daté, avec les 3 cibles chiffrées |

Tant que la ligne « Verdict global » est vide, **le LOT 1 ne commence pas**.
