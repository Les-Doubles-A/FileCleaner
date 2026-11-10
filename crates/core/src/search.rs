//! Langage de recherche : mots libres, exclusions et filtres.
//!
//! `rapport -brouillon ext:pdf,docx size:>10mb age:>365 in:projets cat:images`
//! `modified:>2024-01-31 re:^IMG_\d+ "nom avec espaces"`

use chrono::{Local, NaiveDate, TimeZone};
use rayon::prelude::*;
use regex::{Regex, RegexBuilder};

use crate::config::category_label;
use crate::model::{FileId, Scan, flags};
use crate::util::{Lang, days_since, strip_accents};

#[derive(Clone, Debug, Default)]
pub struct Query {
    terms: Vec<String>,
    excluded: Vec<String>,
    exts: Vec<String>,
    categories: Vec<String>,
    folders: Vec<String>,
    min_size: Option<u64>,
    max_size: Option<u64>,
    min_age_days: Option<i64>,
    max_age_days: Option<i64>,
    modified_after: Option<i64>,
    modified_before: Option<i64>,
    regex: Option<Regex>,
    hidden_only: bool,
    /// Filtres non compris, signales a l'utilisateur.
    pub problems: Vec<String>,
}

fn normalize(text: &str) -> String {
    strip_accents(text).to_lowercase()
}

pub fn parse_size(text: &str) -> Option<u64> {
    let t = text.trim().to_lowercase().replace(',', ".");
    let split = t.find(|c: char| !c.is_ascii_digit() && c != '.').unwrap_or(t.len());
    let (number, unit) = t.split_at(split);
    let value: f64 = number.parse().ok()?;
    let factor: f64 = match unit.trim() {
        "" | "b" | "o" => 1.0,
        "k" | "kb" | "ko" => 1024.0,
        "m" | "mb" | "mo" => 1024.0 * 1024.0,
        "g" | "gb" | "go" => 1024.0_f64.powi(3),
        "t" | "tb" | "to" => 1024.0_f64.powi(4),
        _ => return None,
    };
    Some((value * factor) as u64)
}

fn parse_date(text: &str) -> Option<i64> {
    let date = NaiveDate::parse_from_str(text, "%Y-%m-%d")
        .or_else(|_| NaiveDate::parse_from_str(text, "%d/%m/%Y"))
        .ok()
        .or_else(|| {
            let year: i32 = text.parse().ok()?;
            NaiveDate::from_ymd_opt(year, 1, 1)
        })?;
    let at = date.and_hms_opt(0, 0, 0)?;
    Local.from_local_datetime(&at).single().map(|d| d.timestamp())
}

/// Decoupe en respectant les guillemets : `in:"mes docs" foo`.
fn tokenize(input: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut quoted = false;
    for c in input.chars() {
        match c {
            '"' => quoted = !quoted,
            c if c.is_whitespace() && !quoted => {
                if !current.is_empty() {
                    tokens.push(std::mem::take(&mut current));
                }
            }
            c => current.push(c),
        }
    }
    if !current.is_empty() {
        tokens.push(current);
    }
    tokens
}

impl Query {
    pub fn parse(input: &str) -> Self {
        let mut q = Self::default();
        for token in tokenize(input) {
            let Some((key, value)) = token.split_once(':').filter(|(k, _)| k.len() > 1) else {
                match token.strip_prefix('-').filter(|t| !t.is_empty()) {
                    Some(word) => q.excluded.push(normalize(word)),
                    None => q.terms.push(normalize(&token)),
                }
                continue;
            };
            q.apply_filter(&key.to_lowercase(), value, &token);
        }
        q
    }

    fn apply_filter(&mut self, key: &str, value: &str, token: &str) {
        match key {
            "ext" | "type" => self.exts.extend(
                value.split(',').map(|e| e.trim().trim_start_matches('.').to_lowercase()).filter(|e| !e.is_empty()),
            ),
            "cat" | "categorie" | "category" => self.categories.push(normalize(value)),
            "in" | "dans" | "path" => self.folders.push(normalize(value)),
            "size" | "taille" => self.apply_range(value, token, parse_size, RangeField::Size),
            "age" => self.apply_range(value, token, |v| v.parse().ok(), RangeField::Age),
            "modified" | "modifie" | "date" => self.apply_date(value, token),
            "re" | "regex" => match RegexBuilder::new(value).case_insensitive(true).build() {
                Ok(re) => self.regex = Some(re),
                Err(_) => self.problems.push(token.to_string()),
            },
            "is" if value == "hidden" || value == "cache" => self.hidden_only = true,
            _ => self.problems.push(token.to_string()),
        }
    }

    fn apply_range(&mut self, value: &str, token: &str, parse: impl Fn(&str) -> Option<u64>, field: RangeField) {
        let (low, high) = if let Some((a, b)) = value.split_once("..") {
            (parse(a), parse(b))
        } else if let Some(v) = value.strip_prefix(">=").or(value.strip_prefix('>')) {
            (parse(v), None)
        } else if let Some(v) = value.strip_prefix("<=").or(value.strip_prefix('<')) {
            (None, parse(v))
        } else {
            let v = parse(value);
            (v, v)
        };
        if low.is_none() && high.is_none() {
            self.problems.push(token.to_string());
            return;
        }
        match field {
            RangeField::Size => {
                self.min_size = low.or(self.min_size);
                self.max_size = high.or(self.max_size);
            }
            RangeField::Age => {
                self.min_age_days = low.map(|v| v as i64).or(self.min_age_days);
                self.max_age_days = high.map(|v| v as i64).or(self.max_age_days);
            }
        }
    }

