//! A triangle mesh: the models and support parts read from CHITUBOX, and the meshes written to Lychee.

use rayon::prelude::*;

/// Triangle mesh as plain arrays: float32 positions and 32-bit triangle indices.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Mesh {
    /// Vertex positions.
    pub vertices: Vec<[f32; 3]>,
    /// Triangles as three indices into `vertices`, counter-clockwise seen from outside.
    pub faces: Vec<[u32; 3]>,
}

impl Mesh {
    pub fn new(vertices: Vec<[f32; 3]>, faces: Vec<[u32; 3]>) -> Self {
        Self { vertices, faces }
    }

    /// Axis-aligned bounds `(min, max)`; `None` for a mesh without vertices.
    pub fn bounds(&self) -> Option<([f32; 3], [f32; 3])> {
        let first = *self.vertices.first()?;
        Some(self.vertices.iter().fold((first, first), |(mut lo, mut hi), v| {
            for i in 0..3 {
                lo[i] = lo[i].min(v[i]);
                hi[i] = hi[i].max(v[i]);
            }
            (lo, hi)
        }))
    }

    /// Indexed mesh from triangle corner coordinates (three per triangle), merging corners at identical
    /// positions (bit for bit).
    pub fn from_corners(corners: &[[f32; 3]]) -> Self {
        assert!(corners.len() % 3 == 0, "corner count must be a multiple of 3");
        let key = |c: &[f32; 3]| ((c[0].to_bits() as u64) << 32 | c[1].to_bits() as u64, c[2].to_bits());
        let mut order: Vec<u32> = (0..corners.len() as u32).collect();
        order.par_sort_unstable_by_key(|&i| key(&corners[i as usize]));

        let mut vertices = Vec::new();
        let mut remap = vec![0u32; corners.len()];
        let mut previous = None;
        for &i in &order {
            let k = key(&corners[i as usize]);
            if previous != Some(k) {
                vertices.push(corners[i as usize]);
                previous = Some(k);
            }
            remap[i as usize] = (vertices.len() - 1) as u32;
        }
        let faces = remap.chunks_exact(3).map(|t| [t[0], t[1], t[2]]).collect();
        Self::new(vertices, faces)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_corners_merges_shared_corners_and_keeps_triangles() {
        let quad = [[0., 0., 0.], [1., 0., 0.], [1., 1., 0.], [0., 0., 0.], [1., 1., 0.], [0., 1., 0.]];
        let mesh = Mesh::from_corners(&quad);
        assert_eq!(mesh.vertices.len(), 4);
        let rebuilt: Vec<[f32; 3]> = mesh.faces.iter().flatten().map(|&i| mesh.vertices[i as usize]).collect();
        assert_eq!(rebuilt, quad);
        assert_eq!(mesh.bounds(), Some(([0.0; 3], [1.0, 1.0, 0.0])));
    }
}
