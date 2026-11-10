//! Images visuellement proches par hachage perceptuel (dHash 64 bits).

use std::path::Path;
use std::sync::atomic::Ordering;

use image::ImageReader;
use image::imageops::FilterType;
use rayon::prelude::*;

use crate::model::{FileId, Progress, Scan, flags};

const SUPPORTED: &[&str] = &["jpg", "jpeg", "png", "webp", "bmp", "gif", "tif", "tiff"];
const MAX_IMAGE_BYTES: u64 = 80 * 1024 * 1024;
const MAX_IMAGES: usize = 30_000;

#[derive(Clone, Debug)]
pub struct SimilarItem {
    pub file: FileId,
    pub width: u32,
    pub height: u32,
}

#[derive(Clone, Debug)]
pub struct SimilarGroup {
    /// Trie par resolution decroissante : le premier est le meilleur exemplaire.
    pub items: Vec<SimilarItem>,
}

impl SimilarGroup {
    pub fn recoverable(&self, scan: &Scan) -> u64 {
        self.items.iter().skip(1).map(|i| scan.file(i.file).size).sum()
    }
}

/// dHash : compare chaque pixel a son voisin sur une miniature 9x8 en niveaux de gris.
pub fn dhash(path: &Path) -> Option<(u64, u32, u32)> {
    let reader = ImageReader::open(path).ok()?.with_guessed_format().ok()?;
    let image = reader.decode().ok()?;
    let (width, height) = (image.width(), image.height());
    if width < 16 || height < 16 {
        return None;
    }
    let small = image.resize_exact(9, 8, FilterType::Triangle).to_luma8();
    let mut hash = 0u64;
    for y in 0..8 {
        for x in 0..8 {
            hash <<= 1;
            if small.get_pixel(x, y).0[0] < small.get_pixel(x + 1, y).0[0] {
                hash |= 1;
            }
        }
    }
    Some((hash, width, height))
}

struct UnionFind(Vec<u32>);

impl UnionFind {
    fn find(&mut self, mut x: u32) -> u32 {
        while self.0[x as usize] != x {
            let parent = self.0[x as usize];
            self.0[x as usize] = self.0[parent as usize];
            x = parent;
        }
        x
    }
    fn union(&mut self, a: u32, b: u32) {
        let (ra, rb) = (self.find(a), self.find(b));
        if ra != rb {
            self.0[ra as usize] = rb;
        }
    }
}

/// Regroupe des empreintes dont la distance de Hamming est <= `threshold`.
pub fn cluster(hashes: &[u64], threshold: u32) -> Vec<Vec<usize>> {
    let pairs: Vec<(u32, u32)> = (0..hashes.len())
        .into_par_iter()
        .flat_map_iter(|i| {
            (i + 1..hashes.len())
                .filter(move |&j| (hashes[i] ^ hashes[j]).count_ones() <= threshold)
                .map(move |j| (i as u32, j as u32))
        })
        .collect();
    let mut uf = UnionFind((0..hashes.len() as u32).collect());
    for (a, b) in pairs {
        uf.union(a, b);
    }
    let mut clusters: std::collections::HashMap<u32, Vec<usize>> = std::collections::HashMap::new();
    for i in 0..hashes.len() {
        clusters.entry(uf.find(i as u32)).or_default().push(i);
    }
    clusters.into_values().filter(|c| c.len() > 1).collect()
}

pub fn find_similar(scan: &Scan, threshold: u32, progress: &Progress) -> Vec<SimilarGroup> {
    let mut images: Vec<FileId> = scan
        .alive()
        .filter(|(_, f)| {
            f.size <= MAX_IMAGE_BYTES
                && f.size > 0
                && !f.has(flags::CLOUD)
                && SUPPORTED.contains(&scan.ext_of(f))
        })
        .map(|(id, _)| id)
        .collect();
    images.truncate(MAX_IMAGES);
    progress.set_phase("images");
    progress.done.store(0, Ordering::Relaxed);
    progress.total.store(images.len() as u64, Ordering::Relaxed);

    let hashed: Vec<(FileId, u64, u32, u32)> = images
        .par_iter()
        .filter_map(|&id| {
            if progress.is_cancelled() {
                return None;
            }
            let path = scan.file_path(id);
            progress.set_current(&path.to_string_lossy());
            let result = dhash(&path);
            progress.done.fetch_add(1, Ordering::Relaxed);
            result.map(|(hash, w, h)| (id, hash, w, h))
        })
        .collect();
    if progress.is_cancelled() {
        return Vec::new();
    }

    let hashes: Vec<u64> = hashed.iter().map(|h| h.1).collect();
    let mut groups: Vec<SimilarGroup> = cluster(&hashes, threshold)
        .into_iter()
        .map(|indices| {
            let mut items: Vec<SimilarItem> = indices
                .into_iter()
                .map(|i| SimilarItem { file: hashed[i].0, width: hashed[i].2, height: hashed[i].3 })
                .collect();
            items.sort_by_key(|i| {
                std::cmp::Reverse((u64::from(i.width) * u64::from(i.height), scan.file(i.file).size))
            });
            SimilarGroup { items }
        })
        .collect();
    groups.sort_by_key(|g| std::cmp::Reverse(g.recoverable(scan)));
    groups
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Settings;
    use crate::scanner::scan;
    use image::{ImageBuffer, Rgb};

    fn gradient(width: u32, height: u32, invert: bool) -> ImageBuffer<Rgb<u8>, Vec<u8>> {
        ImageBuffer::from_fn(width, height, |x, y| {
            let v = (x * 255 / width) as u8 ^ ((y * 8 / height) as u8 * 16);
            Rgb([if invert { 255 - v } else { v }; 3])
        })
    }

    #[test]
    fn resized_copy_is_similar_and_best_resolution_comes_first() {
        let dir = tempfile::tempdir().unwrap();
        gradient(400, 300, false).save(dir.path().join("big.png")).unwrap();
        gradient(200, 150, false).save(dir.path().join("small.png")).unwrap();
        gradient(400, 300, true).save(dir.path().join("other.png")).unwrap();
        let s = scan(&[dir.path().to_path_buf()], &Settings::default(), &Default::default());
        let groups = find_similar(&s, 6, &Progress::default());
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].items.len(), 2);
        assert_eq!(&*s.file(groups[0].items[0].file).name, "big.png");
    }

    #[test]
    fn cluster_is_transitive() {
        let clusters = cluster(&[0b0000, 0b0011, 0b1111, u64::MAX], 2);
        assert_eq!(clusters.len(), 1);
        assert_eq!(clusters[0].len(), 3);
    }

    #[test]
    fn corrupt_image_is_skipped() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("bad.png"), b"not an image").unwrap();
        assert!(dhash(&dir.path().join("bad.png")).is_none());
    }
}
