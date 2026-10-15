//! chitu2lys-gui: a small window around the converter. Drop or pick CHITUBOX projects, choose where the Lychee
//! scenes go and (optionally) a printer template, and convert; the work runs in the background.
//!
//!     chitu2lys-gui [projects.chitubox or .ctp...]

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")] // no console window in release builds

use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, channel};
use std::time::Instant;

use eframe::egui;

fn main() -> eframe::Result {
    let viewport = egui::ViewportBuilder::default()
        .with_title(concat!("chitu2lys ", env!("CARGO_PKG_VERSION"), " — CHITUBOX to Lychee"))
        .with_inner_size([620.0, 460.0])
        .with_min_inner_size([460.0, 320.0])
        .with_drag_and_drop(true);
    let options = eframe::NativeOptions { viewport, ..Default::default() };
    let mut converter = Converter::default();
    converter.add(std::env::args_os().skip(1).map(PathBuf::from)); // projects given on the command line
    converter.snapshot = std::env::var_os("CHITU2LYS_SNAPSHOT").map(|p| (PathBuf::from(p), 0));
    converter.convert_into = std::env::var_os("CHITU2LYS_SNAPSHOT_CONVERT").map(PathBuf::from);
    eframe::run_native("chitu2lys", options, Box::new(|_| Ok(Box::new(converter))))
}

/// What happened to one project.
enum State {
    Waiting,
    Converting,
    Done(String),
    Failed(String),
}

/// A message from the conversion thread: project `index` started, or finished with this outcome.
enum Event {
    Started(usize),
    Finished(usize, State),
    AllDone,
}

/// The window's state.
#[derive(Default)]
struct Converter {
    /// Projects to convert, with what happened to each.
    files: Vec<(PathBuf, State)>,
    /// Where the scenes go; `None` puts each next to its project.
    output: Option<PathBuf>,
    /// The printer template and whether braces are converted.
    options: chitu2lys::Options,
    /// Events from a running conversion.
    events: Option<Receiver<Event>>,
    /// Test hook `CHITU2LYS_SNAPSHOT=<file.png>`: save a screenshot after a few frames and quit.
    snapshot: Option<(PathBuf, u32)>,
    /// Test hook `CHITU2LYS_SNAPSHOT_CONVERT=<folder>`: convert into that folder first, snapshot when done.
    convert_into: Option<PathBuf>,
}

impl Converter {
    /// Add projects (anything that is not a `.chitubox` or `.ctp` file is ignored, as are files already listed).
    fn add(&mut self, paths: impl IntoIterator<Item = PathBuf>) {
        for path in paths {
            let is_project =
                path.extension().is_some_and(|e| e.eq_ignore_ascii_case("chitubox") || e.eq_ignore_ascii_case("ctp"));
            if is_project && !self.files.iter().any(|(p, _)| *p == path) {
                self.files.push((path, State::Waiting));
            }
        }
    }

    /// The scene file for `project`.
    fn output_for(&self, project: &Path) -> PathBuf {
        let name = project.with_extension("lys");
        match &self.output {
            Some(folder) => folder.join(name.file_name().unwrap_or_default()),
            None => name,
        }
    }

    /// Convert every listed project on a background thread.
    fn start(&mut self, ctx: &egui::Context) {
        let jobs: Vec<(usize, PathBuf, PathBuf)> =
            self.files.iter().enumerate().map(|(i, (p, _))| (i, p.clone(), self.output_for(p))).collect();
        self.files.iter_mut().for_each(|(_, state)| *state = State::Waiting);
        let (send, receive) = channel();
        let (options, ctx) = (self.options.clone(), ctx.clone());
        std::thread::spawn(move || {
            for (index, input, output) in jobs {
                let _ = send.send(Event::Started(index));
                ctx.request_repaint();
                let start = Instant::now();
                let state = match chitu2lys::convert(&input, &output, &options) {
                    Ok(report) => State::Done(summary(&report, &output, start.elapsed().as_secs_f64())),
                    Err(e) => State::Failed(e),
                };
                let _ = send.send(Event::Finished(index, state));
                ctx.request_repaint();
            }
            let _ = send.send(Event::AllDone);
            ctx.request_repaint();
        });
        self.events = Some(receive);
    }

    /// Take the conversion thread's news.
    fn receive(&mut self) {
        let Some(events) = &self.events else { return };
        let mut finished = false;
        while let Ok(event) = events.try_recv() {
            match event {
                Event::Started(i) => self.files[i].1 = State::Converting,
                Event::Finished(i, state) => self.files[i].1 = state,
                Event::AllDone => finished = true,
            }
        }
        if finished {
            self.events = None;
        }
    }
}

impl Converter {
    /// The screenshot test hook: ask for a screenshot on the 5th frame, save it when it arrives, then quit.
    fn take_snapshot(&mut self, ctx: &egui::Context) {
        let Some((path, _)) = &mut self.snapshot else { return };
        let shot = ctx.input(|i| {
            i.raw.events.iter().find_map(|e| match e {
                egui::Event::Screenshot { image, .. } => Some(image.clone()),
                _ => None,
            })
        });
        if let Some(image) = shot {
            let pixels: Vec<u8> = image.pixels.iter().flat_map(|c| c.to_array()).collect();
            let [w, h] = image.size;
            let _ = image::save_buffer(&*path, &pixels, w as u32, h as u32, image::ColorType::Rgba8);
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }
        if let Some(folder) = self.convert_into.take() {
            self.output = Some(folder);
            self.start(ctx);
        }
        if self.events.is_some() {
            ctx.request_repaint();
            return; // wait for the conversion
        }
        let Some((_, frames)) = &mut self.snapshot else { return };
        *frames += 1;
        if *frames == 5 {
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::default()));
        }
        ctx.request_repaint();
    }
}

