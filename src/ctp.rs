//! Reading CHITUBOX `.ctp` projects, the newer layout of CHITUBOX projects (`docs/CTP_NOTES.md`): the same
//! models and support parts as `.chitubox` projects (`chitubox.rs`), with 64-bit offsets, indexed meshes and
//! support parts that carry no triangles.
//!
//! In short, all little-endian:
//!
//! - header: magic `0xAB231253`, object count, the offset of the object table (u64 at 24) and of the preview header
//!   (u64 at 48);
//! - object record (216 bytes, magic `0xAB231254`): X/Y position, half the mesh's height, lift, and the length and
//!   offset of the name, the offset of the mesh and the offset and count of the support records;
//! - mesh (magic `0xAB231255`): the offset and count of the vertices (3 × f32, centred on the origin) and of the
//!   triangle indices (u32);
//! - support records (40 bytes, magic `0xAB231256`): a part count and the offset of the parts; a part (48 bytes) is
//!   its type, a style word, end points A and B, the radius at each, and two more floats (0 in the sample).

use glam::DVec3;

use crate::chitubox::{Model, Object, Part, decode_preview};

/// Magic numbers of the file, an object record, a mesh and a support record.
const MAGIC: u32 = 0xAB23_1253;
const OBJECT_MAGIC: u32 = 0xAB23_1254;
const MESH_MAGIC: u32 = 0xAB23_1255;
const SUPPORT_MAGIC: u32 = 0xAB23_1256;
/// Sizes of an object record, a support record and a support part.
const OBJECT: usize = 216;
const SUPPORT: usize = 40;
const PART: usize = 48;

/// A little-endian u32, u64 and f32 at `at`, if the data is long enough.
fn word(data: &[u8], at: usize) -> Option<u32> {
    data.get(at..at.checked_add(4)?).map(|b| u32::from_le_bytes(b.try_into().unwrap()))
}

fn long(data: &[u8], at: usize) -> Option<usize> {
    data.get(at..at.checked_add(8)?).and_then(|b| usize::try_from(u64::from_le_bytes(b.try_into().unwrap())).ok())
}

fn float(data: &[u8], at: usize) -> Option<f64> {
    word(data, at).map(|w| f32::from_bits(w) as f64)
}

fn point(data: &[u8], at: usize) -> Option<DVec3> {
    Some(DVec3::new(float(data, at)?, float(data, at + 4)?, float(data, at + 8)?))
}

/// Whether `data` is a `.ctp` project.
pub fn is_ctp(data: &[u8]) -> bool {
    word(data, 0) == Some(MAGIC)
}

/// The objects of a project (in table order) and the objects that could not be read, as `"<name>: <reason>"`.
pub fn objects(data: &[u8]) -> Result<(Vec<Object>, Vec<String>), String> {
    if !is_ctp(data) {
        return Err("not a CHITUBOX .ctp project (unknown file signature)".into());
    }
    let count = word(data, 4).unwrap_or(0) as usize;
    let table = long(data, 24).unwrap_or(0);
    if count == 0 || table.saturating_add(count.saturating_mul(OBJECT)) > data.len() {
        return Err("the object table is missing or cut short".into());
    }
    let (mut objects, mut skipped) = (Vec::new(), Vec::new());
    for i in 0..count {
        match read_object(data, table + i * OBJECT, i) {
            Ok(object) => objects.push(object),
            Err((name, reason)) => skipped.push(format!("{name}: {reason}")),
        }
    }
    if objects.is_empty() {
        return Err("no readable objects".into());
    }
    Ok((objects, skipped))
}

