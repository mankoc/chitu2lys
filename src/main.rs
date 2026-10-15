//! chitu2lys: converts a CHITUBOX project (`.chitubox` or `.ctp`) into a Lychee Slicer scene (`.lys`).
//!
//!     chitu2lys <project.chitubox|project.ctp> [scene.lys] [--template settings.lys] [--no-braces]
//!
//! Every model keeps its place on the plate and its lift. CHITUBOX's support trees become editable Lychee supports
//! along the same paths: a support carrying each tree (on its foot, on the model, or on the tree it leans on), a
//! branch support for every other tip where the tips meet, and Lychee braces for CHITUBOX's braces (unless
//! `--no-braces`). The printer, resins and settings come from a built-in template, or from `--template` (any `.lys`
//! saved with the wanted printer).

use std::path::PathBuf;
use std::time::Instant;

fn main() {
    let mut args = std::env::args().skip(1);
    let (mut input, mut output, mut options) = (None, None, chitu2lys::Options::default());
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--template" => options.template = Some(args.next().map(PathBuf::from).unwrap_or_else(|| usage(2))),
            "--no-braces" => options.braces = false,
            "-h" | "--help" => usage(0),
            _ if input.is_none() => input = Some(PathBuf::from(arg)),
            _ if output.is_none() => output = Some(PathBuf::from(arg)),
            _ => usage(2),
        }
    }
    let Some(input) = input else { usage(2) };
    let output = output.unwrap_or_else(|| input.with_extension("lys"));
    let start = Instant::now();
    match chitu2lys::convert(&input, &output, &options) {
        Ok(report) => {
            println!(
                "{} -> {}: {} models ({} triangles), {} supports ({} on the plate, {} on the model, {} branches), {} braces in {:.1} s",
                input.display(),
                output.display(),
                report.objects,
                report.triangles,
                report.on_plate + report.on_model + report.branches,
                report.on_plate,
                report.on_model,
                report.branches,
                report.braces,
                start.elapsed().as_secs_f64()
            );
            if let Some(percent) = report.drawn_exactly() {
                println!("{percent:.1} % of CHITUBOX's support paths drawn as they were");
            }
            for skipped in &report.skipped {
                eprintln!("skipped {skipped}");
            }
            if !report.outside.is_empty() {
                eprintln!(
                    "note: partly outside the template printer's plate (choose a larger printer in Lychee, or use \
                     --template): {}",
                    report.outside.join(", ")
                );
            }
        }
        Err(e) => {
            eprintln!("error: {e}");
            std::process::exit(1);
        }
    }
}

/// Print the usage and exit with `code`.
fn usage(code: i32) -> ! {
    eprintln!("usage: chitu2lys <project.chitubox|project.ctp> [scene.lys] [--template settings.lys] [--no-braces]");
    eprintln!("  --template  take the printer, resins and settings from a scene saved in Lychee");
    eprintln!("  --no-braces leave CHITUBOX's braces out");
    std::process::exit(code)
}
