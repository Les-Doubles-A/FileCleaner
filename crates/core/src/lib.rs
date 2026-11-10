//! Moteur de File Cleaner : analyse, detection et actions reversibles.
//! Aucune dependance a l'interface, tout fonctionne hors ligne.

pub mod actions;
pub mod analysis;
pub mod config;
pub mod duplicates;
pub mod export;
pub mod history;
pub mod logger;
pub mod model;
pub mod organize;
pub mod rename;
pub mod safety;
pub mod scanner;
pub mod search;
pub mod similar;
pub mod snapshot;
pub mod system;
pub mod treemap;
pub mod util;
