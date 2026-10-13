//! Lychee `.lys` files (`docs/LYS_FILE_FORMAT.md`): writing the MNGO container, the ciphered MessagePack scene
//! state and mesh members, and reading the scene state of a template.

use std::io::Write;
use std::path::Path;

use crate::mesh::Mesh;

/// Version written in the container index (what Lychee 7 and 8 write).
const CONTAINER_VERSION: &str = "3.1.0";
/// Lychee's application id, the repeating key of the cipher on `scene.bin`.
const FILE_APP_ID: &[u8] = b"a8ee1146-8d03-4b69-8a67-59009a3f9ee7";
/// The first bytes of a ZIP file: the first Lychee versions saved `.lys` files as ZIP archives.
const ZIP_MAGIC: &[u8] = b"PK\x03\x04";

/// A file inside the `.lys` container.
pub struct Member {
    pub name: String,
    pub data: Vec<u8>,
}

/// Write `members` as an MNGO container (Electron asar with `mangoFiles` for `files`).
pub fn write(path: &Path, members: &[Member]) -> std::io::Result<()> {
    let mut files = serde_json::Map::new();
    let mut offset = 0usize;
    for member in members {
        files.insert(
            member.name.clone(),
            serde_json::json!({ "size": member.data.len(), "offset": offset.to_string() }),
        );
        offset += member.data.len();
    }
    let index = serde_json::json!({ "version": CONTAINER_VERSION, "mangoFiles": files }).to_string();
    let padded = index.len().div_ceil(4) * 4;
    let mut out = std::io::BufWriter::new(std::fs::File::create(path)?);
    for word in [4, 8 + padded, 4 + padded, index.len()] {
        out.write_all(&(word as u32).to_le_bytes())?;
    }
    out.write_all(index.as_bytes())?;
    out.write_all(&vec![0u8; padded - index.len()])?;
    for member in members {
        out.write_all(&member.data)?;
    }
    out.flush()
}

/// The scene state as `scene.bin`: MessagePack, ciphered with Lychee's application id.
pub fn scene_bin(state: &serde_json::Value) -> Vec<u8> {
    let mut packed = Vec::new();
    rmpv::encode::write_value(&mut packed, &to_msgpack(state)).expect("writing to memory");
    packed.iter().zip(FILE_APP_ID.iter().cycle()).map(|(&b, &k)| b.wrapping_add(k % 255)).collect()
}

/// A mesh as a `<hash>.bin` member: five u32 (version 2, 12, index count, coordinate count, 0), the indices, the
/// coordinates.
pub fn mesh_bin(mesh: &Mesh) -> Vec<u8> {
    let (indices, coords) = (mesh.faces.len() * 3, mesh.vertices.len() * 3);
    let mut out = Vec::with_capacity(20 + 4 * (indices + coords));
    for word in [2, 12, indices as u32, coords as u32, 0] {
        out.extend_from_slice(&word.to_le_bytes());
    }
    mesh.faces.iter().flatten().for_each(|i| out.extend_from_slice(&i.to_le_bytes()));
    mesh.vertices.iter().flatten().for_each(|v| out.extend_from_slice(&v.to_le_bytes()));
    out
}

/// The scene state of the `.lys` file at `path` (its `scene.bin`), as JSON.
pub fn read_scene(path: &Path) -> Result<serde_json::Value, String> {
    let data = std::fs::read(path).map_err(|e| e.to_string())?;
    if data.starts_with(ZIP_MAGIC) {
        return Err("saved by an old Lychee (ZIP file); open it in a current Lychee and save it again".into());
    }
    let ciphered = member(&data, "scene.bin")?;
    let packed: Vec<u8> =
        ciphered.iter().zip(FILE_APP_ID.iter().cycle()).map(|(&b, &k)| b.wrapping_sub(k % 255)).collect();
    let state = rmpv::decode::read_value(&mut &packed[..])
        .map_err(|e| format!("its scene data is not valid MessagePack ({e})"))?;
    Ok(to_json(&state))
}

