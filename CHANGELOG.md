# Changelog

All notable changes to chitu2lys, newest first.

## 0.3.0 — 2026-10-15

### Added
- **`.ctp` projects** (the newer CHITUBOX project files) convert like `.chitubox` projects, from the command line
  and in the window: models, places and lifts, the support trees, braces and the preview. Their supports carry
  no triangles, so supports without foot pads get Lychee's default pads. The layout is in `docs/CTP_NOTES.md`.

### Changed
- Releases no longer include a macOS build (Windows and Linux only).

## 0.2.0 — 2026-10-13

### Added
- **chitu2lys**: converts a CHITUBOX project into a Lychee scene. Models keep their place and lift. CHITUBOX's
  support trees are rebuilt with Lychee's own, editable support types along the same paths: each tree becomes a
  support on the same foot (or on the model, or on the trunk it leans on) and a branch for every other tip, starting
  where CHITUBOX's branches meet and keeping their bends; CHITUBOX's braces become Lychee braces between the same
  trunks; trunk, branch and tip diameters and the square foot pads keep their size. About 98–99.9 % of CHITUBOX's
  support paths are drawn exactly (the tool reports it); the rest are trees on the model that bend more often than a
  Lychee support can, which are drawn within about a millimetre. The printer and settings come from a built-in
  template or from `--template`.
- **`--no-braces`** (and *Braces* in the window): leave CHITUBOX's braces out.
- **chitu2lys-gui** does the same in a small window: drop projects on it, pick the output folder, the printer and
  whether to convert the braces, and convert several at once.

### Changed
- chitu2lys is now its own project. It was first written inside MeshPeek (`mankoc/meshpeek`), whose CHITUBOX
  reader it now carries in `src/chitubox.rs`.
