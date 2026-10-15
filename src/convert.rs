//! Turning a CHITUBOX project (`.chitubox` or `.ctp`) into a Lychee scene state: one Lychee object per model, one editable Lychee support
//! per CHITUBOX top tip (planned in `trees.rs`) and one Lychee brace per CHITUBOX brace.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use glam::DVec3;
use serde_json::{Value, json};

use crate::chitubox::{self, Part};
use crate::ctp;
use crate::ground::Ground;
use crate::lys::{Member, mesh_bin, scene_bin};
use crate::trees::{self, Base, Planned};

/// The scene state of a Lychee 8 save with no objects or supports: printer, resins and settings.
const SCENE: &str = include_str!("../template/scene.json");
/// The record of an imported model, the pattern for every converted model.
const OBJECT: &str = include_str!("../template/object.json");
/// A support record, the pattern for every converted support.
const SUPPORT: &str = include_str!("../template/support.json");
/// Part types: a tip touching the model from below, and a foot pad (`CHITUBOX_FILE_FORMAT.md`, 6.5).
const TOP_TIP: u32 = 1;
const FOOT_PAD: u32 = 5;
/// Longest side of the preview image stored in the `.lys`.
const PREVIEW_SIZE: u32 = 800;

/// How to convert.
#[derive(Clone)]
pub struct Options {
    /// A `.lys` file whose printer, resins and settings the scene gets; `None` uses the built-in template.
    pub template: Option<PathBuf>,
    /// Whether CHITUBOX's braces become Lychee braces.
    pub braces: bool,
}

impl Default for Options {
    fn default() -> Self {
        Self { template: None, braces: true }
    }
}

/// What a conversion did, for the summary.
#[derive(Default)]
pub struct Report {
    pub objects: usize,
    pub triangles: usize,
    /// Supports standing on the plate, on the model, and branching off another support.
    pub on_plate: usize,
    pub on_model: usize,
    pub branches: usize,
    /// Braces between supports.
    pub braces: usize,
    /// Points along CHITUBOX's tips and columns (every 0.25 mm) that the Lychee supports draw, of all of them.
    pub followed: (usize, usize),
    /// Objects that could not be read, and objects partly outside the template's build plate.
    pub skipped: Vec<String>,
    pub outside: Vec<String>,
}

impl Report {
    /// How much of CHITUBOX's support paths the Lychee supports draw exactly, in percent (`None` without supports).
    pub fn drawn_exactly(&self) -> Option<f64> {
        let (followed, of) = self.followed;
        (of > 0).then(|| followed as f64 * 100.0 / of as f64)
    }
}

