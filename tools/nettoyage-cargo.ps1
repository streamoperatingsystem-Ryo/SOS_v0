# nettoyage-cargo.ps1 — purge des objets orphelins laisses par rustc
#
# Chaque compilation interrompue (Ctrl+C sur `npm run tauri dev`, redemarrage
# du dev server) laisse dans target\debug\deps les *.rcgu.o du crate en cours
# de compilation, et parfois un dossier .tmp*.temp-archive. Rustc ne les
# supprime jamais : ils s'accumulent (constat 2026-11-02 : 33 786 fichiers,
# 60 Go reellement rendus au volume). Le cache incremental n'est pas elague
# non plus (960 sessions au lieu de ~4 par crate).
#
# Ce script ne touche QUE des artefacts intermediaires : les .rlib, .rmeta,
# .pdb et les dependances compilees sont conserves, donc le prochain build
# reste rapide.
#
# Usage :
#   powershell -ExecutionPolicy Bypass -File tools\nettoyage-cargo.ps1
#   powershell -ExecutionPolicy Bypass -File tools\nettoyage-cargo.ps1 -Simulation
#   powershell -ExecutionPolicy Bypass -File tools\nettoyage-cargo.ps1 `
#       -JoursAge 7 -InclureIncremental
#
# Parametres :
#   -JoursAge           age minimal des artefacts a supprimer (defaut 2).
#                       Garder >= 1 evite de casser une compilation en cours.
#   -InclureIncremental purge aussi les sessions de target\debug\incremental
#                       plus vieilles que -JoursAge (recompilation des crates
#                       locales au prochain build, dependances intactes).
#   -Simulation         affiche ce qui serait supprime, ne supprime rien.
#   -Profil             profil cargo a nettoyer (defaut debug).

param(
    [int]$JoursAge = 2,
    [switch]$InclureIncremental,
    [switch]$Simulation,
    [string]$Profil = "debug"
)

$ErrorActionPreference = "Stop"

$racine = Split-Path -Parent $PSScriptRoot
$debug = Join-Path $racine "src-tauri\target\$Profil"
$deps = Join-Path $debug "deps"

if (-not (Test-Path $deps)) {
    Write-Host "Rien a nettoyer : $deps introuvable."
    exit 0
}

$limite = (Get-Date).AddDays(-$JoursAge)
$libreAvant = (Get-PSDrive C).Free

function Taille-Go($items) {
    $somme = ($items | Measure-Object Length -Sum).Sum
    if ($null -eq $somme) { return 0 }
    return [math]::Round($somme / 1GB, 2)
}

function Supprimer($items, $libelle) {
    if ($null -eq $items -or @($items).Count -eq 0) {
        Write-Host ("  {0,-22} rien" -f $libelle)
        return
    }
    $items = @($items)
    $go = Taille-Go $items
    if ($Simulation) {
        Write-Host ("  {0,-22} {1,6} elements  {2,7} Go  (simulation)" -f $libelle, $items.Count, $go)
        return
    }
    $items | Remove-Item -Recurse -Force -ErrorAction SilentlyContinue
    Write-Host ("  {0,-22} {1,6} elements  {2,7} Go  supprimes" -f $libelle, $items.Count, $go)
}

Write-Host "Cible   : $debug"
Write-Host ("Filtre  : artefacts non modifies depuis {0} jours" -f $JoursAge)
if ($Simulation) { Write-Host "Mode    : SIMULATION (aucune suppression)" }
Write-Host ""

$objets = Get-ChildItem $deps -File -Filter "*.rcgu.o" -ErrorAction SilentlyContinue |
    Where-Object { $_.LastWriteTime -lt $limite }
$archives = Get-ChildItem $deps -Directory -Filter ".tmp*.temp-archive" -ErrorAction SilentlyContinue |
    Where-Object { $_.LastWriteTime -lt $limite }

Supprimer $objets "objets *.rcgu.o"
Supprimer $archives "archives temp"

if ($InclureIncremental) {
    $incremental = Join-Path $debug "incremental"
    if (Test-Path $incremental) {
        $sessions = Get-ChildItem $incremental -Directory -ErrorAction SilentlyContinue |
            Where-Object { $_.LastWriteTime -lt $limite }
        if ($sessions) {
            $go = [math]::Round(((Get-ChildItem $sessions.FullName -Recurse -File -ErrorAction SilentlyContinue |
                Measure-Object Length -Sum).Sum) / 1GB, 2)
            if ($Simulation) {
                Write-Host ("  {0,-22} {1,6} sessions  {2,7} Go  (simulation)" -f "cache incremental", @($sessions).Count, $go)
            } else {
                $sessions | Remove-Item -Recurse -Force -ErrorAction SilentlyContinue
                Write-Host ("  {0,-22} {1,6} sessions  {2,7} Go  supprimees" -f "cache incremental", @($sessions).Count, $go)
            }
        } else {
            Write-Host ("  {0,-22} rien" -f "cache incremental")
        }
    }
}

$libreApres = (Get-PSDrive C).Free
Write-Host ""
if ($Simulation) {
    Write-Host "Aucune suppression effectuee."
} else {
    Write-Host ("Volume C: {0:N1} Go libres -> {1:N1} Go libres (+{2:N1} Go)" -f ($libreAvant / 1GB), ($libreApres / 1GB), (($libreApres - $libreAvant) / 1GB))
}
