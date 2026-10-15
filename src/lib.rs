//! chitu2lys: converts CHITUBOX projects (`.chitubox` and `.ctp`) into Lychee Slicer scenes (`.lys`). Used by the
//! `chitu2lys` command-line tool and the `chitu2lys-gui` window.

mod chitubox;
pub mod convert;
mod ctp;
mod ground;
mod lys;
mod mesh;
mod trees;

pub use convert::{Options, Report, convert};