/// Convert the CHITUBOX project `input` into the Lychee scene `output`. The printer, resins and settings come from
/// the options' template (a `.lys` file) when given, otherwise from the built-in template. CHITUBOX's braces become
/// Lychee braces when `options.braces` is set.
pub fn convert(input: &Path, output: &Path, options: &Options) -> Result<Report, String> {
    let data = std::fs::read(input).map_err(|e| format!("cannot read {}: {e}", input.display()))?;
    let is_ctp = ctp::is_ctp(&data);
    let (objects, skipped) = if is_ctp { ctp::objects(&data)? } else { chitubox::objects(&data)? };
    let mut state = match &options.template {
        Some(path) => crate::lys::read_scene(path).map_err(|e| format!("template {}: {e}", path.display()))?,
        None => serde_json::from_str(SCENE).expect("built-in template"),
    };
    clear(&mut state);
    let object_pattern: Value = serde_json::from_str(OBJECT).expect("built-in template");
    let support_pattern: Value = serde_json::from_str(SUPPORT).expect("built-in template");
    let plate = plate_size(&state);
    let now = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64);

    let mut report = Report { skipped, ..Report::default() };
    let mut members = Vec::new();
    let (mut object_ids, mut support_ids) = (Vec::new(), Vec::new());
    for (index, object) in objects.iter().enumerate() {
        let id = format!("o{}", index + 1);
        let mesh = object.model.mesh(&data);
        let Some((lo, hi)) = mesh.bounds() else { continue };
        let size = DVec3::from_array(std::array::from_fn(|i| (hi[i] - lo[i]) as f64));
        let model_bytes = object.model.bytes(&data);
        let hash = hash_hex(model_bytes);

        // supports: CHITUBOX's trees rebuilt as trunks, branches and braces
        let parts = &object.parts[..];
        let ground = parts.iter().any(|p| p.kind == TOP_TIP).then(|| Ground::new(&mesh));
        let frame = Frame { plate: lo[2] as f64 - object.lift, pad: Pad::of(&data, parts) };
        let (planned, mut braces) = trees::plan(parts, frame.plate, ground.as_ref());
        if !options.braces {
            braces.clear();
        }
        let compared = options.braces.then_some(&braces[..]);
        let (close, total, _) = trees::fidelity(parts, &planned, compared, frame.plate);
        report.followed.0 += close;
        report.followed.1 += total;
        let first = support_ids.len();
        let sid = |index: usize| format!("s{}", first + index + 1);
        let mut supports = Vec::new();
        for (index, plan) in planned.iter().enumerate() {
            match plan.base {
                Base::Plate { .. } => report.on_plate += 1,
                Base::Model { .. } => report.on_model += 1,
                Base::Branch { .. } => report.branches += 1,
            }
            let record = support(&support_pattern, &sid(index), &id, plan, &frame, &sid, now);
            supports.push((sid(index), record));
        }
        for brace in &braces {
            let index = supports.len();
            supports.push((sid(index), brace_record(&support_pattern, &sid(index), &id, brace, &sid, now)));
            report.braces += 1;
        }
        let ids: Vec<Value> = supports.iter().map(|(sid, _)| json!(sid)).collect();

        let mut record = object_pattern.clone();
        let name = object.name.clone();
        let half = size / 2.0;
        if plate.is_some_and(|p| object.x.abs() + half.x > p.0 / 2.0 || object.y.abs() + half.y > p.1 / 2.0) {
            report.outside.push(name.clone());
        }
        let center = json!({ "x": 0.0, "y": 0.0, "z": half.z, "updatedAt": now });
        merge(
            &mut record,
            json!({
                "id": id,
                "name": name,
                "position": { "x": object.x, "y": object.y, "z": object.lift },
                "rotation": { "x": 0, "y": 0, "z": 0 },
                "scale": { "x": 1, "y": 1, "z": 1 },
                "dimension": { "x": size.x, "y": size.y, "z": size.z },
                "center": center,
                "formerCenter": center,
                "supportsTip": ids,
                "supportsBase": ids,
                "hasMinima": false,
                "hasHollowing2D": false,
                "updatedHollowVoxelsAt": 0,
                "hollowing": { "enabled": false, "updatedAt": now },
                "foundation": { "enabled": false },
                "stats": { "polyCount": mesh.faces.len(), "vertexCount": mesh.vertices.len() },
                "properties": {
                    "hash": hash,
                    "lyId": id,
                    "size": model_bytes.len() as f64 / 1e6,
                    "file": input.display().to_string(),
                    "absolute": input.display().to_string(),
                    "relative": "",
                    "lyPath": null,
                    "isMngo": true,
                    "updatedAt": now,
                },
            }),
        );
        state["objects"]["present"]["byId"][&id] = record;
        object_ids.push(json!(id));
        for (sid, support) in supports {
            state["supports"]["present"]["byId"][&sid] = support;
            support_ids.push(json!(sid));
        }
        report.objects += 1;
        report.triangles += mesh.faces.len();
        members.push(Member { name: format!("{hash}.bin"), data: mesh_bin(&mesh) });
    }
    state["objects"]["present"]["allIds"] = json!(object_ids);
    state["supports"]["present"]["allIds"] = json!(support_ids.clone());
    state["supports"]["present"]["changedIds"] = json!(support_ids);

    let preview = if is_ctp { ctp::preview(&data) } else { chitubox::preview(&data) };
    if let Some(png) = preview.and_then(preview_png) {
        members.push(Member { name: "preview.png".into(), data: png });
    }
    members.push(Member { name: "scene.bin".into(), data: scene_bin(&state) });
    crate::lys::write(output, &members).map_err(|e| format!("cannot write {}: {e}", output.display()))?;
    Ok(report)
}

/// What the supports of an object share: the plate's height in the model frame and the foot pad.
struct Frame {
    plate: f64,
    pad: Pad,
}

