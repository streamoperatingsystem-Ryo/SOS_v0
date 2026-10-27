# Mesures P0 — baseline OBS et verdict go/no-go

> **Lot 0.1** (phase P0 — Preuve). Ce fichier est un **protocole à exécuter**,
> pas un résultat. Tant que les tableaux de la section « Feuille de relevé »
> ne sont pas remplis sur les machines réelles, **aucun chiffre de ce dépôt
> ne prouve quoi que ce soit**.
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

## Scène de référence

Une seule scène, figée, identique des deux côtés de la comparaison :

- 1 capture de jeu
- 1 caméra
- 1 chat
- 1 alerte

**1080p60, 30 minutes d'enregistrement local** (pas de stream : le réseau
introduirait une variable).

> ⚠️ **OBS doit tourner AVEC ses browser sources.** C'est tout l'enjeu : ce
> qu'on cherche à supprimer, c'est le navigateur dans le chemin de sortie. Une
> baseline « OBS nu, 3 sources natives » ne prouve rien — le plan §1.2 le dit
> explicitement, OBS nu est déjà proche de l'optimum GPU.

## Outillage

| Grandeur | Outil | Remarque |
|---|---|---|
| CPU, RSS | PDH `\Process(*)` ou `Get-Process` | **même outil des deux côtés**, sinon les chiffres ne sont pas comparables |
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

## Feuille de relevé — à remplir par machine

Une ligne par machine (matrice §6.4 du plan : « PC principal », « PC ancien »,
puis toute machine supplémentaire).

| Machine | GPU | Pilote | Capture | OBS |
|---|---|---|---|---|
| PC principal | | | | |
| PC ancien | | | | |
| | | | | |

Puis, par machine et par côté :

| Machine | Côté | CPU moy | CPU p95 | RSS moy | RSS p95 | GPU moy | Drops | Latence p95 | Verdict |
|---|---|---|---|---|---|---|---|---|---|
| PC principal | OBS | | | | | | | | référence |
| PC principal | spike | | | | | | | | |
| PC ancien | OBS | | | | | | | | référence |
| PC ancien | spike | | | | | | | | |

Le **verdict** se calcule ligne par ligne, jamais de mémoire :

- CPU : `spike ≤ 0,7 × OBS`
- RAM : `spike ≤ 0,5 × OBS`
- Drops : `spike = 0`

## Verdict global (lot 0.3)

À remplir **après** tous les relevés :

| Cible | PC principal | PC ancien | Tenue ? |
|---|---|---|---|
| CPU ≤ 0,7 × | | | |
| RAM ≤ 0,5 × | | | |
| Drops = 0 | | | |

**Go** si 2 cibles sur 3 tenues **sur les machines de la matrice**. Sinon :
**STOP** — pas de plan de six mois sur une intuition, réévaluation avant toute
ligne de moteur.

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
