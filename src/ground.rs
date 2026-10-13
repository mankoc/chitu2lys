//! Where a support standing below a point lands: on the model (the highest surface below it) or on the plate.

use crate::mesh::Mesh;
use glam::{DVec2, DVec3};

/// The model's triangles binned by their XY footprint, for fast vertical ray casts.
pub struct Ground {
    triangles: Vec<[DVec3; 3]>,
    min: DVec2,
    cell: f64,
    columns: usize,
    rows: usize,
    cells: Vec<Vec<u32>>,
}

impl Ground {
    /// Index the triangles of `mesh` (in the frame the queries use).
    pub fn new(mesh: &Mesh) -> Self {
        let point = |i: u32| DVec3::from_array(mesh.vertices[i as usize].map(f64::from));
        let triangles: Vec<[DVec3; 3]> = mesh.faces.iter().map(|f| f.map(point)).collect();
        let (lo, hi) = mesh.bounds().unwrap_or(([0.0; 3], [1.0; 3]));
        let min = DVec2::new(lo[0] as f64, lo[1] as f64);
        let size = DVec2::new(hi[0] as f64, hi[1] as f64) - min;
        // about one triangle per cell on average
        let cell = (size.x * size.y / triangles.len().max(1) as f64).sqrt().max(0.05) * 2.0;
        let columns = ((size.x / cell).ceil() as usize).clamp(1, 4096);
        let rows = ((size.y / cell).ceil() as usize).clamp(1, 4096);
        let mut cells = vec![Vec::new(); columns * rows];
        for (index, t) in triangles.iter().enumerate() {
            let (a, b) = (t[0].min(t[1]).min(t[2]), t[0].max(t[1]).max(t[2]));
            let (c0, r0) = Self::at(min, cell, columns, rows, a.truncate());
            let (c1, r1) = Self::at(min, cell, columns, rows, b.truncate());
            for r in r0..=r1 {
                for c in c0..=c1 {
                    cells[r * columns + c].push(index as u32);
                }
            }
        }
        Self { triangles, min, cell, columns, rows, cells }
    }

    /// Column and row of the cell holding `p` (clamped to the grid).
    fn at(min: DVec2, cell: f64, columns: usize, rows: usize, p: DVec2) -> (usize, usize) {
        let q = ((p - min) / cell).floor();
        ((q.x.max(0.0) as usize).min(columns - 1), (q.y.max(0.0) as usize).min(rows - 1))
    }

    /// The highest point of the model straight below `p` (more than `gap` lower) and the surface's normal there,
    /// pointing up; `None` when nothing is below (the support reaches the plate).
    pub fn below(&self, p: DVec3, gap: f64) -> Option<(DVec3, DVec3)> {
        let flat = p.truncate();
        let size = DVec2::new(self.columns as f64, self.rows as f64) * self.cell;
        if flat.cmplt(self.min).any() || flat.cmpgt(self.min + size).any() {
            return None;
        }
        let (c, r) = Self::at(self.min, self.cell, self.columns, self.rows, flat);
        let mut best: Option<(f64, DVec3)> = None;
        for &index in &self.cells[r * self.columns + c] {
            let [a, b, t] = self.triangles[index as usize];
            let Some(z) = height_at(a, b, t, flat) else { continue };
            if z < p.z - gap && best.is_none_or(|(top, _)| z > top) {
                let mut normal = (b - a).cross(t - a).normalize_or_zero();
                if normal.z < 0.0 {
                    normal = -normal;
                }
                best = Some((z, normal));
            }
        }
        best.map(|(z, normal)| (DVec3::new(p.x, p.y, z), normal))
    }
}

/// Height of triangle `a b c` above the XY point `p`, if `p` lies inside its footprint.
fn height_at(a: DVec3, b: DVec3, c: DVec3, p: DVec2) -> Option<f64> {
    let (a2, b2, c2) = (a.truncate(), b.truncate(), c.truncate());
    let area = (b2 - a2).perp_dot(c2 - a2);
    if area.abs() < 1e-12 {
        return None;
    }
    let u = (p - a2).perp_dot(c2 - a2) / area;
    let v = (b2 - a2).perp_dot(p - a2) / area;
    (u >= 0.0 && v >= 0.0 && u + v <= 1.0).then_some(a.z + u * (b.z - a.z) + v * (c.z - a.z))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_the_highest_surface_below() {
        // two horizontal squares, at z = 0 and z = 5
        let mut mesh = Mesh::default();
        for z in [0.0f32, 5.0] {
            let base = mesh.vertices.len() as u32;
            mesh.vertices.extend([[-1.0, -1.0, z], [1.0, -1.0, z], [1.0, 1.0, z], [-1.0, 1.0, z]]);
            mesh.faces.extend([[base, base + 1, base + 2], [base, base + 2, base + 3]]);
        }
        let ground = Ground::new(&mesh);
        let (hit, normal) = ground.below(DVec3::new(0.2, 0.3, 9.0), 0.01).unwrap();
        assert!((hit.z - 5.0).abs() < 1e-9 && normal.z > 0.99);
        assert!((ground.below(DVec3::new(0.2, 0.3, 4.0), 0.01).unwrap().0.z).abs() < 1e-9);
        assert!(ground.below(DVec3::new(3.0, 0.0, 9.0), 0.01).is_none()); // beside the model
    }
}
