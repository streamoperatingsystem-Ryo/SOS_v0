// Point d'entrée Windows — empêche l'ouverture d'une console supplémentaire.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    streamos_v0_lib::run()
}
