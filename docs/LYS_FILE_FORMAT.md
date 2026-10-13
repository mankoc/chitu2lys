# Lychee Slicer `.lys` / `.lyl` File Format

A reverse-engineered specification sufficient to read (and write) Lychee scene
files in an external tool. Derived from the renderer bundle
(`bin/renderFile.js`). Related extensions share this format: `.lys` (scene),
`.lyl` (library object), and the "lite" variant.

---

## 1. High-level structure

A `.lys` file is an **archive** that bundles one scene-state blob plus binary
asset files (meshes, supports, preview image, etc.):

```
.lys archive
├── scene.bin                     ← scene/state (MessagePack + cipher, §3)
├── <meshHash>.bin                ← object mesh (native binary, not covered here)
├── <meshHash>_hollowing.bin      ← cached hollowed mesh (same native format)
├── <objectId>_islands.bin        ← island footprints for one object (§5.2)
├── <objectId>_hollowVoxels.bin   ← hollowing voxel grid for one object (§5.3)
├── preview.png
└── <image-note PNGs>
```

All members sit at the archive root (flat). See §5.1 for how the `<objectId>`
and `<meshHash>` parts are resolved.

There are **two archive container formats**:

| Version | Container | When produced |
|---------|-----------|---------------|
| **V1 (legacy)** | standard **ZIP** | only on explicit "Save As → legacy" |
| **V2+ (current)** | **MNGO** package (an `asar` fork) | normal saves |

Detection: if the file is a valid ZIP (PK magic `50 4B 03 04`), treat it as V1;
otherwise parse it as an MNGO package (§2).

---

## 2. MNGO container format (current files)

MNGO is byte-for-byte the **Electron `asar`** layout, with one rename: the
directory-index JSON key `files` is called **`mangoFiles`**. If you have an
asar reader, you can reuse it by treating `mangoFiles` as `files`.

### 2.1 Byte layout

All integers are **little-endian uint32**. The leading bytes are two
Chromium "Pickle" structures (a pickle = `[uint32 payloadSize][payload]`,
strings are length-prefixed and padded to a 4-byte boundary).

```
offset  size            field
------  --------------  ---------------------------------------------
0x00    4               0x00000004   (constant: pickle payload size of the size field)
0x04    4               headerSize   (byte length of the header pickle that follows)
0x08    4               headerPayloadSize  (= 4 + alignTo4(jsonLen))
0x0C    4               jsonLen      (exact UTF-8 byte length of the JSON header)
0x10    jsonLen         JSON header string (UTF-8)   ← the file index
...     (pad to 4)      zero padding to 4-byte alignment
0x08+headerSize  ...    DATA REGION: all file contents concatenated
```

> Practical recipe to get the header JSON: read `jsonLen` at offset `0x0C`,
> then read `jsonLen` bytes starting at `0x10`. (You can ignore the padding
> because `jsonLen` is exact.)

### 2.2 Header JSON (the file index)

A recursive directory tree. Example:

```json
{
  "version": "3.1.0",
  "mangoFiles": {
    "scene.bin":      { "size": 48213, "offset": "0" },
    "preview.png":    { "size": 10204, "offset": "48213" },
    "meshes": {
      "mangoFiles": {
        "obj_0.bin":  { "size": 99831, "offset": "58417" }
      }
    }
  }
}
```

Node shapes:
- **File:**      `{ "size": <int>, "offset": "<int as string>" }`
- **Directory:** `{ "mangoFiles": { <name>: <node>, ... } }`
- **Symlink:**   `{ "link": "<relative path>" }`
- **Unpacked:**  `{ "size": <int>, "unpacked": true }` (content stored
  externally in a sibling `<archive>.unpacked/` dir — not used for normal `.lys`)

### 2.3 Resolving file content

For a file node, the absolute byte position in the archive is:

```
absolutePosition = 8 + headerSize + parseInt(node.offset)
length           = node.size
```

where `headerSize` is the uint32 at offset `0x04`. `node.offset` is relative to
the **start of the data region** (`0x08 + headerSize`), and offsets accumulate
in index order.

---

## 3. `scene.bin` payload encoding

`scene.bin` holds the serialized application/scene state.

### 3.1 V2+ (current) — MessagePack + byte cipher

Two reversible steps. To **read**, undo them in this order:

