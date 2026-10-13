# CHITUBOX project file format (`.chitubox`)

Specification of the scene files that CHITUBOX saves (*File → Save project*): the models of a build plate with
their supports. CHITUBOX does not publish this format; everything here was worked out from sample files and checked
against them byte for byte (see [Verification](#11-verification)). MeshPeek's reader is
`crates/meshpeek-core/src/io/chitubox.rs`; chitu2lys carries a copy of it in `src/chitubox.rs`.

Two layouts exist. **Current** files come from recent CHITUBOX versions; **older** files from earlier ones. They
share the same building blocks and differ in which optional sections are present and in the order of the
per-object data (section 9).

Fields whose purpose is not known are marked *unknown*; their observed values are given so that a writer can
reproduce them.

---

## 1. Conventions

- All integers and floats are **little-endian**. `u32` is unsigned 32-bit, `u64` unsigned 64-bit, `f32` IEEE-754
  single precision.
- **Offsets** are absolute byte positions from the start of the file, stored as `u32` (files are therefore below
  4 GiB). The one exception is the support-settings table (section 8), which stores `u64` offsets.
- **Nothing is aligned.** Sections are packed back to back, so meshes and records often start at odd offsets. Read
  values byte-wise.
- Lengths are in **millimetres**. Coordinates are right-handed with **Z up**; the origin of the plate is its centre
  (X, Y) at the plate surface (Z = 0).
- *Reserved* fields are zero in current files. Older files sometimes leave uninitialised memory in them (section 9);
  readers must ignore their contents.

## 2. File overview

Current layout, in file order:

| # | Section | Size |
|---|---|---|
| 1 | Header | 412 bytes (`0x19C`) |
| 2 | Reserved | 8 bytes |
| 3 | Preview header | 32 bytes |
| 4 | Preview image | variable |
| 5 | Object table | 680 bytes per object |
| 6 | Support-settings table | 24 bytes per object |
| 7 | Support-settings records | variable, one per object |
| 8 | Per object, in object order: model mesh, support mesh, support block, part list, 20-byte trailer | variable |

Every section is located through offsets, so a reader should follow the offsets rather than rely on this order.

## 3. Header

| Offset | Type | Name | Meaning |
|---|---|---|---|
| `0x00` | u32 | magic | Always `0xAB231243` (bytes `43 12 23 AB`) |
| `0x04` | u32 | objectCount | Number of objects (entries of the object table) |
| `0x08` | u32 | objectTableOffset | Offset of the object table (section 5) |
| `0x0C` | u32 | previewOffset | Offset of the preview header (section 4); 0 when the file has no preview |
| `0x10` | u32 | reservedOffset | Current files: `0x19C`, pointing at 8 reserved zero bytes right after the header. Older files: 0 |
| `0x14` | u32 | settingsTableOffset | Current files: offset of the support-settings table (section 7). Older files: 0 |
| `0x18` | — | reserved | Zero up to the end of the header at `0x19C` |

Files seen so far:

| Layout | previewOffset | reservedOffset | settingsTableOffset | objectTableOffset |
|---|---|---|---|---|
| Current | `0x1A4` | `0x19C` | after the object table | after the preview |
| Older with preview | `0x19C` | 0 | 0 | after the preview |
| Oldest, no preview | 0 | 0 | 0 | `0x19C` |

## 4. Preview image

### 4.1 Preview header (32 bytes)

| Offset | Type | Meaning |
|---|---|---|
| 0 | u32 | Width in pixels |
| 4 | u32 | Height in pixels |
| 8 | u32 | Offset of the encoded image data |
| 12 | u32 | Size of the encoded image data in bytes |
| 16 | 4 x u32 | Reserved (zero) |

The image data follows the header directly (data offset = header offset + 32) and the object table follows the
image data. Sizes seen: 2790 x 2215, 2102 x 2308 and 3050 x 2188 in current files, 400 x 300 in an older one.

### 4.2 Encoding

The same run-length encoding as the previews of CHITUBOX print files (`.ctb`). The data is a sequence of `u16`
words, row by row from the top-left pixel:

- A **colour word** holds an RGB colour: red = bits 11–15, green = bits 6–10, blue = bits 0–4 (5 bits each).
- If bit 5 (`0x0020`) of the colour word is set, the **next word** is a repeat count: `0x3000 | (count − 1)`, so
  `count = (word & 0x0FFF) + 1` (1 to 4096 pixels of that colour). Otherwise the colour covers one pixel.

Example: `61 08 FE 3F` is colour `0x0861` with the repeat bit set, followed by `0x3FFE`: 4095 pixels of that colour.

## 5. Object table

`objectCount` slots of **680 bytes**, one per object, starting at `objectTableOffset`.

| Offset | Type | Name | Meaning |
|---|---|---|---|
| 0 | 256 bytes | name | Object name, UTF-8, zero-terminated and zero-padded. CHITUBOX uses the source file name, with suffixes such as "(repaired)" or "(hollowed out)" |
| 256 | f32 | x | X of the object's centre on the plate |
| 260 | f32 | y | Y of the object's centre on the plate |
| 264 | f32 | lift | Height of the model's lowest point above the plate (the support height), e.g. `5.0`; `0.0` for a model standing on the plate |
| 268 | u32 | supportBlockOffset | Offset of the object's support block (section 6.1); `0xFFFFFFFF` when the object has no supports |
| 272 | u32 | meshOffset | Offset of the model mesh |
| 276 | u32 | meshSize | Size of the model mesh in bytes (a multiple of 36) |
| 280 | u32 | colour | Older files: ARGB display colour (e.g. `0xFF4EAEEE`); 0 in current files |
| 284 | 396 bytes | reserved | Zero |

### 5.1 Placement

The model mesh (section 6.2) is stored **centred on the origin** in X, Y and Z, already rotated and scaled. To place
it on the plate, translate it by

```text
( x,  y,  lift − zmin )
```

where `zmin` is the lowest Z of the model mesh. The support mesh uses the same frame and the same translation; its
lowest point then lands on the plate (`zmin − lift` in mesh coordinates becomes Z = 0). The file stores no rotation
or scale: a model's orientation is baked into its triangles.

## 6. Meshes and supports

### 6.1 Support block (416 bytes)

| Offset | Type | Name | Meaning |
|---|---|---|---|
| 0 | u32 | partCount | Number of support parts (entries of the part list) |
| 4 | u32 | partListOffset | Offset of the part list (section 6.3) |
| 8 | u32 | supportMeshOffset | Current files: offset of the support mesh. Older files: offset of the 20-byte trailer (section 6.4) |
| 12 | u32 | supportMeshSize | Current files: size of the support mesh in bytes. Older files: 20 |
| 16 | 400 bytes | reserved | Zero in current files; uninitialised memory in older files |

### 6.2 Mesh encoding

A mesh is a run of triangles, **36 bytes per triangle**:

| Offset | Type | Meaning |
|---|---|---|
| 0 | 3 x f32 | First corner (x, y, z) |
| 12 | 3 x f32 | Second corner |
| 24 | 3 x f32 | Third corner |

There are no normals, no indices, no attributes and no triangle count: the count is the mesh size divided by 36.
Corners shared by several triangles are repeated. Hollowed models include their inner shell.

The **support mesh** is the concatenation of the meshes of all support parts, in part-list order. In current files
it follows the model mesh directly; in older files it lies before the model mesh (section 9). The safest way to find
it in any layout is through the part list: part `k + 1`'s triangles start where part `k`'s end, so the support mesh
runs from the first part's mesh offset to the end of the last part's mesh.

### 6.3 Part list

`partCount` entries of **72 bytes**, one per support part:

| Offset | Type | Name | Meaning |
|---|---|---|---|
| 0 | u32 | marker | Always `0xEA342389` |
| 4 | u32 | type | Part type (section 6.5) |
| 8 | 3 x f32 | a | End point A (model mesh coordinates) |
| 20 | 3 x f32 | b | End point B |
| 32 | f32 | sizeA | Size at A: a **radius** (section 6.5 for the foot pad) |
| 36 | f32 | sizeB | Size at B |
| 40 | u32 | meshOffset | Offset of this part's triangles |
| 44 | u32 | meshSize | Size of this part's triangles in bytes |
| 48 | f32 | depthA | Tips: how deep A lies inside the model (section 6.6); 0 for other types; 0 in older files |
| 52 | f32 | depthB | Type 12: `0.2`; 0 otherwise |
| 56 | 4 x u32 | reserved | Zero |

### 6.4 Trailer (20 bytes)

Current files: 20 zero bytes after the part list. Older files: 20 bytes right after the support block, starting with
four floats `1e8, −1e8, 1e8, −1e8` (an empty 2D bounding box) and an *unknown* `u32`.

### 6.5 Part types and their geometry

All round parts are built around the axis from A to B. Frustums and cylinders have **16 sides** in current files
(60 triangles: 32 on the side, a 14-triangle fan on each end) and **32 sides** in older files (124 triangles).
Spheres are UV spheres with 16 segments and 16 bands (480 triangles) whose poles are A and B.

| Type | Shape | Role | Typical values (current files) |
|---|---|---|---|
| 1 | Frustum, radius `sizeA` at A, `sizeB` at B | **Top tip**: A touches the model from below, B is lower | 2 mm long, radius 0.2 → 0.6; `depthA` 0.4 |
| 2 | Frustum | **Bottom tip**: a support that stands on the model; A touches the model, B is higher | radius 0.15–0.2 → up to 0.6; `depthA` 0.2 |
| 3 | Frustum or cylinder | **Column, branch or brace**. Braces come in crossing pairs (A and B swapped between the two parts) | columns radius 0.6, braces 0.4 |
| 4 | Frustum | **Foot cone**: the bottom of a column, widening towards the pad | 1 mm long, radius 1.0 → 1.1 |
| 5 | Square frustum, 12 triangles, sides parallel to X and Y | **Foot pad** on the plate (A at the top, B on the plate). Here `sizeA` belongs to the **plate end (B)** and `sizeB` to the **top (A)**: the pad is wider at the top. Current files store half the square's **diagonal**, older files half its **side** | 0.8 mm thick, 5.15 mm square at the plate, 7.07 mm at the top |
| 7 | Sphere of radius `sizeA` centred halfway between A and B | **Contact ball** on a tip's contact point (current files only) | radius 0.15–0.2 |
| 9 | Sphere of radius `sizeA` centred halfway between A and B | **Joint** where parts meet, 4 % wider than the column it sits on | radius 0.624 on 0.6 columns |
| 12 | Cylinder with a type-9 joint at each end | **Short link rod** (*role unknown*); `depthA` = `depthB` = 0.2 | radius 0.2, 0.01–3.5 mm long |

The sum of `meshSize` over a part list equals the support mesh size, and every part's triangles lie exactly on the
shape above.

### 6.6 Contact depth

For type 1 (and type 2) parts, `depthA` is the distance from the contact point A to the model's surface along the
part's axis: the tip is embedded that deep into the model. CHITUBOX uses the setting (0.4 mm in the samples) and
reduces it where the model is thinner (values such as 0.079 occur on thin walls).

## 7. Support-settings table (current files)

At `settingsTableOffset`: `objectCount` entries of **24 bytes**, in object order:

| Offset | Type | Meaning |
|---|---|---|
| 0 | u64 | Number of strings in the record header (always 2) |
| 8 | u64 | Offset of the object's record (section 8) |
| 16 | u64 | Offset of the end of the record header (= start of its class list) |

## 8. Support-settings records (current files)

One record per object, back to back after the table. A record has a header and a class list.

**Header:** two string descriptors followed by the string bytes:

| Offset | Type | Meaning |
|---|---|---|
| 0 | u64 | String number (1) |
| 8 | u64 | Offset of string 1 |
| 16 | u64 | Length of string 1 in bytes |
| 24 | u64 | String number (2) |
| 32 | u64 | Offset of string 2 |
| 40 | u64 | Length of string 2 |
| 48 | bytes | String 1 then string 2, UTF-8, not zero-terminated |

The strings are the names of the support presets used for the object, e.g. `Middle * 1` and `Light`, or
`Middle Light` and `Light Light`.

**Class list:** one **8-byte** entry per support part, in part-list order (`partCount` entries): two `u32` class
codes. Columns, tips, pads and joints use the first code (the second is 0); a brace connecting supports of two
classes carries both. The meaning of the codes is *unknown*; values seen:

| Code | Found on |
|---|---|
| `0x00000C00` | Most parts of every object |
| `0x00002609` | Thicker tips (contact radius 0.4) and their columns, joints and pads; braces to them |
| `0x00006105` | Type 12 link rods and their 0.2 mm joints |
| `0x00002205` | Type 12 link rods and their 0.2 mm joints |

The meshes of the first object follow the last record.

## 9. Older layout

Files from earlier CHITUBOX versions differ as follows:

- **Header:** no settings table (field `0x14` = 0) and no reserved block (field `0x10` = 0). The preview, if any,
  starts at `0x19C`; without a preview the object table does.
- **No support-settings table or records**, so no class codes or preset names.
- **Per-object data order:** support block, 20-byte trailer, part list, support mesh (the parts' triangles), then
  the model mesh. A model without supports (`supportBlockOffset` = `0xFFFFFFFF`) is just followed by its mesh.
- **Support block:** fields 8 and 12 point at the 20-byte trailer instead of the support mesh; the reserved area
  holds leftover memory. Use the part list to find the support mesh (section 6.2).
- **Parts:** no type 7 contact balls, `depthA`/`depthB` are 0, frustums have 32 sides, pad sizes are half sides.
- **Object slot:** may carry a display colour at offset 280.

## 10. Reading a file

1. Check the magic number; read `objectCount` and `objectTableOffset`.
2. For each slot: read the name, position, lift, model mesh range and support block offset.
3. Read the model's triangles and translate them by `(x, y, lift − zmin)`.
4. If the support block offset is not `0xFFFFFFFF`, read the part list, check each entry's marker, and take the
   support mesh from the first part's mesh offset to the end of the last part's mesh (parts are contiguous).
   Translate it like the model.

Validate every offset and size against the file length: all ranges must lie inside the file and mesh sizes must be
multiples of 36.

## 11. Verification

Checked on five files: three current (7, 9 and 9 objects; 230–330 MB; including hollowed models) and two older
ones (one object each, one with supports and one without):

- Every byte of the current files is accounted for: header, preview, table, records and per-object sections abut
  without gaps, and each file ends with the last object's trailer.
- In all 25 supported objects the parts' triangle ranges tile the support mesh exactly, and every part's triangles
  lie on the shape its type describes.
- Placing the model and the supports as in section 5.1 puts every support's foot on the plate; tips touch the
  model's surface, with the contact depth of section 6.6 measured by ray casting.
- The preview header sizes match the image data exactly, and decoding as in section 4.2 yields exactly width x
  height pixels showing the scene; an older file's preview shows its model in the slot's colour (offset 280).
