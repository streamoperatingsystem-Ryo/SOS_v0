// =============================================================================
// SignatureScanner — Scan de signatures binaires en mémoire (pattern matching)
// (port de SignatureScanner.cs)
// -----------------------------------------------------------------------------
// Recherche un pattern d'bytes dans une région mémoire d'un processus.
// Utilisé par les scripts ASL pour trouver des adresses dynamiques via des
// signatures (séquences d'bytes connues avec wildcards).
//
// Exemple : chercher la séquence "48 8B 05 ?? ?? ?? ?? 48 85 C0"
//   → les ?? sont des wildcards (n'importe quel byte)
//   → retourne l'adresse où le pattern a été trouvé
// =============================================================================
use super::LecteurMemoire;

/// Cible de scan avec une ou plusieurs signatures (pattern + wildcard mask).
#[derive(Clone)]
pub struct SigScanTarget {
    signatures: Vec<Signature>,
    pub on_found: Option<fn(&dyn LecteurMemoire, usize) -> usize>,
}

#[derive(Clone)]
struct Signature {
    pattern: Vec<u8>,
    mask: Vec<bool>, // true = wildcard (ignore), false = doit matcher
    offset: i32,
}

impl SigScanTarget {
    pub fn new() -> Self {
        Self {
            signatures: Vec::new(),
            on_found: None,
        }
    }

    /// Ajoute une signature depuis une string hex (ex: "48 8B 05 ?? ?? ?? ??").
    /// Les "??" sont des wildcards. L'offset est ajouté à l'adresse trouvée.
    pub fn add_signature_string(&mut self, offset: i32, sig_str: &str) {
        let cleaned: String = sig_str.chars().filter(|c| !c.is_whitespace()).collect();
        let mut pattern = Vec::new();
        let mut mask = Vec::new();

        let chars: Vec<char> = cleaned.chars().collect();
        let mut i = 0;
        while i + 1 < chars.len() {
            let pair = &chars[i..i + 2];
            if pair[0] == '?' || pair[1] == '?' {
                pattern.push(0);
                mask.push(true);
            } else {
                let byte = u8::from_str_radix(&pair.iter().collect::<String>(), 16).unwrap_or(0);
                pattern.push(byte);
                mask.push(false);
            }
            i += 2;
        }

        self.signatures.push(Signature {
            pattern,
            mask,
            offset,
        });
    }

    pub fn add_signature_bytes(&mut self, offset: i32, bytes: &[u8]) {
        self.signatures.push(Signature {
            pattern: bytes.to_vec(),
            mask: vec![false; bytes.len()],
            offset,
        });
    }
}

impl Default for SigScanTarget {
    fn default() -> Self {
        Self::new()
    }
}

/// Scanner qui cherche des signatures dans une région mémoire d'un processus.
pub struct SignatureScanner<'a> {
    process: &'a dyn LecteurMemoire,
    address: usize,
    size: usize,
}

impl<'a> SignatureScanner<'a> {
    /// Crée un scanner pour la région mémoire [address, address+size).
    pub fn new(process: &'a dyn LecteurMemoire, address: usize, size: usize) -> Self {
        Self {
            process,
            address,
            size,
        }
    }

    /// Scanne la région mémoire pour trouver la première occurrence du target.
    /// Retourne l'adresse (après application de l'offset et du callback on_found).
    pub fn scan(&self, target: &SigScanTarget) -> Option<usize> {
        // Lire toute la région mémoire
        let memory = self.process.lire_bytes(self.address, self.size)?;

        for sig in &target.signatures {
            if let Some(offset_in_memory) = scan_pattern(&memory, &sig.pattern, &sig.mask) {
                let mut found_addr =
                    (self.address + offset_in_memory).wrapping_add(sig.offset as usize);

                if let Some(callback) = target.on_found {
                    found_addr = callback(self.process, found_addr);
                }

                return Some(found_addr);
            }
        }

        None
    }
}

/// Cherche un pattern dans un buffer mémoire. Retourne l'offset du premier match.
fn scan_pattern(memory: &[u8], pattern: &[u8], mask: &[bool]) -> Option<usize> {
    if pattern.is_empty() || pattern.len() > memory.len() {
        return None;
    }

    let end = memory.len() - pattern.len();
    for i in 0..=end {
        let mut matched = true;
        for j in 0..pattern.len() {
            if mask[j] {
                continue;
            }
            if memory[i + j] != pattern[j] {
                matched = false;
                break;
            }
        }
        if matched {
            return Some(i);
        }
    }

    None
}
