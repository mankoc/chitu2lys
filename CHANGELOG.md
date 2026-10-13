# Changelog

All notable changes to chitu2lys, newest first.

## Unreleased

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
