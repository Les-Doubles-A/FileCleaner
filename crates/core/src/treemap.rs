//! Disposition squarifiee (Bruls, Huizing, van Wijk) pour la carte de l'espace disque.

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

fn worst_ratio(row: &[f64], side: f64) -> f64 {
    let sum: f64 = row.iter().sum();
    let (max, min) = row.iter().fold((f64::MIN, f64::MAX), |(a, b), &v| (a.max(v), b.min(v)));
    let s2 = sum * sum;
    let w2 = side * side;
    (w2 * max / s2).max(s2 / (w2 * min))
}

/// `values` doivent etre triees par ordre decroissant et strictement positives.
/// Retourne un rectangle par valeur, dans le meme ordre.
pub fn squarify(values: &[u64], bounds: Rect) -> Vec<Rect> {
    let total: f64 = values.iter().map(|&v| v as f64).sum();
    if total <= 0.0 || bounds.w <= 0.0 || bounds.h <= 0.0 {
        return vec![Rect { x: bounds.x, y: bounds.y, w: 0.0, h: 0.0 }; values.len()];
    }
    let scale = f64::from(bounds.w) * f64::from(bounds.h) / total;
    let areas: Vec<f64> = values.iter().map(|&v| v as f64 * scale).collect();

    let mut out = Vec::with_capacity(values.len());
    let (mut x, mut y) = (f64::from(bounds.x), f64::from(bounds.y));
    let (mut w, mut h) = (f64::from(bounds.w), f64::from(bounds.h));
    let mut start = 0;
    while start < areas.len() {
        let side = w.min(h);
        let mut end = start + 1;
        while end < areas.len()
            && worst_ratio(&areas[start..=end], side) <= worst_ratio(&areas[start..end], side)
        {
            end += 1;
        }
        let row = &areas[start..end];
        let sum: f64 = row.iter().sum();
        if w >= h {
            let col_w = if h > 0.0 { sum / h } else { 0.0 };
            let mut cy = y;
            for &area in row {
                let ch = if col_w > 0.0 { area / col_w } else { 0.0 };
                out.push(Rect { x: x as f32, y: cy as f32, w: col_w as f32, h: ch as f32 });
                cy += ch;
            }
            x += col_w;
            w -= col_w;
        } else {
            let row_h = if w > 0.0 { sum / w } else { 0.0 };
            let mut cx = x;
            for &area in row {
                let cw = if row_h > 0.0 { area / row_h } else { 0.0 };
                out.push(Rect { x: cx as f32, y: y as f32, w: cw as f32, h: row_h as f32 });
                cx += cw;
            }
            y += row_h;
            h -= row_h;
        }
        start = end;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const BOUNDS: Rect = Rect { x: 10.0, y: 20.0, w: 600.0, h: 400.0 };

    #[test]
    fn areas_are_proportional_and_cover_bounds() {
        let values = [600u64, 600, 400, 300, 200, 100, 50, 10];
        let rects = squarify(&values, BOUNDS);
        let total: u64 = values.iter().sum();
        let area: f32 = rects.iter().map(|r| r.w * r.h).sum();
        assert!((area - BOUNDS.w * BOUNDS.h).abs() < 1.0);
        for (r, &v) in rects.iter().zip(&values) {
            let expected = v as f32 / total as f32 * BOUNDS.w * BOUNDS.h;
            assert!((r.w * r.h - expected).abs() < 1.0);
            assert!(r.x >= BOUNDS.x - 0.01 && r.y >= BOUNDS.y - 0.01);
            assert!(r.x + r.w <= BOUNDS.x + BOUNDS.w + 0.01);
            assert!(r.y + r.h <= BOUNDS.y + BOUNDS.h + 0.01);
        }
    }

    #[test]
    fn no_overlap() {
        let values: Vec<u64> = (1..40).rev().map(|v| v * 7).collect();
        let rects = squarify(&values, BOUNDS);
        for (i, a) in rects.iter().enumerate() {
            for b in &rects[i + 1..] {
                let overlap_w = (a.x + a.w).min(b.x + b.w) - a.x.max(b.x);
                let overlap_h = (a.y + a.h).min(b.y + b.h) - a.y.max(b.y);
                assert!(overlap_w < 0.01 || overlap_h < 0.01);
            }
        }
    }

    #[test]
    fn degenerate_inputs() {
        assert!(squarify(&[], BOUNDS).is_empty());
        assert_eq!(squarify(&[0, 0], BOUNDS).len(), 2);
        let zero = Rect { x: 0.0, y: 0.0, w: 0.0, h: 10.0 };
        assert_eq!(squarify(&[5], zero).len(), 1);
    }
}
