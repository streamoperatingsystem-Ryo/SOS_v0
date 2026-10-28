//! Sink fichier brut — bitstream élémentaire (`.h264`), sans conteneur.
//!
//! Utile au diagnostic et aux tests ; un lecteur a besoin d'un remux
//! (`ffmpeg -i x.h264 -c copy x.mp4`).

use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::Path;

use super::{ErreurSortie, Sink};
use crate::encode::PaquetEncode;
use sos_moteur_api::ErreurMoteur;

/// Écrit les paquets tels quels dans un fichier.
pub struct FileSink {
    fichier: BufWriter<File>,
    octets: u64,
}

impl FileSink {
    pub fn nouveau(chemin: &Path) -> Result<Self, ErreurSortie> {
        let fichier = File::create(chemin).map_err(|e| {
            ErreurSortie::Fichier(format!("creation {} : {e}", chemin.display()))
        })?;
        Ok(Self {
            fichier: BufWriter::new(fichier),
            octets: 0,
        })
    }

    pub fn octets(&self) -> u64 {
        self.octets
    }
}

impl Sink for FileSink {
    fn pousser(&mut self, paquet: &PaquetEncode) -> Result<(), ErreurMoteur> {
        self.fichier
            .write_all(&paquet.data)
            .map_err(|e| ErreurSortie::Fichier(format!("ecriture : {e}")))?;
        self.octets += paquet.data.len() as u64;
        Ok(())
    }

    fn fermer(&mut self) -> Result<(), ErreurMoteur> {
        self.fichier
            .flush()
            .map_err(|e| ErreurMoteur::from(ErreurSortie::Fichier(format!("flush : {e}"))))
    }
}
