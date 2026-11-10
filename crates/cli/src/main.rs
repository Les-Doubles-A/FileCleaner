//! `fc` : analyses en ligne de commande. Strictement en lecture seule, sauf
//! l'ecriture du rapport demande par `export`.

use std::path::PathBuf;
use std::sync::Mutex;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand, ValueEnum};
use filecleaner_core::analysis::{self, rule_label};
use filecleaner_core::config::{AppPaths, Settings};
use filecleaner_core::duplicates::{self, DupGroup, HashCache};
use filecleaner_core::export;
use filecleaner_core::model::{FileId, Progress, Scan};
use filecleaner_core::scanner;
use filecleaner_core::search::Query;
use filecleaner_core::util::{Lang, format_count, format_size};
use serde_json::json;

#[derive(Parser)]
#[command(name = "fc", version, about = "File Cleaner en ligne de commande (lecture seule)")]
struct Cli {
    /// Sortie JSON
    #[arg(long, global = true)]
    json: bool,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Resume d'un dossier
    Scan { paths: Vec<PathBuf> },
    /// Doublons par contenu
    Dupes { paths: Vec<PathBuf> },
    /// Fichiers inutiles
    Junk { paths: Vec<PathBuf> },
    /// Gros fichiers
    Big {
        paths: Vec<PathBuf>,
        #[arg(long, default_value_t = 100)]
        min_mb: u64,
    },
    /// Dossiers regenerables de projets
    Dev { paths: Vec<PathBuf> },
    /// Recherche, ex: `fc search . "ext:pdf size:>10mb"`
    Search { path: PathBuf, query: String },
    /// Exporte un rapport
    Export {
        path: PathBuf,
        #[arg(short, long)]
        output: PathBuf,
        #[arg(short, long, value_enum, default_value_t = Format::Html)]
        format: Format,
    },
}

#[derive(Clone, Copy, ValueEnum)]
enum Format {
    Csv,
    Json,
    Html,
    Pdf,
}

const LANG: Lang = Lang::Fr;

fn run_scan(paths: &[PathBuf], settings: &Settings) -> Result<Scan> {
    anyhow::ensure!(!paths.is_empty(), "indiquez au moins un dossier");
    for path in paths {
        anyhow::ensure!(path.is_dir(), "{} n'est pas un dossier", path.display());
    }
    let absolute: Vec<PathBuf> =
        paths.iter().map(|p| std::path::absolute(p).unwrap_or_else(|_| p.clone())).collect();
    Ok(scanner::scan(&absolute, settings, &Progress::default()))
}

fn print_files(scan: &Scan, ids: &[FileId], json_output: bool) {
    if json_output {
        let rows: Vec<_> =
            ids.iter().map(|&id| json!({"path": scan.file_path(id), "size": scan.file(id).size})).collect();
        println!("{}", serde_json::Value::Array(rows));
    } else {
        for &id in ids {
            println!("{:>10}  {}", format_size(scan.file(id).size, LANG), scan.file_path(id).display());
        }
        let total: u64 = ids.iter().map(|&id| scan.file(id).size).sum();
        println!("{} fichier(s), {}", format_count(ids.len() as u64, LANG), format_size(total, LANG));
    }
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let paths = AppPaths::resolve();
    let settings = Settings::load(&paths);
    match cli.command {
        Command::Scan { paths } => {
            let scan = run_scan(&paths, &settings)?;
            if cli.json {
                let summary = json!({
                    "files": scan.total_files(),
                    "folders": scan.total_dirs(),
                    "size": scan.total_size(),
                    "duration_ms": scan.duration_ms,
                    "categories": scan.category_breakdown(),
                });
                println!("{summary}");
            } else {
                println!(
                    "{} fichiers, {} dossiers, {} ({} ms)",
                    format_count(scan.total_files(), LANG),
                    format_count(scan.total_dirs(), LANG),
                    format_size(scan.total_size(), LANG),
                    scan.duration_ms
                );
                for c in scan.category_breakdown() {
                    println!("{:>10}  {:<20} {}", format_size(c.size, LANG), c.name, format_count(c.count, LANG));
                }
            }
        }
        Command::Dupes { paths: roots } => {
            let scan = run_scan(&roots, &settings)?;
            let cache = Mutex::new(HashCache::load(&paths.hash_cache_file()));
            let groups =
                duplicates::find_by_content(&scan, settings.min_duplicate_bytes, &cache, &Progress::default());
            if let Ok(mut guard) = cache.lock() {
                guard.save(&paths.hash_cache_file());
            }
            if cli.json {
                let rows: Vec<_> = groups
                    .iter()
                    .map(|g| {
                        json!({
                            "hash": g.key,
                            "size": g.size,
                            "recoverable": g.recoverable(),
                            "files": g.files.iter().map(|&f| scan.file_path(f)).collect::<Vec<_>>(),
                        })
                    })
                    .collect();
                println!("{}", serde_json::Value::Array(rows));
            } else {
                for group in &groups {
                    println!("{} x{}", format_size(group.size, LANG), group.files.len());
                    for &id in &group.files {
                        println!("    {}", scan.file_path(id).display());
                    }
                }
                let total: u64 = groups.iter().map(DupGroup::recoverable).sum();
                println!("{} groupe(s), {} recuperables", groups.len(), format_size(total, LANG));
            }
        }
        Command::Junk { paths } => {
            let scan = run_scan(&paths, &settings)?;
            let hits = analysis::find_junk(&scan, &settings);
            if cli.json {
                let ids: Vec<FileId> = hits.iter().map(|h| h.file).collect();
                print_files(&scan, &ids, true);
            } else {
                for hit in &hits {
                    println!(
                        "{:>10}  {:<32} {}",
                        format_size(scan.file(hit.file).size, LANG),
                        rule_label(hit.rule).fr,
                        scan.file_path(hit.file).display()
                    );
                }
                println!("{} fichier(s)", format_count(hits.len() as u64, LANG));
            }
        }
        Command::Big { paths, min_mb } => {
            let scan = run_scan(&paths, &settings)?;
            print_files(&scan, &analysis::find_big(&scan, min_mb * 1024 * 1024), cli.json);
        }
        Command::Dev { paths } => {
            let scan = run_scan(&paths, &settings)?;
            let found = analysis::find_dev_artifacts(&scan);
            if cli.json {
                let rows: Vec<_> = found
                    .iter()
                    .map(|a| json!({"path": scan.dir(a.dir).path, "kind": a.kind, "size": a.size}))
                    .collect();
                println!("{}", serde_json::Value::Array(rows));
            } else {
                for artifact in &found {
                    println!(
                        "{:>10}  {:<10} {}",
                        format_size(artifact.size, LANG),
                        artifact.kind,
                        scan.dir(artifact.dir).path.display()
                    );
                }
            }
        }
        Command::Search { path, query } => {
            let scan = run_scan(&[path], &settings)?;
            let query = Query::parse(&query);
            for problem in &query.problems {
                eprintln!("filtre ignore : {problem}");
            }
            print_files(&scan, &query.run(&scan), cli.json);
        }
        Command::Export { path, output, format } => {
            let scan = run_scan(&[path], &settings)?;
            match format {
                Format::Csv => export::export_csv(&scan, &output),
                Format::Json => export::export_json(&scan, &output),
                Format::Html => export::export_html(&scan, &settings, &output),
                Format::Pdf => export::export_pdf(&scan, &settings, &output),
            }
            .context("export")?;
            println!("{}", output.display());
        }
    }
    Ok(())
}