**Step A — reverse the byte cipher.** The payload is obfuscated with the
application id string (the "fileAppId", a fixed app constant — see note below)
as a repeating key:

```python
# encode (app does):   b[i] = (b[i] + (key[i % len(key)] % 255)) & 0xFF
# decode (you do):
def decipher(buf: bytes, key: str) -> bytes:
    out = bytearray(buf)
    for i in range(len(out)):
        out[i] = (out[i] - (ord(key[i % len(key)]) % 255)) & 0xFF
    return bytes(out)
```

Notes on the exact arithmetic (matches the source):
- The key is indexed cyclically by **byte position** `i % len(key)`.
- `ord(...) % 255` is applied to the key char code (note: `255`, not `256`).
- The add/subtract wraps in the 0–255 byte range (`& 0xFF`).

**Step B — MessagePack-decode** the result. The codec is a standard
**MessagePack** implementation; any conformant msgpack library decodes it into
the state object (§4).

So: `state = msgpack_decode( decipher(scene_bin_bytes, fileAppId) )`.

### 3.2 V1 (legacy) — MessagePack only

No cipher. `scene.bin` is plain MessagePack: `state = msgpack_decode(scene_bin_bytes)`.

### 3.3 The `fileAppId` cipher key

The cipher key is a fixed application id (`Globals.env.fileAppId`):

```
fileAppId = "a8ee1146-8d03-4b69-8a67-59009a3f9ee7"
```

This 36-character string (including the hyphens) is the literal key — pass it
to `decipher()` exactly as written.

Build note: in the source, `fileAppId` is assigned the **production** app id
*before* the dev/viewer-specific `applicationId` overrides run, so the same
value is used for the main slicer, CLI, viewer, and dev builds — i.e. all
`.lys`/`.lyl` files share this one key. (For reference, the other ids that
appear nearby — `8d025437-…` dev, `1735f028-…` viewer — are **not** used for
file encoding.)

If decoding still yields garbage, the cause is almost always skipping Step A or
mis-indexing the key. As a sanity check, msgpack documents start with a
recognizable type byte (commonly `0x80–0x8f` fixmap or `0xde`/`0xdf` map), so
the first byte of `decipher(scene_bin, fileAppId)` should be a valid msgpack
map header.

---

## 4. Decoded scene-state object (shape)

After msgpack-decode you get the (refined) Redux state. Notable fields:

| Field | Meaning |
|-------|---------|
| `version` | format/state version: `3` for FDM scenes; `4` if incompatible params present; lower = legacy |
| `lysFeatureFlags` | present when `version === 4`; feature gates used by the scene |
| `kind.current` | printer kind (`FDM` / `MSLA`) |
| `objects.present.byId` / `.allIds` | scene objects; each has `type`, `properties` (incl. `absolute`/`relative` file paths), `note`, transforms |
| `settings` | app/export settings |
| `slicer`, `supports`, `holes` | slicing, support, and hole data |
| `minimas.present.byId` / `.allIds` | island metadata (`objectId`, `area`, `angle`, `zPosition`, `hasSupport`); geometry is in `<objectId>_islands.bin` (§5.2) |
| `menus.common.personalView` | UI view state |
| `isBackup`, `originalMokName` | backup metadata |
| `account` | cached account/license flags |

Object mesh geometry itself is **not** in `scene.bin`; it lives in the separate
binary asset files inside the archive (referenced by object id / name).

---

## 5. Per-object asset members

### 5.1 Naming

| Member | Key | Written when |
|--------|-----|--------------|
| `<meshHash>.bin` | `objects.present.byId[id].properties.hash` | one per unique mesh, for embedded objects only (see below) |
| `<meshHash>_hollowing.bin` | same hash as the object's mesh | object has a hollowing result |
| `<objectId>_islands.bin` | the object's key in `objects.present.allIds` | object has ≥ 1 island |
| `<objectId>_hollowVoxels.bin` | the object's key in `objects.present.allIds` | object uses voxel hollowing (`hollowing.useVoxels`) and has a non-empty grid |

`<objectId>` is the in-memory state id at save time (e.g. `o12`), which is the
same string used as the key in `scene.bin`. Always use that key to find the
members. `properties.lyId` is unreliable. When present it equals the key, but it
can be missing: a newly created parametric cube was saved without it and only
gained it after the file was reopened and saved again. A missing islands/voxels member is not an error; the app just
skips it (and recomputes voxels if needed).

