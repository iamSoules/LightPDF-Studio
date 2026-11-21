//! In-app file browser for platforms without native dialogs (Android, iOS).
//! Large touch rows, folders first, multi-select.

use crate::i18n::tr;
use crate::platform::{self, IncomingFile};
use crate::theme::{self, Palette};
use eframe::egui::{self, RichText};
use std::collections::BTreeSet;
use std::path::PathBuf;

pub struct FileBrowser {
    dir: PathBuf,
    ext: Vec<String>,
    multi: bool,
    selected: BTreeSet<PathBuf>,
    entries: Vec<(PathBuf, bool, u64)>,
    error: Option<String>,
}

impl FileBrowser {
    pub fn new(ext: &[&str], multi: bool) -> Self {
        let dir = platform::browse_roots().into_iter().map(|r| r.1).find(|p| p.is_dir()).unwrap_or_else(|| PathBuf::from("/"));
        let mut b =
            Self { dir, ext: ext.iter().map(|e| e.to_string()).collect(), multi, selected: BTreeSet::new(), entries: vec![], error: None };
        b.refresh();
        b
    }

    fn refresh(&mut self) {
        self.entries.clear();
        self.error = None;
        match std::fs::read_dir(&self.dir) {
            Ok(rd) => {
                for e in rd.flatten() {
                    let p = e.path();
                    let name = e.file_name().to_string_lossy().into_owned();
                    if name.starts_with('.') {
                        continue;
                    }
                    let Ok(md) = e.metadata() else { continue };
                    if md.is_dir() {
                        self.entries.push((p, true, 0));
                    } else if p.extension().is_some_and(|x| self.ext.iter().any(|e| x.eq_ignore_ascii_case(e))) {
                        self.entries.push((p, false, md.len()));
                    }
                }
                self.entries.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.file_name().cmp(&b.0.file_name())));
            }
            Err(e) => self.error = Some(e.to_string()),
        }
    }

    fn cd(&mut self, p: PathBuf) {
        self.dir = p;
        self.refresh();
    }

    /// `Some(Some(files))` = picked, `Some(None)` = cancelled, `None` = still open.
    pub fn show(&mut self, ctx: &egui::Context, pal: &Palette) -> Option<Option<Vec<IncomingFile>>> {
        let mut result = None;
        let screen = ctx.content_rect();
        egui::Modal::new(egui::Id::unique("file-browser")).show(ctx, |ui| {
            ui.set_width((screen.width() - 32.0).min(560.0));
            ui.set_height((screen.height() - 120.0).min(640.0));
            ui.horizontal(|ui| {
                ui.heading(tr("Dateien"));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("✖").clicked() {
                        result = Some(None);
                    }
                });
            });
            ui.horizontal_wrapped(|ui| {
                for (label, p) in platform::browse_roots() {
                    if ui.selectable_label(self.dir == p, label).clicked() {
                        self.cd(p);
                    }
                }
            });
            ui.horizontal(|ui| {
                if ui.add_enabled(self.dir.parent().is_some(), egui::Button::new("⬆ ".to_string() + &tr("Nach oben"))).clicked()
                    && let Some(p) = self.dir.parent()
                {
                    self.cd(p.to_path_buf());
                }
                ui.label(RichText::new(self.dir.to_string_lossy()).small().color(pal.muted));
            });
            ui.separator();
            let footer = 56.0;
            egui::ScrollArea::vertical().max_height(ui.available_height() - footer).auto_shrink([false, false]).show(ui, |ui| {
                if let Some(e) = &self.error {
                    ui.colored_label(pal.danger, e);
                }
                if self.entries.iter().all(|e| e.1) && self.error.is_none() {
                    ui.label(RichText::new(tr("Keine PDF-Dateien in diesem Ordner")).color(pal.muted));
                }
                let mut go = None;
                let mut pick_now = None;
                for (p, is_dir, size) in &self.entries {
                    let name = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                    let sel = self.selected.contains(p);
                    let text = if *is_dir {
                        format!("🗀  {name}")
                    } else {
                        format!("{}  {name}    {}", if sel { "☑" } else { "🗋" }, crate::storage::human_size(*size))
                    };
                    let r = ui.add_sized(
                        [ui.available_width(), 44.0],
                        egui::Button::selectable(sel, text).wrap_mode(egui::TextWrapMode::Truncate),
                    );
                    if r.clicked() {
                        if *is_dir {
                            go = Some(p.clone());
                        } else if self.multi {
                            if !self.selected.remove(p) {
                                self.selected.insert(p.clone());
                            }
                        } else {
                            pick_now = Some(p.clone());
                        }
                    }
                }
                if let Some(p) = go {
                    self.cd(p);
                }
                if let Some(p) = pick_now {
                    self.selected = [p].into();
                    result = Some(Some(self.read_selected()));
                }
            });
            ui.separator();
            ui.horizontal(|ui| {
                let n = self.selected.len();
                if ui
                    .add_enabled(
                        n > 0,
                        egui::Button::new(RichText::new(format!("{} ({n})", tr("Öffnen"))).strong().color(pal.accent_ink)).fill(pal.accent),
                    )
                    .clicked()
                {
                    result = Some(Some(self.read_selected()));
                }
                if ui.button(tr("Abbrechen")).clicked() {
                    result = Some(None);
                }
                let _ = theme::card; // keep theme in scope for consistent look
            });
        });
        result
    }

    fn read_selected(&self) -> Vec<IncomingFile> {
        self.selected
            .iter()
            .filter_map(|p| {
                Some(IncomingFile {
                    name: crate::storage::file_name(p),
                    bytes: std::fs::read(p).ok()?,
                    origin: Some(p.to_string_lossy().into_owned()),
                })
            })
            .collect()
    }
}
