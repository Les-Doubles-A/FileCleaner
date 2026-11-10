//! Export local en CSV, JSON, HTML et PDF.

use std::fmt::Write as _;
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::Path;

use anyhow::{Context, Result};
use serde_json::json;

use crate::analysis::Insights;
use crate::config::{Settings, category_label};
use crate::model::{FileId, Scan};
use crate::util::{Lang, format_count, format_datetime, format_size};

/// Neutralise l'injection de formules dans un tableur.
fn csv_field(value: &str) -> String {
    let guarded = if value.starts_with(['=', '+', '-', '@', '\t', '\r']) {
        format!("'{value}")
    } else {
        value.to_string()
    };
    if guarded.contains([',', ';', '"', '\n', '\r']) {
        format!("\"{}\"", guarded.replace('"', "\"\""))
    } else {
        guarded
    }
}

pub fn export_csv(scan: &Scan, output: &Path) -> Result<()> {
    let file = File::create(output).with_context(|| format!("creation de {}", output.display()))?;
    let mut w = BufWriter::new(file);
    w.write_all(b"\xEF\xBB\xBF")?;
    writeln!(w, "chemin;nom;extension;taille_octets;cree;modifie;accede;categorie")?;
    for (id, f) in scan.alive() {
        writeln!(
            w,
            "{};{};{};{};{};{};{};{}",
            csv_field(&scan.file_path(id).to_string_lossy()),
            csv_field(&f.name),
            csv_field(scan.ext_of(f)),
            f.size,
            format_datetime(f.created, Lang::En),
            format_datetime(f.modified, Lang::En),
            format_datetime(f.accessed, Lang::En),
            csv_field(scan.category_of(f)),
        )?;
    }
    w.flush()?;
    Ok(())
}

pub fn export_json(scan: &Scan, output: &Path) -> Result<()> {
    let files: Vec<_> = scan
        .alive()
        .map(|(id, f)| {
            json!({
                "path": scan.file_path(id),
                "name": &*f.name,
                "extension": scan.ext_of(f),
                "size": f.size,
                "created": f.created,
                "modified": f.modified,
                "accessed": f.accessed,
                "category": scan.category_of(f),
            })
        })
        .collect();
    let data = json!({
        "roots": scan.roots,
        "scanned_at": scan.scanned_at,
        "total_files": scan.total_files(),
        "total_dirs": scan.total_dirs(),
        "total_size": scan.total_size(),
        "categories": scan.category_breakdown(),
        "extensions": scan.extension_breakdown(),
        "errors": scan.errors,
        "files": files,
    });
    let file = File::create(output).with_context(|| format!("creation de {}", output.display()))?;
    serde_json::to_writer_pretty(BufWriter::new(file), &data)?;
    Ok(())
}