**Library objects are not embedded.** Objects with `type: "libraryFile"` (for
example the bundled benchy) have a `properties.hash` but no `<meshHash>.bin` in
the archive. Their geometry is loaded from `properties.file`, a path to the
app's bundled model (e.g. `C:/Program Files/Lychee/bin/stl/benchy.stl`). To
read the scene in another tool, you need that file. Parametric objects
(`type: "cube"`, …) do embed their mesh.

### 5.2 Islands file — `<objectId>_islands.bin`

**Encoding:** identical to V2 `scene.bin`: the byte cipher with `fileAppId`
(§3.1 Step A) followed by MessagePack. The writer always uses this encoding, even
when the container is a legacy ZIP.

```
islands = msgpack_decode( decipher(raw, fileAppId) )
```

**Decoded shape:** a map from **island id** to a footprint record. The island
id matches a key in `scene.bin` → `minimas.present.byId` (island metadata such
as `area`, `angle`, `zPosition` and `hasSupport` lives there, not here).

Real record from a test file (one triangular island on the underside of a
10 mm cube; numbers shortened):

```jsonc
{
  "m1": {
    "resolutionXY": 1,                    // number: XY scale of the path coordinates
    "resolutionZ":  0.03,                 // number: Z scale (layer-height related)
    "normalInObjectSpace":         { "x": 0, "y": 0, "z": -1 },
    "centerPositionInObjectSpace": { "x": -0.8192, "y": -3.5156, "z": 10.7366 },
    "localPositionInObjectSpace":  { "x": 0.5,     "y": 0.5,     "z": 10.7366 },
    "polys": [
      {
        "shape": { "curve": "M -0.7322 -3.6031 L -0.8627 -3.3224 L -0.8627 -3.6215 L -0.7322 -3.6031 Z" },
        "holes": []                                              // zero or more { "curve": ... }
      }
    ],
    "needOffsetOnSave": false
  }
}
```

The matching `scene.bin` entry is
`minimas.present.byId.m1 = { id: "m1", objectId: "o1", area: 0.0195, angle: 69.42, zPosition: 10.7366, hasSupport: false, lyId: null }`.

Field notes:
- Vectors are plain `{x, y, z}` maps (serialized three.js `Vector3`), in the
  object's local (mesh) space.
- `polys[].shape.curve` / `holes[].curve` are SVG path strings using `M`, `L`,
  `C` and `Z`. Numbers may appear in plain decimal (exponent notation is
  expanded before writing).
- `needOffsetOnSave` is a backward-compatibility hack. If it is `true` at save
  time, the writer overwrites `localPositionInObjectSpace.x`/`.y` with `0.5`
  and stores the flag as `false`. In practice saved files contain
  `false` and `x = y = 0.5`.

**Rebuilding the footprint** (what the app does on load, per poly):
1. Parse `shape.curve` into a 2D shape and add each `holes[].curve` as a hole.
2. Translate by `(-0.5, -0.5)`.
3. Scale by `(resolutionXY, resolutionXY, resolutionZ)`.
4. Translate by `localPositionInObjectSpace`.

The result is a flat region in object space. (The app also extrudes it with a
small bevel, depth 0, for display only.)

**In practice the XY offsets cancel.** Current files store `resolutionXY = 1`
and `localPositionInObjectSpace.x = y = 0.5`. Step 2 subtracts 0.5 and step 4
adds it back, so **the path coordinates are already object-space millimetres**.
The island's Z is `localPositionInObjectSpace.z`. You only need the full
transform for older files written with the image-tracer path, where the
coordinates are in pixel units and `resolutionXY` is not 1.

### 5.3 Hollow-voxels file — `<objectId>_hollowVoxels.bin`

**Encoding:** raw little-endian binary. **No cipher and no MessagePack.**

```
offset  size  type     field
------  ----  -------  ----------------------------------------------
0x00    4     uint32   sizeX   (voxel count along X)
0x04    4     uint32   sizeY
0x08    4     uint32   sizeZ
0x0C    8     float64  wallThickness (mm; `hollowing.outer` when the grid was computed)
0x14    1     uint8    useVoxels (1 = voxel hollowing enabled, 0 = not)
0x15    ...   bytes    RLE-compressed voxel data
```

> The source has a comment that lists `wallThickness` as a 4-byte float. That
> comment is wrong: the code writes and reads an 8-byte double at `0x0C`, so the
> header is 21 bytes.