/// A foot pad: a square, `side` wide on the plate, `thickness` thick, its top wider by `angle` degrees.
struct Pad {
    side: f64,
    thickness: f64,
    angle: f64,
}

impl Pad {
    /// Measure CHITUBOX's foot pads (part type 5) from their triangles; Lychee-like defaults without one (or
    /// without its triangles, as in `.ctp` projects).
    fn of(data: &[u8], parts: &[Part]) -> Self {
        let measured = parts.iter().find(|p| p.kind == FOOT_PAD).and_then(|pad| {
            let mesh = chitubox::mesh(data, pad.mesh);
            let (lo, hi) = mesh.bounds()?;
            let width_at = |z: f32| {
                let xs = mesh.vertices.iter().filter(|v| (v[2] - z).abs() < 1e-3).map(|v| v[0]);
                let (min, max) = xs.fold((f32::MAX, f32::MIN), |(a, b), x| (a.min(x), b.max(x)));
                (max - min) as f64
            };
            let (bottom, top, thickness) = (width_at(lo[2]), width_at(hi[2]), (hi[2] - lo[2]) as f64);
            let angle = ((top - bottom) / 2.0 / thickness.max(1e-3)).atan().to_degrees();
            (bottom > 0.0).then_some(Self { side: bottom, thickness, angle })
        });
        measured.unwrap_or(Self { side: 5.0, thickness: 0.8, angle: 45.0 })
    }
}

/// A Lychee support for a CHITUBOX tip: the tip keeps its contact point and sizes, and runs to the planned tip join;
/// the base goes where the plan puts it (`sid` names the support at a plan index).
fn support(
    pattern: &Value,
    id: &str,
    object: &str,
    plan: &Planned,
    frame: &Frame,
    sid: &dyn Fn(usize) -> String,
    now: u64,
) -> Value {
    let tip = plan.tip;
    let direction = (plan.tip_join - tip.a).normalize_or(DVec3::NEG_Z);
    let column = plan.column;
    let xyz = |v: DVec3| json!({ "x": v.x, "y": v.y, "z": v.z });
    // base point and normal, whether it is a branch-type base, the support it rests on, the base piece's length,
    // depth into the model and diameter at its start, and the height of a plate support's column
    let (base, normal, parent, base_length, depth, point, join_length) = match plan.base {
        Base::Plate { foot, join } => {
            let height = (join - frame.plate).max(column / 2.0);
            (DVec3::new(foot.x, foot.y, 0.0), DVec3::Z, None, 2.0, 0.0, column, Some(height))
        }
        Base::Model { contact, depth, point, join } => {
            let rise = join - contact;
            (contact, rise.normalize_or(-direction), None, rise.length().max(0.01), depth, point, None)
        }
        Base::Branch { parent, point, join } => {
            let rise = join - point;
            (point, rise.normalize_or(-direction), Some(sid(parent)), rise.length().max(0.01), 0.0, column, None)
        }
    };
    let mut record = pattern.clone();
    merge(
        &mut record,
        json!({
            "id": id,
            "objectIdTip": object,
            "objectIdBase": object,
            "tip": xyz(tip.a),
            "tipNormal": xyz(direction),
            "base": xyz(base),
            "baseNormal": xyz(normal),
            "isBaseTip": join_length.is_none(),
            "parentBaseId": parent,
            "parentTipId": null,
            "parentId": [],
            "mini": false,
            "tags": [],
            "updatedAt": now,
            "isCollidingWithObject": false,
            "settings": {
                "mid": { "type": "cylinder" },
                "tip": {
                    "type": "cone",
                    "pointDiameter": tip.size_a * 2.0,
                    "diameter": tip.size_b * 2.0,
                    "length": tip.a.distance(plan.tip_join),
                    "penetration": tip.depth_a,
                    "angle": 100,
                    "breakPoint": 0,
                },
                "baseTip": {
                    "type": "cone",
                    "pointDiameter": point,
                    "diameter": column,
                    "length": base_length,
                    "penetration": depth,
                    "angle": 100,
                },
                "base": {
                    "type": "cube",
                    "diameter": frame.pad.side,
                    "length": frame.pad.thickness,
                    "angle": frame.pad.angle,
                    "joinDiameter": column,
                    "joinLength": join_length.unwrap_or(0.5),
                },
            },
        }),
    );
    record
}

