# CHITUBOX `.ctp` projects

A `.ctp` file is a CHITUBOX project in a newer layout than `.chitubox` (see `CHITUBOX_FILE_FORMAT.md`): the
models of a scene with their supports, with 64-bit offsets and support parts that carry no triangles. This was
worked out from one sample file (13 models, 6,572 support records, 84 MB) for MeshPeek; the reader here is
`src/ctp.rs`. Everything is little-endian; offsets are absolute.

## Header

| Offset | Type | Meaning |
|---|---|---|
| 0 | u32 | Magic `0xAB231253` (`.chitubox` projects: `0xAB231243`) |
| 4 | u32 | Object count (repeated at 8) |
| 24 | u64 | Offset of the object table |
| 32 | u64 | End of the preview image (start of the support-settings table) |
| 48 | u64 | Offset of the preview header |
| 64 | u64 | Offset of a table pointing at the support-settings data |
| 80 | — | Preview header: width, height (u32), offset and size of the image (u64); the image uses the 15-bit run-length encoding of `.chitubox` previews |

Between the preview and the object table lie a support-settings table (48 bytes per object: values such as 110,
1.0, 1.8, 30, 2, 2 and the object's id) and binary support-settings data, which are not needed. The file holds no
printer.

## Object record (216 bytes, magic `0xAB231254`)

| Offset | Type | Meaning |
|---|---|---|
| 0 | u32 | Magic |
| 4 | u32 | Object id (also in the support records) |
| 8, 12 | f32 | X, Y of the model's centre on the plate |
| 16 | f32 | Half the mesh's height: the mesh's origin is this far above its lowest point |
| 20 | f32 | Lift: the gap between the plate and the model's lowest point |
| 32 | f32 ×3 | Size of the model before its rotation |
| 44 | f32 ×6 | Scale and more (1 in the sample) |
| 68 | f32 ×9 | The first two rows of the model's rotation (the third is zero); the rotation is already applied to the mesh |
| 104 | f32 ×16 | A 4 × 4 matrix (identity in the sample) |
| 172 | u32 | 1 |
| 176 | u32 | Length of the name |
| 184 | u64 | Offset of the name (UTF-8, no terminator; all names lie back to back after the table) |
| 192 | u64 | Offset of the mesh block |
| 200 | u64 | Offset of the first support record |
| 208 | u64 | Support record count |

Placement: the mesh's origin goes to (X, Y, half height + lift). The lowest support part then reaches z = 0, the
plate, for every object of the sample.

## Mesh block (magic `0xAB231255`)

Magic (u32, then 4 zero bytes), the offset and count of the vertices (u64 each), the offset and count of the
triangle indices (u64 each), then the data: vertices as three f32, centred on the origin with the object's
rotation applied, and indices as u32, three per triangle.

## Support records (40 bytes, magic `0xAB231256`)

Magic (u32, then 4 zero bytes), the object id (u64), 0 (u64), the number of parts (u64) and the offset of the first
part (u64). Each part is 48 bytes:

| Offset | Type | Meaning |
|---|---|---|
| 0 | u32 | Part type, as in `.chitubox` projects: 1 top tip, 2 bottom tip, 3 column or brace, 4 foot cone, 5 foot pad, 7 contact ball, 9 joint, 12 link rod |
| 4 | u32 | A style word (1, 5, 7, 8 or 9; meaning unknown) |
| 8 | f32 ×3 | End point A, in the model's frame |
| 20 | f32 ×3 | End point B |
| 32 | f32 | Radius at A (`sizeA`) |
| 36 | f32 | Radius at B (`sizeB`) |
| 40 | f32 ×2 | 0 in the sample |

The sample has 2,539 + 292 columns, 621 + 45 foot cones, 988 contact balls, 1,046 + 46 joints, 989 top tips, 2
bottom tips and 4 link rods; no foot pads.