    fn apply_date(&mut self, value: &str, token: &str) {
        let (after, text) = if let Some(v) = value.strip_prefix('>') {
            (true, v)
        } else if let Some(v) = value.strip_prefix('<') {
            (false, v)
        } else {
            (true, value)
        };
        match (parse_date(text), after) {
            (Some(ts), true) => self.modified_after = Some(ts),
            (Some(ts), false) => self.modified_before = Some(ts),
            (None, _) => self.problems.push(token.to_string()),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.terms.is_empty()
            && self.excluded.is_empty()
            && self.exts.is_empty()
            && self.categories.is_empty()
            && self.folders.is_empty()
            && self.min_size.is_none()
            && self.max_size.is_none()
            && self.min_age_days.is_none()
            && self.max_age_days.is_none()
            && self.modified_after.is_none()
            && self.modified_before.is_none()
            && self.regex.is_none()
            && !self.hidden_only
    }

    pub fn matches(&self, scan: &Scan, id: FileId) -> bool {
        let f = scan.file(id);
        if self.min_size.is_some_and(|m| f.size < m) || self.max_size.is_some_and(|m| f.size > m) {
            return false;
        }
        if self.modified_after.is_some_and(|t| f.modified < t)
            || self.modified_before.is_some_and(|t| f.modified > t)
        {
            return false;
        }
        if self.min_age_days.is_some() || self.max_age_days.is_some() {
            let age = days_since(f.modified);
            if self.min_age_days.is_some_and(|d| age < d) || self.max_age_days.is_some_and(|d| age > d) {
                return false;
            }
        }
        if !self.exts.is_empty() && !self.exts.iter().any(|e| e == scan.ext_of(f)) {
            return false;
        }
        if self.hidden_only && !f.has(flags::HIDDEN) {
            return false;
        }
        if !self.categories.is_empty() {
            let fr = normalize(scan.category_of(f));
            let en = normalize(category_label(scan.category_of(f), Lang::En));
            if !self.categories.iter().any(|c| fr.contains(c) || en.contains(c)) {
                return false;
            }
        }
        if let Some(re) = &self.regex
            && !re.is_match(&f.name) {
                return false;
            }
        if !self.terms.is_empty() || !self.excluded.is_empty() {
            let name = normalize(&f.name);
            if !self.terms.iter().all(|t| name.contains(t)) || self.excluded.iter().any(|t| name.contains(t)) {
                return false;
            }
        }
        if !self.folders.is_empty() {
            let folder = normalize(&scan.folder_of(id).to_string_lossy());
            if !self.folders.iter().all(|t| folder.contains(t)) {
                return false;
            }
        }
        true
    }

    pub fn run(&self, scan: &Scan) -> Vec<FileId> {
        (0..scan.files.len() as FileId)
            .into_par_iter()
            .filter(|&id| scan.file(id).is_alive() && self.matches(scan, id))
            .collect()
    }
}

enum RangeField {
    Size,
    Age,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Settings;
    use crate::model::Progress;
    use crate::scanner::scan;
    use std::fs;

    fn fixture() -> (tempfile::TempDir, Scan) {
        let dir = tempfile::tempdir().unwrap();
        let r = dir.path();
        fs::create_dir_all(r.join("Projets/été")).unwrap();
        fs::write(r.join("Projets/été/Rapport final.pdf"), vec![0u8; 2048]).unwrap();
        fs::write(r.join("Projets/brouillon rapport.docx"), vec![0u8; 10]).unwrap();
        fs::write(r.join("IMG_1234.JPG"), vec![0u8; 5000]).unwrap();
        let s = scan(&[r.to_path_buf()], &Settings::default(), &Progress::default());
        (dir, s)
    }

    fn names(s: &Scan, q: &str) -> Vec<String> {
        let query = Query::parse(q);
        assert!(query.problems.is_empty(), "{:?}", query.problems);
        let mut out: Vec<_> = query.run(s).iter().map(|&id| s.file(id).name.to_string()).collect();
        out.sort();
        out
    }

    #[test]
    fn words_exclusions_and_accents() {
        let (_d, s) = fixture();
        assert_eq!(names(&s, "rapport -brouillon"), ["Rapport final.pdf"]);
        assert_eq!(names(&s, "in:ete"), ["Rapport final.pdf"]);
        assert_eq!(names(&s, "\"rapport final\""), ["Rapport final.pdf"]);
    }

    #[test]
    fn filters_combine() {
        let (_d, s) = fixture();
        assert_eq!(names(&s, "ext:jpg,pdf size:>3kb"), ["IMG_1234.JPG"]);
        assert_eq!(names(&s, "size:1kb..3ko"), ["Rapport final.pdf"]);
        assert_eq!(names(&s, "cat:images"), ["IMG_1234.JPG"]);
        assert_eq!(names(&s, r"re:^img_\d+"), ["IMG_1234.JPG"]);
        assert_eq!(names(&s, "age:<1").len(), 3);
        assert_eq!(names(&s, "modified:<2001").len(), 0);
    }

    #[test]
    fn size_parser() {
        assert_eq!(parse_size("10mb"), Some(10 * 1024 * 1024));
        assert_eq!(parse_size("1,5 Go"), Some(1610612736));
        assert_eq!(parse_size("bad"), None);
    }

    #[test]
    fn bad_filters_are_reported_not_fatal() {
        let q = Query::parse("foo size:huge bar:baz re:(");
        assert_eq!(q.problems.len(), 3);
        assert!(!q.is_empty());
    }

    #[test]
    fn windows_path_like_term_is_not_a_filter() {
        assert!(Query::parse("c:temp").problems.is_empty());
    }
}