/// The object whose record starts at `at` (number `index` in the table), or its name and why it cannot be read.
fn read_object(data: &[u8], at: usize, index: usize) -> Result<Object, (String, String)> {
    let mut name = format!("Object {}", index + 1);
    if word(data, at) != Some(OBJECT_MAGIC) {
        return Err((name, "its record is not valid".into()));
    }
    let name_length = word(data, at + 176).unwrap_or(0) as usize;
    let text = long(data, at + 184).and_then(|offset| data.get(offset..offset.checked_add(name_length)?));
    if let Some(text) = text.map(|t| String::from_utf8_lossy(t).trim_matches(char::from(0)).trim().to_string()) {
        if !text.is_empty() {
            name = text;
        }
    }
    let fail = |reason: &str| (name.clone(), reason.to_string());
    let (x, y, lift) = (float(data, at + 8), float(data, at + 12), float(data, at + 20));
    let (Some(x), Some(y), Some(lift)) = (x, y, lift) else { return Err(fail("its record is cut short")) };
    if ![x, y, lift].iter().all(|v| v.is_finite()) {
        return Err(fail("its position is not a number"));
    }
    let model = long(data, at + 192)
        .and_then(|mesh| mesh_ranges(data, mesh))
        .ok_or_else(|| fail("its mesh lies outside the file"))?;
    let records = long(data, at + 200).zip(long(data, at + 208));
    let parts = records
        .map_or(Some(Vec::new()), |(first, count)| parts(data, first, count))
        .ok_or_else(|| fail("its supports are not valid"))?;
    Ok(Object { name, x, y, lift, model, parts })
}

/// The vertex and index byte ranges of the mesh block at `at`.
fn mesh_ranges(data: &[u8], at: usize) -> Option<Model> {
    if word(data, at)? != MESH_MAGIC {
        return None;
    }
    let (vertices, vertex_count) = (long(data, at + 8)?, long(data, at + 16)?);
    let (indices, index_count) = (long(data, at + 24)?, long(data, at + 32)?);
    let vertices = (vertices, vertex_count.checked_mul(12)?);
    let indices = (indices, index_count.checked_mul(4)?);
    let fits = |(offset, size): (usize, usize)| offset.checked_add(size).is_some_and(|end| end <= data.len());
    (vertex_count > 0 && index_count > 0 && index_count % 3 == 0 && fits(vertices) && fits(indices))
        .then_some(Model::Indexed { vertices, indices })
}

/// The parts of the `count` support records starting at `first`.
fn parts(data: &[u8], first: usize, count: usize) -> Option<Vec<Part>> {
    let mut parts = Vec::new();
    for record in 0..count {
        let at = first.checked_add(record.checked_mul(SUPPORT)?)?;
        if word(data, at)? != SUPPORT_MAGIC {
            return None;
        }
        let (n, list) = (long(data, at + 24)?, long(data, at + 32)?);
        for k in 0..n {
            let p = list.checked_add(k.checked_mul(PART)?)?;
            parts.push(Part {
                kind: word(data, p)?,
                a: point(data, p + 8)?,
                b: point(data, p + 20)?,
                size_a: float(data, p + 32)?,
                size_b: float(data, p + 36)?,
                depth_a: float(data, p + 40)?,
                mesh: (0, 0),
            });
        }
    }
    Some(parts)
}