/// The bytes of the top-level member `name` of an MNGO container: `[4][header size][payload size][index
/// length][index JSON]`, the members' data from `8 + header size`, at the offsets the index gives (as strings).
fn member<'a>(data: &'a [u8], name: &str) -> Result<&'a [u8], String> {
    let word = |at: usize| {
        data.get(at..at + 4).map(|b| u32::from_le_bytes(b.try_into().unwrap()) as usize).ok_or("not a Lychee scene")
    };
    let (header, length) = (word(4)?, word(12)?);
    let index = data.get(16..16 + length).ok_or("its file index runs past the end of the file")?;
    let index: serde_json::Value = serde_json::from_slice(index).map_err(|e| format!("unreadable file index: {e}"))?;
    let entry = &index["mangoFiles"][name];
    let offset =
        entry["offset"].as_str().and_then(|s| s.parse::<usize>().ok()).or(entry["offset"].as_u64().map(|o| o as usize));
    let (Some(offset), Some(size)) = (offset, entry["size"].as_u64()) else {
        return Err(format!("it has no {name}"));
    };
    let start = 8 + header + offset;
    data.get(start..start + size as usize).ok_or_else(|| format!("its {name} runs past the end of the file"))
}

/// MessagePack to JSON (byte blobs become `"<N bytes>"`).
fn to_json(value: &rmpv::Value) -> serde_json::Value {
    use rmpv::Value as M;
    use serde_json::Value as J;
    match value {
        M::Nil => J::Null,
        M::Boolean(b) => J::Bool(*b),
        M::Integer(i) => i.as_i64().map(J::from).or_else(|| i.as_u64().map(J::from)).unwrap_or(J::Null),
        M::F32(f) => serde_json::Number::from_f64(*f as f64).map_or(J::Null, J::Number),
        M::F64(f) => serde_json::Number::from_f64(*f).map_or(J::Null, J::Number),
        M::String(s) => J::String(s.as_str().unwrap_or("<invalid utf-8>").to_string()),
        M::Binary(b) => J::String(format!("<{} bytes>", b.len())),
        M::Array(a) => J::Array(a.iter().map(to_json).collect()),
        M::Map(m) => J::Object(
            m.iter().map(|(k, v)| (k.as_str().map_or_else(|| k.to_string(), str::to_string), to_json(v))).collect(),
        ),
        M::Ext(t, b) => J::String(format!("<ext {t}: {} bytes>", b.len())),
    }
}

/// JSON to MessagePack: integers stay integers, other numbers become 64-bit floats.
fn to_msgpack(value: &serde_json::Value) -> rmpv::Value {
    use serde_json::Value as J;
    match value {
        J::Null => rmpv::Value::Nil,
        J::Bool(b) => rmpv::Value::Boolean(*b),
        J::Number(n) => match (n.as_i64(), n.as_u64()) {
            (Some(i), _) => rmpv::Value::from(i),
            (_, Some(u)) => rmpv::Value::from(u),
            _ => rmpv::Value::F64(n.as_f64().unwrap_or(0.0)),
        },
        J::String(s) => rmpv::Value::from(s.as_str()),
        J::Array(a) => rmpv::Value::Array(a.iter().map(to_msgpack).collect()),
        J::Object(m) => {
            rmpv::Value::Map(m.iter().map(|(k, v)| (rmpv::Value::from(k.as_str()), to_msgpack(v))).collect())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn written_files_read_back() {
        let dir = std::env::temp_dir().join("chitu2lys-lys-test");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("t.lys");
        let mesh = Mesh::new(vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]], vec![[0, 1, 2]]);
        let state = serde_json::json!({ "version": 4, "objects": { "n": 1.5, "name": "x" } });
        let members = [
            Member { name: "abc.bin".into(), data: mesh_bin(&mesh) },
            Member { name: "scene.bin".into(), data: scene_bin(&state) },
        ];
        write(&path, &members).unwrap();

        assert_eq!(read_scene(&path).unwrap(), state);
        let data = std::fs::read(&path).unwrap();
        let bin = member(&data, "abc.bin").unwrap();
        assert_eq!(bin, &members[0].data[..]);
        let words: Vec<u32> = bin[..20].chunks_exact(4).map(|w| u32::from_le_bytes(w.try_into().unwrap())).collect();
        assert_eq!(words, [2, 12, 3, 9, 0]);
    }
}
