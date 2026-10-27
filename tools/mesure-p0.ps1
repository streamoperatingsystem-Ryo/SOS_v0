# mesure-p0.ps1 — harnais de mesure P0 (baseline OBS vs spike Rust)
#
# Echantillonne CPU, RAM, GPU et RAM systeme d'un processus, a intervalle
# regulier, et produit un CSV + un resume (moyenne, p95).
#
# Les compteurs GPU utilisent PdhAddEnglishCounterW : le chemin anglais
# fonctionne meme sur un Windows francais (meme methode que conso.rs).
#
# Usage :
#   powershell -ExecutionPolicy Bypass -File tools\mesure-p0.ps1 `
#       -IdProc 1234 -DureeSecondes 1800 -Sortie mesures\obs-run1.csv
#
# Parametres :
#   -IdProc                 PID du processus a mesurer (obligatoire)
#   -Arbre               mesure le processus ET tous ses descendants
#                        (spike : encode_nvenc + ffmpeg ; OBS : obs64 +
#                        obs-browser-page). Sans ce commutateur, seul le PID
#                        racine est mesure.
#   -DureeSecondes       duree totale (defaut 1800 = 30 min)
#   -IntervalleMs        periode d'echantillonnage (defaut 1000)
#   -IgnorerDebutSecondes  echantillons ignores dans le resume (defaut 60)
#   -Sortie              chemin du CSV (defaut mesures-p0-<pid>-<ts>.csv)

param(
    [Parameter(Mandatory = $true)][int]$IdProc,
    [switch]$Arbre,
    [int]$DureeSecondes = 1800,
    [int]$IntervalleMs = 1000,
    [int]$IgnorerDebutSecondes = 60,
    [string]$Sortie = ""
)

$ErrorActionPreference = "Stop"

# Culture invariante : virgule decimale interdite dans le CSV (comparabilite).
[System.Threading.Thread]::CurrentThread.CurrentCulture =
    [System.Globalization.CultureInfo]::InvariantCulture

if ($Sortie -eq "") {
    $horodatage = Get-Date -Format "yyyyMMdd-HHmmss"
    $Sortie = "mesures-p0-$IdProc-$horodatage.csv"
}

$dossier = Split-Path -Parent $Sortie
if ($dossier -and -not (Test-Path $dossier)) {
    New-Item -ItemType Directory -Path $dossier | Out-Null
}