/// The preview image as `(width, height, RGB pixels row by row)`, when the file has one: its header (width, height
/// as u32, the offset and size of the image as u64) is at the offset in the u64 at 48.
pub fn preview(data: &[u8]) -> Option<(u32, u32, Vec<u8>)> {
    let header = long(data, 48)?;
    let (width, height) = (word(data, header)?, word(data, header + 4)?);
    let (offset, size) = (long(data, header + 8)?, long(data, header + 16)?);
    decode_preview(data.get(offset..offset.checked_add(size)?)?, width, height)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A project of one object (a triangle) with one support record of two parts, and a 2 × 1 preview.
    fn project() -> Vec<u8> {
        let mut data = vec![0u8; 0x60];
        let put32 = |data: &mut Vec<u8>, at: usize, v: u32| data[at..at + 4].copy_from_slice(&v.to_le_bytes());
        let put64 = |data: &mut Vec<u8>, at: usize, v: u64| data[at..at + 8].copy_from_slice(&v.to_le_bytes());
        let putf = |data: &mut Vec<u8>, at: usize, v: f32| data[at..at + 4].copy_from_slice(&v.to_le_bytes());
        put32(&mut data, 0, MAGIC);
        put32(&mut data, 4, 1);
        // preview: header at 0x40, two white pixels as one run
        put64(&mut data, 48, 0x40);
        put32(&mut data, 0x40, 2);
        put32(&mut data, 0x44, 1);
        let image = data.len();
        data.extend([0xff, 0xff, 0x01, 0x00]); // colour 0xFFFF (run flag set), run of 1 + 1
        put64(&mut data, 0x48, image as u64);
        put64(&mut data, 0x50, 4);
        let table = data.len();
        put64(&mut data, 24, table as u64);
        data.resize(table + OBJECT, 0);
        let name = data.len();
        data.extend(b"Thing.stl");
        let mesh = data.len();
        data.resize(mesh + 40, 0);
        let vertices = data.len();
        for v in [[-1.0f32, -1.0, -2.0], [1.0, -1.0, -2.0], [0.0, 1.0, 2.0]] {
            v.iter().for_each(|c| data.extend(c.to_le_bytes()));
        }
        let indices = data.len();
        [0u32, 1, 2].iter().for_each(|i| data.extend(i.to_le_bytes()));
        put32(&mut data, mesh, MESH_MAGIC);
        put64(&mut data, mesh + 8, vertices as u64);
        put64(&mut data, mesh + 16, 3);
        put64(&mut data, mesh + 24, indices as u64);
        put64(&mut data, mesh + 32, 3);
        let record = data.len();
        data.resize(record + SUPPORT, 0);
        let list = data.len();
        for (kind, a, b) in [(1u32, [0.0f32, 0.0, -1.0], [0.0, 0.0, -2.0]), (3, [0.0, 0.0, -2.0], [0.0, 0.0, -7.0])] {
            let p = data.len();
            data.resize(p + PART, 0);
            put32(&mut data, p, kind);
            for k in 0..3 {
                putf(&mut data, p + 8 + 4 * k, a[k]);
                putf(&mut data, p + 20 + 4 * k, b[k]);
            }
            putf(&mut data, p + 32, 0.2);
            putf(&mut data, p + 36, 0.4);
        }
        put32(&mut data, record, SUPPORT_MAGIC);
        put64(&mut data, record + 24, 2);
        put64(&mut data, record + 32, list as u64);
        put32(&mut data, table, OBJECT_MAGIC);
        putf(&mut data, table + 8, 10.0);
        putf(&mut data, table + 12, -5.0);
        putf(&mut data, table + 16, 2.0);
        putf(&mut data, table + 20, 5.0);
        put32(&mut data, table + 176, 9);
        put64(&mut data, table + 184, name as u64);
        put64(&mut data, table + 192, mesh as u64);
        put64(&mut data, table + 200, record as u64);
        put64(&mut data, table + 208, 1);
        data
    }

    #[test]
    fn objects_parts_and_preview_are_read() {
        let data = project();
        assert!(is_ctp(&data));
        let (objects, skipped) = objects(&data).unwrap();
        assert!(skipped.is_empty());
        let object = &objects[0];
        assert_eq!((object.name.as_str(), object.x, object.y, object.lift), ("Thing.stl", 10.0, -5.0, 5.0));
        let mesh = object.model.mesh(&data);
        assert_eq!((mesh.vertices.len(), mesh.faces.len()), (3, 1));
        assert_eq!(mesh.bounds().unwrap().0[2], -2.0);
        assert_eq!(object.parts.iter().map(|p| p.kind).collect::<Vec<_>>(), [1, 3]);
        assert_eq!(object.parts[1].b, DVec3::new(0.0, 0.0, -7.0));
        assert_eq!(preview(&data), Some((2, 1, vec![255; 6])));
    }

    #[test]
    fn other_files_are_refused() {
        assert!(objects(b"not a project").is_err());
        let mut broken = project();
        let table = long(&broken, 24).unwrap();
        broken[table + 192..table + 200].copy_from_slice(&u64::MAX.to_le_bytes());
        assert!(objects(&broken).is_err());
    }
}