/// One line about a finished conversion.
fn summary(report: &chitu2lys::Report, output: &Path, seconds: f64) -> String {
    let mut text = format!(
        "{} models, {} supports ({} on the plate, {} on the model, {} branches), {} braces, saved as {} in {seconds:.1} s",
        report.objects,
        report.on_plate + report.on_model + report.branches,
        report.on_plate,
        report.on_model,
        report.branches,
        report.braces,
        output.file_name().unwrap_or_default().to_string_lossy()
    );
    if let Some(percent) = report.drawn_exactly() {
        text += &format!("\n{percent:.1} % of the support paths drawn as in CHITUBOX");
    }
    if !report.skipped.is_empty() {
        text += &format!("\nSkipped: {}", report.skipped.join("; "));
    }
    if !report.outside.is_empty() {
        text += &format!("\n{} models stick out of the template printer's plate", report.outside.len());
    }
    text
}

impl eframe::App for Converter {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.receive();
        self.take_snapshot(ctx);
        let dropped: Vec<PathBuf> = ctx.input(|i| i.raw.dropped_files.iter().filter_map(|f| f.path.clone()).collect());
        if self.events.is_none() {
            self.add(dropped);
        }
        let busy = self.events.is_some();

        egui::TopBottomPanel::bottom("actions").show(ctx, |ui| {
            ui.add_space(6.0);
            egui::Grid::new("options").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
                ui.label("Save to:");
                ui.horizontal(|ui| {
                    if ui.add_enabled(!busy, egui::Button::new("Choose folder…")).clicked() {
                        self.output = rfd::FileDialog::new().pick_folder().or(self.output.take());
                    }
                    if self.output.is_some() && ui.add_enabled(!busy, egui::Button::new("🗙")).clicked() {
                        self.output = None;
                    }
                    let text =
                        self.output.as_ref().map_or("next to each project".to_string(), |p| p.display().to_string());
                    ui.add(egui::Label::new(text).truncate()).on_hover_text("Where the Lychee scenes are written");
                });
                ui.end_row();
                ui.label("Printer:");
                ui.horizontal(|ui| {
                    let pick = ui
                        .add_enabled(!busy, egui::Button::new("Use a .lys…"))
                        .on_hover_text("Take the printer and settings from a scene saved in Lychee");
                    if pick.clicked() {
                        let picked = rfd::FileDialog::new().add_filter("Lychee scene", &["lys"]).pick_file();
                        self.options.template = picked.or(self.options.template.take());
                    }
                    if self.options.template.is_some() && ui.add_enabled(!busy, egui::Button::new("🗙")).clicked() {
                        self.options.template = None;
                    }
                    let text = self.options.template.as_ref().map_or("built-in template".to_string(), |p| {
                        format!("from {}", p.file_name().unwrap_or_default().to_string_lossy())
                    });
                    ui.add(egui::Label::new(text).truncate())
                        .on_hover_text("The printer, resins and settings the scenes get");
                });
                ui.end_row();
                ui.label("Braces:");
                ui.add_enabled(!busy, egui::Checkbox::new(&mut self.options.braces, "Convert CHITUBOX's braces"))
                    .on_hover_text("Off: the scenes get the supports only, and Lychee's own braces can be added there");
                ui.end_row();
            });
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                let can_convert = !busy && !self.files.is_empty();
                if ui.add_enabled(can_convert, egui::Button::new("Convert").min_size(egui::vec2(110.0, 28.0))).clicked()
                {
                    self.start(ctx);
                }
                if busy {
                    ui.spinner();
                    ui.label("Converting…");
                }
            });
            ui.add_space(6.0);
        });

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.horizontal(|ui| {
                if ui.add_enabled(!busy, egui::Button::new("Add projects…")).clicked() {
                    let picked =
                        rfd::FileDialog::new().add_filter("CHITUBOX project", &["chitubox", "ctp"]).pick_files();
                    self.add(picked.unwrap_or_default());
                }
                if ui.add_enabled(!busy && !self.files.is_empty(), egui::Button::new("Clear list")).clicked() {
                    self.files.clear();
                }
            });
            ui.separator();
            if self.files.is_empty() {
                ui.centered_and_justified(|ui| {
                    ui.weak("Drop .chitubox or .ctp files here, or use Add projects…");
                });
                return;
            }
            egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
                let mut remove = None;
                for (index, (path, state)) in self.files.iter().enumerate() {
                    ui.horizontal(|ui| {
                        let (icon, colour) = match state {
                            State::Waiting => ("•", ui.visuals().weak_text_color()),
                            State::Converting => ("⟳", ui.visuals().text_color()),
                            State::Done(_) => ("✔", egui::Color32::from_rgb(90, 190, 110)),
                            State::Failed(_) => ("✖", ui.visuals().warn_fg_color),
                        };
                        ui.colored_label(colour, icon);
                        let name = path.file_name().unwrap_or_default().to_string_lossy();
                        ui.label(egui::RichText::new(name).strong()).on_hover_text(path.display().to_string());
                        if !busy && ui.small_button("🗙").on_hover_text("Remove from the list").clicked() {
                            remove = Some(index);
                        }
                    });
                    match state {
                        State::Done(text) => {
                            ui.weak(text);
                        }
                        State::Failed(text) => {
                            ui.colored_label(ui.visuals().warn_fg_color, text);
                        }
                        _ => {}
                    }
                    ui.add_space(4.0);
                }
                if let Some(index) = remove {
                    self.files.remove(index);
                }
            });
        });
    }
}