# --- Interop : PDH (compteurs anglais) + GlobalMemoryStatusEx -------------
if (-not ("MesureP0.Pdh" -as [type])) {
    Add-Type -TypeDefinition @"
using System;
using System.Runtime.InteropServices;

namespace MesureP0 {

    public static class Pdh {
        [StructLayout(LayoutKind.Sequential)]
        public struct PDH_FMT_COUNTERVALUE {
            public uint CStatus;
            public double doubleValue;
        }

        [StructLayout(LayoutKind.Sequential)]
        public struct PDH_FMT_COUNTERVALUE_ITEM_W {
            public IntPtr szName;
            public PDH_FMT_COUNTERVALUE FmtValue;
        }

        [DllImport("pdh.dll", CharSet = CharSet.Unicode)]
        static extern uint PdhOpenQueryW(string dataSource, IntPtr userData, out IntPtr query);

        [DllImport("pdh.dll", CharSet = CharSet.Unicode)]
        static extern uint PdhAddEnglishCounterW(IntPtr query, string path, IntPtr userData, out IntPtr counter);

        [DllImport("pdh.dll")]
        static extern uint PdhCollectQueryData(IntPtr query);

        [DllImport("pdh.dll")]
        static extern uint PdhGetFormattedCounterArrayW(
            IntPtr counter, uint format, ref uint bufferSize, ref uint itemCount, IntPtr buffer);

        [DllImport("pdh.dll")]
        static extern uint PdhCloseQuery(IntPtr query);

        const uint PDH_FMT_DOUBLE = 0x00000200;
        const uint PDH_MORE_DATA = 0x800007D2;

        // Somme des moteurs GPU du PID. -1 = non mesurable.
        public static double GpuPourPid(int pid) {
            return GpuPourPids(new int[] { pid });
        }

        // Somme des moteurs GPU de plusieurs PID (arbre de processus).
        // -1 = non mesurable.
        public static double GpuPourPids(int[] pids) {
            if (pids == null || pids.Length == 0) return -1;

            string[] motifs = new string[pids.Length];
            for (int j = 0; j < pids.Length; j++) motifs[j] = "pid_" + pids[j] + "_";

            IntPtr query;
            if (PdhOpenQueryW(null, IntPtr.Zero, out query) != 0) return -1;

            IntPtr counter;
            if (PdhAddEnglishCounterW(query, "\\GPU Engine(*)\\Utilization Percentage", IntPtr.Zero, out counter) != 0) {
                PdhCloseQuery(query);
                return -1;
            }

            PdhCollectQueryData(query);
            System.Threading.Thread.Sleep(200);
            PdhCollectQueryData(query);

            uint taille = 0;
            uint nombre = 0;
            uint r = PdhGetFormattedCounterArrayW(counter, PDH_FMT_DOUBLE, ref taille, ref nombre, IntPtr.Zero);
            if (r != PDH_MORE_DATA || taille == 0) {
                PdhCloseQuery(query);
                return -1;
            }

            IntPtr tampon = Marshal.AllocHGlobal((int)taille);
            try {
                r = PdhGetFormattedCounterArrayW(counter, PDH_FMT_DOUBLE, ref taille, ref nombre, tampon);
                if (r != 0) return -1;

                double total = 0;
                int tailleItem = Marshal.SizeOf(typeof(PDH_FMT_COUNTERVALUE_ITEM_W));

                for (int i = 0; i < nombre; i++) {
                    IntPtr p = new IntPtr(tampon.ToInt64() + i * tailleItem);
                    var item = (PDH_FMT_COUNTERVALUE_ITEM_W)Marshal.PtrToStructure(p, typeof(PDH_FMT_COUNTERVALUE_ITEM_W));
                    string nom = Marshal.PtrToStringUni(item.szName);
                    if (nom == null) continue;

                    for (int j = 0; j < motifs.Length; j++) {
                        if (nom.IndexOf(motifs[j], StringComparison.OrdinalIgnoreCase) >= 0) {
                            total += item.FmtValue.doubleValue;
                            break;
                        }
                    }
                }

                return total > 100.0 ? 100.0 : total;
            }
            finally {
                Marshal.FreeHGlobal(tampon);
                PdhCloseQuery(query);
            }
        }
    }

    public static class Memoire {
        [StructLayout(LayoutKind.Sequential)]
        public struct MEMORYSTATUSEX {
            public uint dwLength;
            public uint dwMemoryLoad;
            public ulong ullTotalPhys;
            public ulong ullAvailPhys;
            public ulong ullTotalPageFile;
            public ulong ullAvailPageFile;
            public ulong ullTotalVirtual;
            public ulong ullAvailVirtual;
            public ulong ullAvailExtendedVirtual;
        }

        [DllImport("kernel32.dll", SetLastError = true)]
        static extern bool GlobalMemoryStatusEx(ref MEMORYSTATUSEX lpBuffer);

        // RAM systeme OCCUPEE, en Mo. -1 si indisponible.
        public static double RamSystemeOccupeeMo() {
            var etat = new MEMORYSTATUSEX();
            etat.dwLength = (uint)Marshal.SizeOf(typeof(MEMORYSTATUSEX));
            if (!GlobalMemoryStatusEx(ref etat)) return -1;
            ulong utilise = etat.ullTotalPhys - etat.ullAvailPhys;
            return utilise / (1024.0 * 1024.0);
        }
    }
}
"@
}

function Centile95 {
    param([double[]]$Valeurs)
    if ($null -eq $Valeurs -or $Valeurs.Count -eq 0) { return $null }
    $triees = $Valeurs | Sort-Object
    $index = [math]::Ceiling(0.95 * $triees.Count) - 1
    if ($index -lt 0) { $index = 0 }
    return $triees[$index]
}