**RLE payload:** a sequence of `(value: uint8, count: uint8)` pairs, with
`count` from 1 to 255. Longer runs are split into multiple pairs.

```python
def decode_hollow_voxels(buf: bytes):
    import struct
    sx, sy, sz = struct.unpack_from("<III", buf, 0)
    wall,      = struct.unpack_from("<d", buf, 12)
    use_voxels = buf[20] == 1
    rle = buf[21:]
    vox = bytearray()
    for i in range(0, len(rle) - 1, 2):
        vox += bytes([rle[i]]) * rle[i + 1]
    assert len(vox) == sx * sy * sz
    return (sx, sy, sz), wall, use_voxels, vox
```

**Grid layout:** X varies fastest, then Y, then Z:

```
index(x, y, z) = x + y * sizeX + z * sizeX * sizeY
```

**Voxel value:** each byte is a bitfield.

| Bit | Mask | Meaning |
|-----|------|---------|
| 0 | `0x01` | inside the hollowing volume (voxels without this bit are not rendered) |
| 1 | `0x02` | user override: voxel manually **disabled** (drawn red). The JS calls it `BIT_ENABLED`, but the "add to disabled" action *sets* it and "add to enabled" *clears* it. Clear by default. |
| 2 | `0x04` | selected (UI) |
| 3 | `0x08` | temporary select (UI) |
| 4 | `0x10` | temporary deselect (UI) |
| 5 | `0x20` | pocket: excluded by pocket detection |
| 6 | `0x40` | infill/support voxel (named `BIT_INFILL` in the JS and `BIT_SUPPORT` in the shader) |
| 7 | `0x80` | set by the native addon and never read or written by the JS. In `cube.lys` it marks the core of the cavity, with a one-voxel ring of plain `0x01` voxels around it. Probably "inside by more than one voxel"; meaning not confirmed. |

Bits 2–4 are transient UI state and are normally cleared before a save. Bits 5
and 6 are cleared and recomputed each time hollowing runs.

**Spatial placement is not stored.** The file holds only the grid dimensions,
not its origin or voxel pitch. The app gets both (`voxelsCenter`, a float array
of voxel centres in mesh/STL space; pitch = `voxelsCenter[3] - voxelsCenter[0]`)
by recomputing them from the mesh with the native addon. The pitch is derived
from the wall thickness, roughly `max(0.5, max(0.5, wall/2)/2)` mm, but the
exact grid comes from native code. To place the voxels in space, an external
tool has to reproduce that computation or fit the grid to the mesh bounds.

**Validity on load:** the app reuses the stored grid only if `sizeX/Y/Z`,
`wallThickness` and `useVoxels` all match the object's current hollowing
settings. Otherwise it recomputes the grid, and any user enable/disable edits in
the file are lost.

---

## 6. Minimal read algorithm (pseudocode)

```
open file F
if F starts with ZIP magic (50 4B 03 04):
    unzip → read member "scene.bin"     # V1
    state = msgpack_decode(scene.bin)
else:                                    # V2+ MNGO
    headerSize = uint32_le(F, 0x04)
    jsonLen    = uint32_le(F, 0x0C)
    header     = JSON.parse(utf8(F[0x10 : 0x10 + jsonLen]))
    dataBase   = 0x08 + headerSize
    node       = header.mangoFiles["scene.bin"]
    raw        = F[dataBase + int(node.offset) : dataBase + int(node.offset) + node.size]
    state      = msgpack_decode(decipher(raw, fileAppId))
# other assets resolved the same way via header.mangoFiles[...]

for objectId in state.objects.present.allIds:
    if "<objectId>_islands.bin" in archive:
        islands = msgpack_decode(decipher(member, fileAppId))   # §5.2, cipher even in V1
    if "<objectId>_hollowVoxels.bin" in archive:
        voxels = decode_hollow_voxels(member)                   # §5.3, raw binary
```

---

## 7. Source references

In `bin/renderFile.js`:

| Concern | Location |
|---------|----------|
| Save orchestration | `saveToFile` — line 246481 |
| `scene.bin` write (V1 msgpack vs V2 encodeDataBuffer) | `_saveSceneToTemporaryDirectory` — line 246823 |
| Container write (ZIP vs MNGO) | `_saveTempDirectoryToFile` — line 246843 |
| MNGO header write | `writeFilesystem` — line 2442 |
| MNGO header read | `readArchiveHeaderSync` — line 2462 |
| File-content offset math | `readFileSync` — line 2503 (`8 + headerSize + offset`) |
| Header index node shapes | `Filesystem.insertFile` — line 2587 |
| Byte cipher (encode `ed` / decode `d`) | line 2550 / line 2557 |
| `scene.bin` encode/decode wrappers | `encodeDataBuffer` 154922, `decodeConfigAndScene` 154943 |
| Islands / voxels save requests (IPC `SITF` / `SHVTF`) | `_saveObjectsToTempDirectory` — line 246712 |
| Island member name on load | line 248690 |

The islands and hollow-voxel files are written and read in the 3D window, in
`bin/render3D.js`:

| Concern | Location |
|---------|----------|
| Islands writer | `SceneIslandsToLys.run` — line 403430 |
| Islands reader | `ImportMinimaFromMok` — line 320757 |
| Island record shape | `Minima.transformResultToJSONSVG` — line 314072 |
| Island geometry rebuild | `Minima.buildGeometryFromSVG` — line 314083 |
| Hollow-voxels writer + `compressRLE` | `SceneHollowVoxelsToLys.run` — line 333693 |
| Hollow-voxels reader + `decompressRLE` | `ImportHollowVoxelsFromLys` / `importFromDataBuffer` — line 333782 |
| Voxel bit masks | `HollowVoxelsHelper` — line 320150; shader — line 319939 |
| Voxel index formula | `Hollowing.detectPocketsWithoutHoles` — line 320560 |
| `_hollowing.bin` load | `Hollowing.run` — line 320298 |

---

## 8. Notes & caveats

- The byte cipher is **obfuscation, not encryption** — trivially reversible
  once `fileAppId` is known.
- "MNGO" = renamed `asar`; existing asar tooling works if you alias the header
  key `mangoFiles` → `files`.
- MessagePack is the standard codec; arrays/maps/strings/ints/floats/binary all
  decode normally. Large meshes are kept out of `scene.bin` as separate members.
- Max single member size is 4 GiB (uint32 offset/size fields).
- Islands use the cipher + MessagePack; hollow voxels are raw binary. Do not
  run hollow-voxel files through `decipher()`.
- The hollow-voxels writer uses `-1` as a fallback for missing sizes with
  `writeUInt32LE`, which throws for negative values. The save error is caught and
  the member is skipped, so a valid file never contains a `-1` size.

---

## 9. Findings from implementing a reader (MeshPeek)

Verified against real files (Lychee 8.0.0 saves) and implemented in `rust/crates/meshpeek-core/src/io/lys/`.
These fill gaps in the sections above.

### 9.1 Mesh members — `<meshHash>.bin` (and `_hollowing.bin`)

```
offset  size            field
0x00    4 × 5           u32 version (= 2), u32 header_len, u32 n_indices, u32 n_coords, u32 reserved
0x14    n_indices × 4   u32 triangle indices
...     n_coords × 4    f32 coordinates (x, y, z per vertex)
```

The data **always starts at byte 20**, even though `header_len` reads 12. File size is exactly
`20 + 4 * (n_indices + n_coords)`. The mesh is stored **centred on its origin**.

Members with the `.lyd` extension (`type: "libraryGeometry"`) are encrypted Lychee Library store models and
are not readable.

### 9.2 Object placement (`objects.present.byId[id]`)

- `rotation` is in **degrees**, applied about the mesh origin: X first, then Y, then Z (`R = Rz · Ry · Rx`),
  then `scale`. The order only matters for objects rotated about several axes; it was settled with
  `Nosferatu_Base_75mm_SUP.lys` (106°, −21°, 49°), where only this order puts the support tips above their
  feet at consistent heights and makes the brace tips of each support lie on one vertical column.
- `position.x/y` place the mesh origin. `position.z` is the height of the object's **lowest point after
  rotation** (the support lift); the translation is `position.z − min(R·S·v).z`.
- `center` is the part's centre in its original file; it is **not** used for placement.
- `type: "libraryFile"` objects load `properties.file` (e.g. Lychee's bundled `benchy.stl`); centre that mesh
  on its bounding box first, then place it as above.

### 9.3 Supports (`supports.present.byId`)

Source: Lychee's own `SupportTip.ts`, `SupportBase.ts`, `SupportMiddle.ts` and `CapsuleBufferGeometry.ts` (the bundle
`render3D.js`), ported in `meshpeek-core/src/io/lys/supports.rs`.

