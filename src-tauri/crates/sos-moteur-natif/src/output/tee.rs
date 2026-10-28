//! TeeSink — diffuse chaque paquet vers plusieurs sinks (stream + record).
//!
//! Un sink en erreur fait échouer le paquet (jamais de perte silencieuse) ;
//! `fermer` ferme tous les sinks et rend la première erreur rencontrée.

use super::Sink;
use crate::encode::PaquetEncode;
use sos_moteur_api::ErreurMoteur;

/// Diffuse chaque paquet vers tous ses sinks.
pub struct TeeSink {
    sinks: Vec<Box<dyn Sink>>,
}

impl TeeSink {
    pub fn nouveau(sinks: Vec<Box<dyn Sink>>) -> Self {
        Self { sinks }
    }

    pub fn len(&self) -> usize {
        self.sinks.len()
    }

    pub fn is_empty(&self) -> bool {
        self.sinks.is_empty()
    }
}

impl Sink for TeeSink {
    fn pousser(&mut self, paquet: &PaquetEncode) -> Result<(), ErreurMoteur> {
        for sink in &mut self.sinks {
            sink.pousser(paquet)?;
        }
        Ok(())
    }

    fn fermer(&mut self) -> Result<(), ErreurMoteur> {
        let mut premiere: Option<ErreurMoteur> = None;
        for sink in &mut self.sinks {
            if let Err(e) = sink.fermer() {
                if premiere.is_none() {
                    premiere = Some(e);
                }
            }
        }
        match premiere {
            Some(e) => Err(e),
            None => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::output::file::FileSink;
    use std::fs;

    #[test]
    fn diffuse_vers_deux_fichiers() {
        let base = std::env::temp_dir();
        let a = base.join("sos-tee-a.h264");
        let b = base.join("sos-tee-b.h264");
        let mut tee = TeeSink::nouveau(vec![
            Box::new(FileSink::nouveau(&a).expect("sink a")),
            Box::new(FileSink::nouveau(&b).expect("sink b")),
        ]);
        assert_eq!(tee.len(), 2);

        let premier = PaquetEncode {
            data: vec![1, 2, 3],
            pts_us: 0,
            cle: true,
        };
        let second = PaquetEncode {
            data: vec![4, 5],
            pts_us: 1,
            cle: false,
        };
        tee.pousser(&premier).expect("paquet 1");
        tee.pousser(&second).expect("paquet 2");
        tee.fermer().expect("fermeture");

        assert_eq!(fs::read(&a).expect("lire a"), vec![1, 2, 3, 4, 5]);
        assert_eq!(fs::read(&b).expect("lire b"), vec![1, 2, 3, 4, 5]);

        let _ = fs::remove_file(&a);
        let _ = fs::remove_file(&b);
    }

    #[test]
    fn vide_ne_fait_rien() {
        let mut tee = TeeSink::nouveau(Vec::new());
        assert!(tee.is_empty());
        let paquet = PaquetEncode {
            data: vec![1],
            pts_us: 0,
            cle: false,
        };
        assert!(tee.pousser(&paquet).is_ok());
        assert!(tee.fermer().is_ok());
    }
}