function Moyenne {
    param([double[]]$Valeurs)
    if ($null -eq $Valeurs -or $Valeurs.Count -eq 0) { return $null }
    return ($Valeurs | Measure-Object -Average).Average
}

# Arbre de processus : racine + tous les descendants, a partir d'une table
# Win32_Process deja collectee (une seule requete par echantillon).
function ArbrePids {
    param([int]$Racine, $Table)
    $enfants = @{}
    foreach ($t in $Table) {
        $parent = [int]$t.ParentProcessId
        if (-not $enfants.ContainsKey($parent)) {
            $enfants[$parent] = New-Object System.Collections.Generic.List[int]
        }
        $enfants[$parent].Add([int]$t.ProcessId)
    }

    $tous = New-Object System.Collections.Generic.List[int]
    $file = New-Object System.Collections.Generic.Queue[int]
    $tous.Add($Racine)
    $file.Enqueue($Racine)
    while ($file.Count -gt 0) {
        $courant = $file.Dequeue()
        if (-not $enfants.ContainsKey($courant)) { continue }
        foreach ($e in $enfants[$courant]) {
            if (-not $tous.Contains($e)) {
                $tous.Add($e)
                $file.Enqueue($e)
            }
        }
    }
    return $tous.ToArray()
}

# --- Boucle ---------------------------------------------------------------
$processus = Get-Process -Id $IdProc -ErrorAction Stop
$coeurs = [Environment]::ProcessorCount
$nombre = [math]::Ceiling($DureeSecondes * 1000.0 / $IntervalleMs)

Write-Host ("Mesure de {0} (PID {1}) - {2} echantillons, {3} coeurs" -f $processus.ProcessName, $IdProc, $nombre, $coeurs)
Write-Host ("Sortie CSV : {0}" -f $Sortie)
Write-Host "Ctrl+C pour arreter proprement (le CSV deja ecrit reste valide)."

$lignes = New-Object System.Collections.Generic.List[object]
$cpuPrecedent = @{}   # PID -> ms CPU au tour precedent
$instantPrecedent = [DateTime]::UtcNow

# Cadence sur l'horloge murale : les requetes PDH/CIM (200-300 ms) ne doivent
# pas allonger la duree totale. La mesure s'arrete a DureeSecondes reelles.
$debutMesure = [DateTime]::UtcNow
$finMesure = $debutMesure.AddSeconds($DureeSecondes)

for ($i = 0; $i -lt $nombre; $i++) {
    $echeance = $debutMesure.AddMilliseconds(($i + 1) * $IntervalleMs)
    $attente = ($echeance - [DateTime]::UtcNow).TotalMilliseconds
    if ($attente -gt 0) { Start-Sleep -Milliseconds ([math]::Round($attente)) }
    if ([DateTime]::UtcNow -ge $finMesure) { break }

    $p = Get-Process -Id $IdProc -ErrorAction SilentlyContinue
    if ($null -eq $p) {
        Write-Host "Processus termine : arret de la mesure."
        break
    }

    if ($Arbre) {
        $table = Get-CimInstance Win32_Process -Property ProcessId, ParentProcessId
        $pids = @(ArbrePids $IdProc $table)
    } else {
        $pids = @($IdProc)
    }

    $instant = [DateTime]::UtcNow
    $deltaMs = ($instant - $instantPrecedent).TotalMilliseconds

    # CPU et RSS cumules sur l'arbre. Un PID nouveau n'apporte que son delta
    # (sa consommation anterieure est ignoree, pas comptee d'un coup).
    $cpuMs = 0.0
    $ramMo = 0.0
    $cpuCourant = @{}
    foreach ($idp in $pids) {
        $proc = Get-Process -Id $idp -ErrorAction SilentlyContinue
        if ($null -eq $proc) { continue }
        $cpu = $proc.TotalProcessorTime.TotalMilliseconds
        if ($cpuPrecedent.ContainsKey($idp)) { $cpuMs += $cpu - $cpuPrecedent[$idp] }
        $cpuCourant[$idp] = $cpu
        $ramMo += $proc.WorkingSet64 / 1MB
    }
    $cpuPrecedent = $cpuCourant

    $cpuPct = 0.0
    if ($deltaMs -gt 0) {
        $cpuPct = ($cpuMs / $deltaMs) * 100.0 / $coeurs
    }

    $gpu = if ($Arbre) { [MesureP0.Pdh]::GpuPourPids($pids) } else { [MesureP0.Pdh]::GpuPourPid($IdProc) }
    $ramSysteme = [MesureP0.Memoire]::RamSystemeOccupeeMo()

    $ligne = [pscustomobject]@{
        ts         = $instant.ToString("o")
        cpu_pct    = [math]::Round($cpuPct, 2)
        ram_mo     = [math]::Round($ramMo, 1)
        gpu_pct    = if ($gpu -lt 0) { "N/A" } else { [math]::Round($gpu, 2) }
        ram_sys_mo = if ($ramSysteme -lt 0) { "N/A" } else { [math]::Round($ramSysteme, 1) }
    }
    $lignes.Add($ligne) | Out-Null

    $instantPrecedent = $instant

    if (($i + 1) % 10 -eq 0) {
        $suffixe = if ($Arbre) { " - arbre $($pids.Count) proc." } else { "" }
        Write-Host ("  {0}/{1} - CPU {2}% - RAM {3} Mo - GPU {4}%{5}" -f ($i + 1), $nombre, $ligne.cpu_pct, $ligne.ram_mo, $ligne.gpu_pct, $suffixe)
    }
}