/// A Lychee brace: a support whose base rests on one column and whose tip rests on another.
fn brace_record(
    pattern: &Value,
    id: &str,
    object: &str,
    brace: &trees::Brace,
    sid: &dyn Fn(usize) -> String,
    now: u64,
) -> Value {
    let ((from, base), (to, tip)) = (brace.lower, brace.upper);
    let along = (tip - base).normalize_or(DVec3::Z);
    let xyz = |v: DVec3| json!({ "x": v.x, "y": v.y, "z": v.z });
    let d = brace.diameter;
    let mut record = pattern.clone();
    merge(
        &mut record,
        json!({
            "id": id,
            "type": 0,
            "objectIdTip": object,
            "objectIdBase": object,
            "base": xyz(base),
            "baseNormal": xyz(along),
            "tip": xyz(tip),
            "tipNormal": xyz(-along),
            "isBaseTip": true,
            "parentBaseId": sid(from),
            "parentTipId": sid(to),
            "parentId": [],
            "mini": false,
            "tags": [],
            "updatedAt": now,
            "isCollidingWithObject": false,
            "settings": {
                "mid": { "type": "cylinder" },
                "tip": { "type": "cone", "pointDiameter": d, "diameter": d, "length": 2.0,
                         "penetration": 0, "angle": 100, "breakPoint": 0 },
                "baseTip": { "type": "cone", "pointDiameter": d, "diameter": d, "length": 2.0,
                             "penetration": 0, "angle": 100, "isStraight": true },
            },
        }),
    );
    record
}

/// Remove the template's own objects, supports and detected overhangs.
fn clear(state: &mut Value) {
    state["objects"] = json!({ "present": { "byId": {}, "allIds": [] } });
    let tags = state["supports"]["present"].get("tags").cloned().unwrap_or(json!(["allIds", "byId"]));
    state["supports"] = json!({ "present": { "byId": {}, "allIds": [], "changedIds": [], "tags": tags } });
    state["minimas"] = json!({ "present": { "byId": {}, "allIds": [] } });
}

/// Width and depth of the template's first build plate.
fn plate_size(state: &Value) -> Option<(f64, f64)> {
    let plates = &state["plates"]["present"];
    let first = plates["allIds"].get(0)?.as_str()?;
    let size = &plates["byId"][first]["size"];
    Some((size["x"].as_f64()?, size["y"].as_f64()?))
}

/// Copy the fields of `patch` into `target`, recursing into objects (other values are replaced).
fn merge(target: &mut Value, patch: Value) {
    match (target, patch) {
        (Value::Object(t), Value::Object(p)) => {
            for (key, value) in p {
                match t.get_mut(&key) {
                    Some(existing) if existing.is_object() && value.is_object() => merge(existing, value),
                    _ => {
                        t.insert(key, value);
                    }
                }
            }
        }
        (t, p) => *t = p,
    }
}

/// A 32-digit hexadecimal id for a mesh (two FNV-1a hashes of its bytes).
fn hash_hex(bytes: &[u8]) -> String {
    let fnv = |seed: u64| bytes.iter().fold(seed, |h, &b| (h ^ b as u64).wrapping_mul(0x0100_0000_01b3));
    format!("{:016x}{:016x}", fnv(0xcbf2_9ce4_8422_2325), fnv(0x84222325_cbf29ce4))
}

/// The project's preview (width, height, RGB pixels) as a PNG, scaled down to at most `PREVIEW_SIZE` pixels.
fn preview_png((width, height, rgb): (u32, u32, Vec<u8>)) -> Option<Vec<u8>> {
    let image = image::RgbImage::from_raw(width, height, rgb)?;
    let scale = (PREVIEW_SIZE as f64 / width.max(height) as f64).min(1.0);
    let (w, h) = (((width as f64 * scale) as u32).max(1), ((height as f64 * scale) as u32).max(1));
    let small = image::imageops::resize(&image, w, h, image::imageops::FilterType::Triangle);
    let mut png = Vec::new();
    small.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).ok()?;
    Some(png)
}