fn html_escape(text: &str) -> String {
    text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

fn largest(scan: &Scan, n: usize) -> Vec<FileId> {
    let mut ids: Vec<FileId> = scan.alive().map(|(id, _)| id).collect();
    ids.sort_by_key(|&id| std::cmp::Reverse(scan.file(id).size));
    ids.truncate(n);
    ids
}

pub fn export_html(scan: &Scan, settings: &Settings, output: &Path) -> Result<()> {
    let lang = settings.language;
    let t = |fr: &'static str, en: &'static str| if lang == Lang::Fr { fr } else { en };
    let insights = Insights::compute(scan, settings);
    let total = scan.total_size().max(1);
    let mut html = String::new();
    write!(
        html,
        "<!doctype html><html lang=\"{}\"><meta charset=\"utf-8\"><title>File Cleaner</title><style>\
         :root{{color-scheme:light dark;--bg:#f6f7f9;--card:#fff;--text:#1a1d21;--muted:#667;--line:#e3e6ea;--accent:#2f6fed}}\
         @media(prefers-color-scheme:dark){{:root{{--bg:#14161a;--card:#1c1f25;--text:#e8eaed;--muted:#9aa3ad;--line:#2b2f37;--accent:#6ea0ff}}}}\
         body{{font:15px/1.5 'Segoe UI',system-ui,sans-serif;background:var(--bg);color:var(--text);margin:0 auto;max-width:1000px;padding:32px 16px}}\
         h1{{font-size:26px;margin:0}}h2{{font-size:17px;margin:32px 0 10px}}.m{{color:var(--muted)}}\
         .g{{display:grid;grid-template-columns:repeat(auto-fit,minmax(170px,1fr));gap:12px;margin-top:20px}}\
         .c{{background:var(--card);border:1px solid var(--line);border-radius:10px;padding:14px}}.c b{{display:block;font-size:22px}}\
         table{{width:100%;border-collapse:collapse;background:var(--card);border:1px solid var(--line);table-layout:fixed}}\
         td,th{{padding:7px 12px;border-bottom:1px solid var(--line);text-align:left;overflow-wrap:anywhere}}\
         th{{font-weight:600}}td.n{{text-align:right;white-space:nowrap;width:110px}}\
         .b{{height:6px;background:var(--accent);border-radius:3px}}</style>",
        if lang == Lang::Fr { "fr" } else { "en" }
    )?;
    write!(
        html,
        "<h1>{}</h1><div class=m>{} &middot; {}</div><div class=g>",
        t("Rapport d'analyse", "Scan report"),
        html_escape(&scan.title()),
        format_datetime(scan.scanned_at, lang),
    )?;
    let cards = [
        (t("Taille totale", "Total size"), format_size(scan.total_size(), lang)),
        (t("Fichiers", "Files"), format_count(scan.total_files(), lang)),
        (t("Dossiers", "Folders"), format_count(scan.total_dirs(), lang)),
        (t("Fichiers inutiles", "Junk files"), format_size(insights.junk_bytes, lang)),
        (t("Artefacts de dev", "Dev artifacts"), format_size(insights.dev_bytes, lang)),
        (t("Dossiers vides", "Empty folders"), format_count(insights.empty_dirs, lang)),
    ];
    for (label, value) in cards {
        write!(html, "<div class=c><span class=m>{label}</span><b>{value}</b></div>")?;
    }
    write!(html, "</div><h2>{}</h2><table>", t("Répartition par catégorie", "Breakdown by category"))?;
    for c in scan.category_breakdown() {
        let width = c.size as f64 / total as f64 * 100.0;
        write!(
            html,
            "<tr><td>{}<div class=b style=\"width:{width:.1}%\"></div></td><td class=n>{}</td><td class=n>{}</td></tr>",
            html_escape(category_label(&c.name, lang)),
            format_count(c.count, lang),
            format_size(c.size, lang),
        )?;
    }
    write!(html, "</table><h2>{}</h2><table>", t("Plus gros fichiers", "Largest files"))?;
    for id in largest(scan, 25) {
        write!(
            html,
            "<tr><td>{}<div class=m>{}</div></td><td class=n>{}</td></tr>",
            html_escape(&scan.file(id).name),
            html_escape(&scan.folder_of(id).to_string_lossy()),
            format_size(scan.file(id).size, lang),
        )?;
    }
    html.push_str("</table></html>");
    std::fs::write(output, html).with_context(|| format!("ecriture de {}", output.display()))?;
    Ok(())
}

/// Encode en WinAnsi (Windows-1252) pour les polices PDF standard.
fn win_ansi(text: &str) -> Vec<u8> {
    text.chars()
        .map(|c| match c {
            '\u{20AC}' => 0x80,
            '\u{2019}' => 0x92,
            '\u{2013}' => 0x96,
            '\u{2014}' => 0x97,
            '\u{0153}' => 0x9C,
            '\u{0152}' => 0x8C,
            '\u{202F}' => 0xA0,
            c if (c as u32) < 0x80 || (0xA0..=0xFF).contains(&(c as u32)) => c as u32 as u8,
            _ => b'?',
        })
        .collect()
}

fn pdf_string(text: &str) -> Vec<u8> {
    let mut out = vec![b'('];
    for b in win_ansi(text) {
        if matches!(b, b'(' | b')' | b'\\') {
            out.push(b'\\');
        }
        out.push(b);
    }
    out.push(b')');
    out
}

/// Generateur PDF minimal multi-pages, sans dependance.
struct Pdf {
    pages: Vec<Vec<u8>>,
    current: Vec<u8>,
    y: f32,
}

const PAGE_W: f32 = 595.0;
const PAGE_H: f32 = 842.0;
const MARGIN: f32 = 48.0;

impl Pdf {
    fn new() -> Self {
        Self { pages: Vec::new(), current: Vec::new(), y: PAGE_H - MARGIN }
    }

    fn ensure_room(&mut self, height: f32) {
        if self.y - height < MARGIN {
            self.pages.push(std::mem::take(&mut self.current));
            self.y = PAGE_H - MARGIN;
        }
    }

    fn text_at(&mut self, x: f32, y: f32, size: f32, bold: bool, text: &str) {
        let font = if bold { "F2" } else { "F1" };
        let head = format!("BT /{font} {size} Tf {x:.1} {y:.1} Td ");
        self.current.extend_from_slice(head.as_bytes());
        self.current.extend_from_slice(&pdf_string(text));
        self.current.extend_from_slice(b" Tj ET\n");
    }

    fn line(&mut self, size: f32, bold: bool, text: &str) {
        self.ensure_room(size + 6.0);
        self.y -= size + 6.0;
        let y = self.y;
        self.text_at(MARGIN, y, size, bold, text);
    }

    /// Texte a gauche, valeur alignee a droite.
    fn row(&mut self, size: f32, left: &str, right: &str) {
        self.ensure_room(size + 5.0);
        self.y -= size + 5.0;
        let y = self.y;
        let max_chars = ((PAGE_W - 2.0 * MARGIN - 80.0) / (size * 0.5)) as usize;
        let clipped: String = if left.chars().count() > max_chars {
            let tail: String = left.chars().rev().take(max_chars - 3).collect::<Vec<_>>().into_iter().rev().collect();
            format!("...{tail}")
        } else {
            left.to_string()
        };
        self.text_at(MARGIN, y, size, false, &clipped);
        let x = PAGE_W - MARGIN - right.chars().count() as f32 * size * 0.52;
        self.text_at(x, y, size, false, right);
    }

    fn gap(&mut self, height: f32) {
        self.y -= height;
    }

    fn finish(mut self) -> Vec<u8> {
        if !self.current.is_empty() || self.pages.is_empty() {
            self.pages.push(std::mem::take(&mut self.current));
        }
        let page_count = self.pages.len();
        // Objets : 1 catalogue, 2 pages, 3-4 polices, puis page/contenu par page.
        let mut objects: Vec<Vec<u8>> = vec![
            b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
            format!(
                "<< /Type /Pages /Count {page_count} /Kids [{}] >>",
                (0..page_count).map(|i| format!("{} 0 R", 5 + i * 2)).collect::<Vec<_>>().join(" ")
            )
            .into_bytes(),
            b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>".to_vec(),
            b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica-Bold /Encoding /WinAnsiEncoding >>".to_vec(),
        ];
        for (i, content) in self.pages.iter().enumerate() {
            objects.push(
                format!(
                    "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {PAGE_W} {PAGE_H}] \
                     /Resources << /Font << /F1 3 0 R /F2 4 0 R >> >> /Contents {} 0 R >>",
                    6 + i * 2
                )
                .into_bytes(),
            );
            let mut stream = format!("<< /Length {} >>\nstream\n", content.len()).into_bytes();
            stream.extend_from_slice(content);
            stream.extend_from_slice(b"\nendstream");
            objects.push(stream);
        }

        let mut out = b"%PDF-1.4\n%\xE2\xE3\xCF\xD3\n".to_vec();
        let mut offsets = Vec::with_capacity(objects.len());
        for (i, object) in objects.iter().enumerate() {
            offsets.push(out.len());
            out.extend_from_slice(format!("{} 0 obj\n", i + 1).as_bytes());
            out.extend_from_slice(object);
            out.extend_from_slice(b"\nendobj\n");
        }
        let xref = out.len();
        out.extend_from_slice(format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes());
        for offset in offsets {
            out.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
        }
        out.extend_from_slice(
            format!("trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n", objects.len() + 1).as_bytes(),
        );
        out
    }
}

pub fn export_pdf(scan: &Scan, settings: &Settings, output: &Path) -> Result<()> {
    let lang = settings.language;
    let t = |fr: &'static str, en: &'static str| if lang == Lang::Fr { fr } else { en };
    let insights = Insights::compute(scan, settings);
    let mut pdf = Pdf::new();
    pdf.line(20.0, true, t("File Cleaner - Rapport d'analyse", "File Cleaner - Scan report"));
    pdf.line(10.0, false, &scan.title());
    pdf.line(10.0, false, &format_datetime(scan.scanned_at, lang));
    pdf.gap(10.0);
    pdf.line(13.0, true, t("Synthèse", "Summary"));
    let rows = [
        (t("Taille totale", "Total size"), format_size(scan.total_size(), lang)),
        (t("Fichiers", "Files"), format_count(scan.total_files(), lang)),
        (t("Dossiers", "Folders"), format_count(scan.total_dirs(), lang)),
        (t("Fichiers inutiles", "Junk files"), format_size(insights.junk_bytes, lang)),
        (t("Artefacts de développement", "Dev artifacts"), format_size(insights.dev_bytes, lang)),
        (t("Dossiers vides", "Empty folders"), format_count(insights.empty_dirs, lang)),
    ];
    for (label, value) in rows {
        pdf.row(10.0, label, &value);
    }
    pdf.gap(10.0);
    pdf.line(13.0, true, t("Répartition par catégorie", "Breakdown by category"));
    for c in scan.category_breakdown() {
        pdf.row(10.0, category_label(&c.name, lang), &format_size(c.size, lang));
    }
    pdf.gap(10.0);
    pdf.line(13.0, true, t("Plus gros fichiers", "Largest files"));
    for id in largest(scan, 40) {
        pdf.row(9.0, &scan.file_path(id).to_string_lossy(), &format_size(scan.file(id).size, lang));
    }
    std::fs::write(output, pdf.finish()).with_context(|| format!("ecriture de {}", output.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Progress;
    use crate::scanner::scan;
    use std::fs;

    fn fixture() -> (tempfile::TempDir, Scan) {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("=danger;.txt"), b"abc").unwrap();
        fs::write(dir.path().join("été & co.jpg"), vec![0u8; 2048]).unwrap();
        let s = scan(&[dir.path().to_path_buf()], &Settings::default(), &Progress::default());
        (dir, s)
    }

    #[test]
    fn csv_is_excel_safe() {
        let (dir, s) = fixture();
        let out = dir.path().join("o.csv");
        export_csv(&s, &out).unwrap();
        let text = fs::read_to_string(out).unwrap();
        assert!(text.starts_with('\u{feff}'));
        assert!(text.contains("\"'=danger;.txt\""));
        assert_eq!(text.lines().count(), 3);
    }

    #[test]
    fn json_is_valid() {
        let (dir, s) = fixture();
        let out = dir.path().join("o.json");
        export_json(&s, &out).unwrap();
        let v: serde_json::Value = serde_json::from_str(&fs::read_to_string(out).unwrap()).unwrap();
        assert_eq!(v["total_files"], 2);
        assert_eq!(v["files"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn html_escapes_names() {
        let (dir, s) = fixture();
        let out = dir.path().join("o.html");
        export_html(&s, &Settings::default(), &out).unwrap();
        let text = fs::read_to_string(out).unwrap();
        assert!(text.contains("été &amp; co.jpg"));
        assert!(!text.contains("été & co.jpg"));
    }

    #[test]
    fn pdf_structure_is_consistent() {
        let (dir, s) = fixture();
        let out = dir.path().join("o.pdf");
        export_pdf(&s, &Settings::default(), &out).unwrap();
        let bytes = fs::read(out).unwrap();
        assert!(bytes.starts_with(b"%PDF-1.4"));
        let text = String::from_utf8_lossy(&bytes);
        let start: usize = text.rsplit("startxref\n").next().unwrap().lines().next().unwrap().parse().unwrap();
        assert_eq!(&bytes[start..start + 4], b"xref");
    }

    #[test]
    fn pdf_paginates() {
        let mut pdf = Pdf::new();
        for i in 0..200 {
            pdf.line(10.0, false, &format!("ligne {i}"));
        }
        let text = String::from_utf8_lossy(&pdf.finish()).into_owned();
        assert!(text.contains("/Count 5"), "46 lignes par page");
    }

    #[test]
    fn win_ansi_keeps_french() {
        assert_eq!(win_ansi("é"), [0xE9]);
        assert_eq!(win_ansi("œ"), [0x9C]);
        assert_eq!(win_ansi("日"), [b'?']);
    }
}