if ($lignes.Count -eq 0) {
    Write-Error "Aucun echantillon collecte."
    exit 1
}

$lignes | Export-Csv -Path $Sortie -NoTypeInformation -Encoding UTF8

# --- Resume (ignore les N premiers echantillons) --------------------------
$ignorer = [math]::Min($IgnorerDebutSecondes, [math]::Max(0, $lignes.Count - 1))
$retenues = $lignes | Select-Object -Skip $ignorer

$cpu = @($retenues | ForEach-Object { [double]$_.cpu_pct })
$ram = @($retenues | ForEach-Object { [double]$_.ram_mo })
$gpu = @($retenues | Where-Object { $_.gpu_pct -ne "N/A" } | ForEach-Object { [double]$_.gpu_pct })
$ramSys = @($retenues | Where-Object { $_.ram_sys_mo -ne "N/A" } | ForEach-Object { [double]$_.ram_sys_mo })

$resume = @()
$resume += "=== RESUME P0 ==="
$resume += ("processus       : {0} (PID {1})" -f $processus.ProcessName, $IdProc)
$resume += ("mode            : {0}" -f $(if ($Arbre) { "arbre de processus" } else { "PID seul" }))
$resume += ("echantillons    : {0} collectes, {1} ignores, {2} retenus" -f $lignes.Count, $ignorer, $retenues.Count)
$resume += ("CPU  moyenne    : {0:N2} %" -f (Moyenne $cpu))
$resume += ("CPU  p95        : {0:N2} %" -f (Centile95 $cpu))
$resume += ("RAM  moyenne    : {0:N1} Mo" -f (Moyenne $ram))
$resume += ("RAM  p95        : {0:N1} Mo" -f (Centile95 $ram))
if ($gpu.Count -gt 0) {
    $resume += ("GPU  moyenne    : {0:N2} %" -f (Moyenne $gpu))
    $resume += ("GPU  p95        : {0:N2} %" -f (Centile95 $gpu))
} else {
    $resume += "GPU  moyenne    : N/A"
    $resume += "GPU  p95        : N/A"
}
if ($ramSys.Count -gt 0) {
    $resume += ("RAM systeme moy : {0} Mo" -f [math]::Round((Moyenne $ramSys)))
} else {
    $resume += "RAM systeme moy : N/A"
}
$resume += ("CSV             : {0}" -f (Resolve-Path $Sortie))

$cheminResume = [System.IO.Path]::ChangeExtension($Sortie, ".resume.txt")
$resume | Set-Content -Path $cheminResume -Encoding UTF8

$resume | ForEach-Object { Write-Host $_ }
Write-Host ("Resume ecrit : {0}" -f $cheminResume)