**Records.** `tip` and `tipNormal` are in the tip object's space (`objectIdTip`). A support **without** `isBaseTip`
stands on the plate: `base.z` is plate height, `base.x/y` are relative to the base object's `position.x/y` **and
turned by the object's Z rotation** (the support grid follows the yaw). A support **with** `isBaseTip` is a *branch*:
`base` is in the base object's space (`objectIdBase`), on a parent support (`parentBaseId` set) or on the model
itself (not set). `parentTipId`: the tip rests on another support. `isBracing` = both parents set and different.
`mini: true` marks mini supports. The plate support's `baseNormal` is ignored: its column is always vertical.

**Primitive: the capsule.** Every part is a *capsule*: the hull of two spheres whose centres are the chain's points
(radii `r_a`, `r_b`), so each round end reaches one radius past its point; an end can be cut flat at its centre.

**Positions** (`L` = straight distance from base to tip; `short = min(0.33 * L, 2)`, Lychee's
`PERCENT_LENGTH_BRACING` and `MAX_LENGTH_BRACING`):

- tip normal: the stored one, leaned towards `-Z` by `(100 - tip.angle)` percent (not for bracing); for a mini
  branch, the direction back to the base join.
- tip join = tip point + normal * `tip.length` (`short` for bracing and mini), pushed up if it would dip below the
  plate (`z < tip.diameter`: set to `tip.diameter / 2 + 0.1`).
- plate support: base join = foot + `(0, 0, lengthJoin)` with `lengthJoin = max(min(tip join z, base.joinLength),
  base.joinDiameter / 2)`.
- branch: orientation = `baseNormal`, leaned towards the tip by `(100 - baseTip.angle)` percent when it starts on the
  model (a mini points straight at the tip); base join = start + orientation * `baseTip.length` (`short` for bracing
  and mini).

**Parts and radii:**

- *Tip* (capsule, tip point to tip join): radius at the point `tip.pointDiameter / 2`, at the join `tip.diameter / 2`,
  both capped at `length * 1.75 / 2`, with `length = |tip point - tip join| + tip.penetration`; flat at the point when
  `parentTipId` is set, whose radii are then capped at the parent's diameter there. A **mini** has radius
  `tip.pointDiameter / 2` at both ends, reduced by 33% towards `baseTip.pointDiameter / 2`: a thin rod.
- *Base of a branch* (capsule, start to base join): same rules with `baseTip.*`; flat at the start when it is on a
  parent support, flat at the join end for a bracing.
- *Base of a plate support*: a column of radius `base.joinDiameter / 2` from the plate to the base join, flat at the
  plate, with a ball at the join; a **foot pad** (`base.length` thick, `base.diameter` wide, its top wider by
  `length * tan(base.angle)`; `base.type` can make it a square, diamond or octagon); and the **join cone**
  (`base.joinCone` tall, `joinDiameter / 2` wider than the column at the plate) on the pad. With a raft (section 9.3a)
  the pad is not drawn.
- *Middle* (capsule, base join to tip join): radius `base radius` at the base join (`joinDiameter / 2` for a plate
  support) and the tip's radius at the tip join; flat at the tip join when `tip.type` is not `cylinder`, flat at the
  base join for a branch unless `baseTip.type` is `cylinder`.
