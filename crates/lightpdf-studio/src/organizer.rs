//! Organise pages: thumbnail grid, multi-select, rotate / delete / duplicate / move / extract.
//! Big touch targets; moving works with buttons and by dragging a page onto another.

use crate::app::{BrowsePurpose, Studio};
use crate::i18n::{tr, trf};
use crate::reader::Reader;
use crate::theme::{self, Palette};
use eframe::egui::{self, Align2, Color32, RichText, Sense, Stroke, TextureHandle, pos2, vec2};
use lightpdf_core::ops;
use std::collections::{BTreeSet, HashMap};

pub enum OrgAction {
    Close,
    Rotate(i32),
    Delete,
    Duplicate,
    /// Move selection before the given index.
    MoveTo(usize),
    Extract,
    InsertBlank,
    InsertPdf,
}

pub struct Organizer {
    pub selected: BTreeSet<usize>,
    pub wanted_thumbs: Vec<usize>,
    drag: Option<usize>,
}

impl Organizer {
    pub fn new(_pages: usize, current: usize) -> Self {
        Self { selected: [current].into(), wanted_thumbs: vec![], drag: None }
    }

    pub fn show(
        &mut self,
        ctx: &egui::Context,
        pal: &Palette,
        thumbs: &HashMap<usize, TextureHandle>,
        pages: usize,
        sizes: &[(f64, f64)],
    ) -> Option<OrgAction> {
        let mut act = None;
        let screen = ctx.content_rect();
        self.selected.retain(|&i| i < pages);
        egui::Modal::new(egui::Id::unique("organizer")).show(ctx, |ui| {
            ui.set_width(screen.width() - 32.0);
            ui.set_height(screen.height() - 48.0);
            ui.horizontal_wrapped(|ui| {
                ui.heading(tr("Seiten organisieren"));
                ui.label(RichText::new(trf("{0} ausgewählt", &[&self.selected.len()])).color(pal.muted));
            });
            ui.horizontal_wrapped(|ui| {
                let any = !self.selected.is_empty();
                if ui.add_enabled(any, egui::Button::new("⟲")).on_hover_text(tr("Drehen")).clicked() {
                    act = Some(OrgAction::Rotate(-90));
                }
                if ui.add_enabled(any, egui::Button::new("⟳")).on_hover_text(tr("Drehen")).clicked() {
                    act = Some(OrgAction::Rotate(90));
                }
                if ui.add_enabled(any, egui::Button::new(format!("⎘ {}", tr("Duplizieren")))).clicked() {
                    act = Some(OrgAction::Duplicate);
                }
                let first = self.selected.iter().next().copied().unwrap_or(0);
                let last = self.selected.iter().last().copied().unwrap_or(0);
                if ui.add_enabled(any && first > 0, egui::Button::new("⏴")).on_hover_text(tr("Nach vorne")).clicked() {
                    act = Some(OrgAction::MoveTo(first - 1));
                }
                if ui.add_enabled(any && last + 1 < pages, egui::Button::new("⏵")).on_hover_text(tr("Nach hinten")).clicked() {
                    act = Some(OrgAction::MoveTo(last + 2));
                }
                if ui.add_enabled(any, egui::Button::new(format!("⇪ {}", tr("Extrahieren")))).clicked() {
                    act = Some(OrgAction::Extract);
                }
                if ui.button(format!("➕ {}", tr("Leere Seite"))).clicked() {
                    act = Some(OrgAction::InsertBlank);
                }
                if ui.button(format!("📄 {}", tr("PDF einfügen …"))).clicked() {
                    act = Some(OrgAction::InsertPdf);
                }
                if ui
                    .add_enabled(
                        any && self.selected.len() < pages,
                        egui::Button::new(RichText::new(format!("🗑 {}", tr("Löschen"))).color(pal.danger)),
                    )
                    .clicked()
                {
                    act = Some(OrgAction::Delete);
                }
                ui.separator();
                if ui.button(tr("Alle auswählen")).clicked() {
                    self.selected = (0..pages).collect();
                }
                if ui.button(tr("Auswahl aufheben")).clicked() {
                    self.selected.clear();
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if theme::primary(ui, pal, tr("Fertig")).clicked() {
                        act = Some(OrgAction::Close);
                    }
                });
            });
            ui.separator();
            let ppp = ctx.pixels_per_point();
            let cell = if screen.width() < 600.0 { (screen.width() - 80.0) / 3.0 } else { 150.0 };
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                let cols = ((ui.available_width() + 12.0) / (cell + 12.0)).floor().max(1.0) as usize;
                let pointer = ctx.input(|i| i.pointer.interact_pos());
                let mut drop_target = None;
                egui::Grid::new("org-grid").spacing([12.0, 12.0]).show(ui, |ui| {
                    for i in 0..pages {
                        let (w, h) = sizes.get(i).copied().unwrap_or((595.0, 842.0));
                        let k = (cell / w.max(h) as f32).min(1.0);
                        let (tw, th) = (w as f32 * k, h as f32 * k);
                        let (rect, r) = ui.allocate_exact_size(vec2(cell, cell + 18.0), Sense::click_and_drag());
                        if ui.is_rect_visible(rect) {
                            if !thumbs.contains_key(&i) {
                                self.wanted_thumbs.push(i);
                            }
                            let img = egui::Rect::from_center_size(pos2(rect.center().x, rect.min.y + cell / 2.0), vec2(tw, th));
                            let p = ui.painter();
                            p.rect_filled(img, 2.0, Color32::WHITE);
                            if let Some(t) = thumbs.get(&i) {
                                p.image(t.id(), img, egui::Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
                            }
                            let sel = self.selected.contains(&i);
                            p.rect_stroke(
                                img,
                                2.0,
                                Stroke::new(if sel { 3.0 } else { 1.0 }, if sel { pal.accent } else { pal.line }),
                                egui::StrokeKind::Outside,
                            );
                            if sel {
                                p.circle_filled(img.right_top() + vec2(-12.0, 12.0), 10.0, pal.accent);
                                p.text(
                                    img.right_top() + vec2(-12.0, 12.0),
                                    Align2::CENTER_CENTER,
                                    "✔",
                                    egui::FontId::proportional(12.0),
                                    pal.accent_ink,
                                );
                            }
                            p.text(
                                pos2(rect.center().x, rect.max.y),
                                Align2::CENTER_BOTTOM,
                                (i + 1).to_string(),
                                egui::FontId::proportional(12.0),
                                pal.muted,
                            );
                            if self.drag.is_some() && pointer.is_some_and(|q| rect.contains(q)) {
                                drop_target = Some(i);
                                p.line_segment([rect.left_top(), rect.left_bottom()], Stroke::new(3.0, pal.accent));
                            }
                        }
                        if r.clicked() && !self.selected.remove(&i) {
                            self.selected.insert(i);
                        }
                        if r.drag_started() {
                            self.drag = Some(i);
                            if !self.selected.contains(&i) {
                                self.selected = [i].into();
                            }
                        }
                        if (i + 1) % cols == 0 {
                            ui.end_row();
                        }
                    }
                });
                let _ = ppp;
                if self.drag.is_some() && ctx.input(|i| i.pointer.any_released()) {
                    self.drag = None;
                    if let Some(t) = drop_target {
                        act = Some(OrgAction::MoveTo(t));
                    }
                }
            });
        });
        act
    }

    /// Applies an action; the organizer stays open unless it was closed.
    pub fn apply(mut self, action: OrgAction, r: &mut Reader, s: &mut Studio, ctx: &egui::Context) {
        let sel: Vec<usize> = self.selected.iter().copied().collect();
        let n = r.pages.len();
        let label = tr("Bearbeite Seiten …");
        match action {
            OrgAction::Close => return,
            OrgAction::Rotate(d) => r.page_op(&mut s.tasks, ctx, label, move |b| ops::rotate_pages(b, &sel, d)),
            OrgAction::Delete => {
                self.selected.clear();
                r.page_op(&mut s.tasks, ctx, label, move |b| ops::delete_pages(b, &sel));
            }
            OrgAction::Duplicate => {
                let mut order: Vec<usize> = Vec::new();
                for i in 0..n {
                    order.push(i);
                    if sel.contains(&i) {
                        order.push(i);
                    }
                }
                r.page_op(&mut s.tasks, ctx, label, move |b| ops::reorder_pages(b, &order));
            }
            OrgAction::MoveTo(target) => {
                let rest: Vec<usize> = (0..n).filter(|i| !sel.contains(i)).collect();
                let before = rest.iter().filter(|&&i| i < target).count();
                let mut order = rest.clone();
                for (k, &p) in sel.iter().enumerate() {
                    order.insert(before + k, p);
                }
                self.selected = (before..before + sel.len()).collect();
                r.page_op(&mut s.tasks, ctx, label, move |b| ops::reorder_pages(b, &order));
            }
            OrgAction::Extract => {
                let base = r.export_bytes(false);
                let name = format!("{} ({})", r.meta.name, tr("Auszug"));
                s.tasks.spawn(ctx, tr("Bitte warten …"), move |_| {
                    let res = base.map_err(lightpdf_core::pdfdoc::Error::Other).and_then(|b| ops::extract_pages(&b, &sel));
                    Box::new(move |s: &mut Studio| match res {
                        Ok(bytes) => match s.storage.add(&name, &bytes, None) {
                            Ok(_) => s.toast(tr("Als neues Dokument gespeichert")),
                            Err(e) => s.toast_err(e.to_string()),
                        },
                        Err(e) => s.toast_err(e.to_string()),
                    })
                });
            }
            OrgAction::InsertBlank => {
                let at = sel.iter().last().map(|l| l + 1).unwrap_or(n);
                r.page_op(&mut s.tasks, ctx, label, move |b| ops::insert_blank(b, at));
            }
            OrgAction::InsertPdf => {
                let at = sel.iter().last().map(|l| l + 1).unwrap_or(n);
                r.current_page = at.saturating_sub(1);
                if let Some(f) = s.pick(BrowsePurpose::InsertPdf) {
                    r.files_picked(BrowsePurpose::InsertPdf, f);
                }
            }
        }
        r.organizer = Some(self);
    }
}
