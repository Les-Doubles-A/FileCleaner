//! Journal local, un fichier par jour. Rien ne quitte la machine.

use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::sync::Mutex;

use chrono::Local;
use log::{Level, LevelFilter, Log, Metadata, Record};

struct FileLogger {
    file: Mutex<File>,
}

impl Log for FileLogger {
    fn enabled(&self, metadata: &Metadata<'_>) -> bool {
        metadata.level() <= Level::Info || metadata.target().starts_with("filecleaner")
    }

    fn log(&self, record: &Record<'_>) {
        if !self.enabled(record.metadata()) {
            return;
        }
        if let Ok(mut file) = self.file.lock() {
            let _ = writeln!(
                file,
                "{} {:<5} {} | {}",
                Local::now().format("%Y-%m-%d %H:%M:%S"),
                record.level(),
                record.target(),
                record.args()
            );
        }
    }

    fn flush(&self) {
        if let Ok(mut file) = self.file.lock() {
            let _ = file.flush();
        }
    }
}

/// Initialise le journal et retire ceux de plus de 30 jours.
pub fn init(dir: &Path) {
    let _ = fs::create_dir_all(dir);
    if let Ok(entries) = fs::read_dir(dir) {
        let limit = std::time::Duration::from_secs(30 * 86_400);
        for entry in entries.flatten() {
            let expired = entry
                .metadata()
                .and_then(|m| m.modified())
                .ok()
                .and_then(|t| t.elapsed().ok())
                .is_some_and(|age| age > limit);
            if expired {
                let _ = fs::remove_file(entry.path());
            }
        }
    }
    let path = dir.join(format!("filecleaner_{}.log", Local::now().format("%Y%m%d")));
    let Ok(file) = OpenOptions::new().create(true).append(true).open(path) else {
        return;
    };
    if log::set_boxed_logger(Box::new(FileLogger { file: Mutex::new(file) })).is_ok() {
        log::set_max_level(LevelFilter::Debug);
    }
}