- A *brace* (`isBracing`) is drawn as **one straight capsule** from its base point to its tip point, as long as their
  distance: Lychee ignores the stored normals for it (they are the parents' surface normals) and uses the direction
  between the two points. It is `baseTip.pointDiameter` wide at the start and narrows to the diameter of the support
  it ends on (see below); both ends are flat.
- A brace matches the parent's diameter at its height, from Lychee's `smallestDiameterToBraceOn`: the base diameter
  below the parent's base join, the tip diameter above its tip point, interpolated in between.

**Shape types.** `tip.type` / `baseTip.type`: `cone` (default), `cube` (4-sided, turned 45 degrees), `cylinder`
(constant point radius) or `cross` (extruded 12-corner cross, with a ball at the join). `mid.type` (middle and plate
column): `cylinder`, `cube` (4-sided prism) or `cross`. `base.type` (pad): `cylinder`, `cone`, `cube`, `cube45`,
`octagon` or `cross`. `breakPoint` adds a ball at the tip / base point of a non-mini support that rests on the model.

Not ported: adaptive bases, FDM printers and the mini plate supports' sideways join offset.

### 9.3a Raft (`objects.present.byId.<id>.foundation`)

Source: Lychee's own `foundation/*.ts` (the bundle `render3D.js`), ported in `meshpeek-core/src/io/lys/raft.rs`.

- The raft is switched on **per object**: `foundation.enabled` (and `visible`). The scene setting
  `settings.batch.msla.raft` is not what drives it (35 of 55 sampled scenes have `foundation.enabled: true` with it
  off); MeshPeek uses that setting only for objects without a `foundation`.
- A raft is **not built from the supports' feet** (except `triangle`): it follows the object's footprint on the plate
  (its silhouette seen from below, or its bounding box), which Lychee computes when it runs. All of them are built in
  the object's frame without its yaw (Z rotation) and then turned by the yaw about the object's `position`.
- With `foundation.baseSupport`, Lychee paints a disc around the foot of every plate support into the silhouette
  before tracing it (radius `ceil(joinCone + joinDiameter / 2)`, rounded up to the raster step), so the silhouette
  rafts also cover feet that lie outside the model's outline.
- `foundation.type` and its parameters, with Lychee's defaults for the type in brackets:

| type | shape | parameters |
|---|---|---|
| `cylinder` | disc, radius `0.51 * diagonal of the footprint's bounding box * scale/100` | size (0.5), scale (100), angle (100) |
| `cube` | square (a 4-sided cylinder turned 45 degrees), half-diagonal `0.75 * the larger side * scale/100` | size (0.5), scale, angle (100) |
| `rectangle` | box `width`% x `height`% of the footprint's bounding box, times `scale`% | size (0.5), scale, angle (100), width, height (100) |
| `grid` | lattice of bars with a peaked profile, `wall` wide and `size` tall, every `2 * wall`, over the bounding box | size (1.5), wall (1), width, height, scale |
| `triangle` | a cylinder (12 sides) under every plate support and a thin bar along every Delaunay edge between them | size (0.5), scale (100) |
| `vectoriel` | the exact silhouette (holes included) extruded `size` thick | precision (1, divided by 5, at least 0.1 mm), size (0.5), scale (200), angle (100) |
| `vectorielWall` | the silhouette's concave hull (concavity 10 is practically the convex hull): a floor `size` thick and a rim wall `wall` wide | size (1), wall (1), extraWall (1), angle (200), concavity (10), precision |
| `pixelate`, `pixelateRound` | a small diamond (4 sides) or cylinder (12 sides) per silhouette cell of `precision` mm | precision (1), size (0.5), scale (200), angle (100) |

- **`angle` is a slope in percent: the top of the raft is wider than the bottom by `size * angle / 100`** (so 100 =
  45 degrees, and a 1 mm raft with `angle: 200` is 2 mm wider all round at the top). It applies to `cylinder`,
  `cube`, `rectangle`, `vectoriel`, `pixelate` and `vectorielWall`.
- `vectorielWall` is a dish: the floor's top is at `size`, the outer face leans out from the outline at the plate
  (z = 0) to `size * angle / 100` beyond it at `size + extraWall`, the rim wall's top is `wall` wide, and its inner
  face comes down to the floor.
- `triangle`: the cylinder's diameter is `ceil((joinCone + joinDiameter / 2) * 2) * scale / 100` (or
  `joinDiameter / 2` when `joinCone` is 0), `size + 0.01` tall; the bar between two supports is `joinDiameter` wide
  (the smaller of the two), `size` tall and the distance between them minus 0.4 long, shifted 0.2 towards the first.
  Collinear feet have no triangulation and so no bars.
- **The supports on a raft:** the foot pad (`base.diameter`, `base.type`) is **not drawn**; the column starts with its
  join cone (`base.joinCone` tall and `joinDiameter / 2` wider than the column at the plate). The cones are raised by
  `foundation.size` (minus 0.001) for `triangle`, `vectoriel`, `vectorielWall` and `pixelate*`, and stay at the plate
  for `cylinder`, `cube`, `rectangle` and `grid`.

### 9.4 Build plate

`plates.present.byId[id]` has `size` and `position` (mm). Skip plates with `isVirtual` or `visible: false`.
The plate is centred on `position` in X/Y and stands on its Z.
