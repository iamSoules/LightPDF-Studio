//! Library: all documents with covers, folders, stars, search, sorting, multi-select.

use crate::app::{BrowsePurpose, Studio};
use crate::i18n::{tr, trf};
use crate::storage::{DocMeta, Folder, SortBy, human_size};
use crate::theme::{self, Palette};
use eframe::egui::{self, Color32, CornerRadius, RichText, Sense, Stroke, TextureHandle, Vec2};
use std::collections::{BTreeSet, HashMap, HashSet};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender, channel};

#[derive(Clone, PartialEq, Eq, Default)]
pub enum Filter {
    #[default]
    All,
    Starred,
    Folder(String),
}

type ThumbResult = (String, Option<egui::ColorImage>);

pub struct Library {
    pub search: String,
    pub filter: Filter,
    pub selected: BTreeSet<String>,
    pub select_mode: bool,
    thumbs: HashMap<String, Option<TextureHandle>>,
    pending: HashSet<String>,
    tx: Option<Sender<(String, std::path::PathBuf, std::path::PathBuf)>>,
    rx: Option<Receiver<ThumbResult>>,
    rename: Option<(String, String)>,
    new_folder: Option<String>,
    confirm_delete: Option<Vec<String>>,
}

impl Default for Library {
    fn default() -> Self {
        Self {
            search: String::new(),
            filter: Filter::All,
            selected: BTreeSet::new(),
            select_mode: false,
            thumbs: HashMap::new(),
            pending: HashSet::new(),
            tx: None,
            rx: None,
            rename: None,
            new_folder: None,
            confirm_delete: None,
        }
    }
}

const FOLDER_COLORS: [&str; 6] = ["#4c6a47", "#2f6bd6", "#c58cf2", "#f0905a", "#d64545", "#f6c945"];

impl Library {
    fn ensure_worker(&mut self, ctx: &egui::Context) {
        if self.tx.is_some() {
            return;
        }
        let (tx, jobs) = channel::<(String, std::path::PathBuf, std::path::PathBuf)>();
        let (res_tx, rx) = channel();
        let ctx = ctx.clone();
        std::thread::Builder::new()
            .name("lightpdf-thumbs".into())
            .stack_size(16 << 20)
            .spawn(move || {
                while let Ok((id, doc, thumb)) = jobs.recv() {
                    let img = make_thumb(&doc, &thumb);
                    if res_tx.send((id, img)).is_err() {
                        break;
                    }
                    ctx.request_repaint();
                }
            })
            .ok();
        self.tx = Some(tx);
        self.rx = Some(rx);
    }

    fn thumb(&mut self, ctx: &egui::Context, s: &crate::storage::Storage, m: &DocMeta) -> Option<TextureHandle> {
        if let Some(t) = self.thumbs.get(&m.id) {
            return t.clone();
        }
        if m.encrypted || self.pending.contains(&m.id) {
            return None;
        }
        // cached on disk?
        if let Some(img) = std::fs::read(s.thumb_path(&m.id)).ok().and_then(|b| load_png(&b)) {
            let t = ctx.load_texture(format!("thumb-{}", m.id), img, egui::TextureOptions::LINEAR);
            self.thumbs.insert(m.id.clone(), Some(t.clone()));
            return Some(t);
        }
        self.ensure_worker(ctx);
        self.pending.insert(m.id.clone());
        if let Some(tx) = &self.tx {
            let _ = tx.send((m.id.clone(), s.doc_path(&m.id), s.thumb_path(&m.id)));
        }
        None
    }

    fn poll(&mut self, ctx: &egui::Context) {
        let Some(rx) = &self.rx else { return };
        while let Ok((id, img)) = rx.try_recv() {
            self.pending.remove(&id);
            let t = img.map(|img| ctx.load_texture(format!("thumb-{id}"), img, egui::TextureOptions::LINEAR));
            self.thumbs.insert(id, t);
        }
    }

    pub fn forget_thumb(&mut self, id: &str) {
        self.thumbs.remove(id);
    }
}

fn load_png(b: &[u8]) -> Option<egui::ColorImage> {
    let i = image::load_from_memory(b).ok()?.to_rgba8();
    Some(egui::ColorImage::from_rgba_unmultiplied([i.width() as usize, i.height() as usize], &i))
}

fn make_thumb(doc: &std::path::Path, out: &std::path::Path) -> Option<egui::ColorImage> {
    let bytes = std::fs::read(doc).ok()?;
    let pdf = lightpdf_core::render::open(Arc::new(bytes), None).ok()?;
    let info = *lightpdf_core::render::page_infos(&pdf).first()?;
    let g = info.geom(0);
    let scale = 320.0 / g.width().max(g.height());
    let cache = hayro_cache();
    let bmp = lightpdf_core::render::render_page(&pdf, &cache, 0, scale, 0, None)?;
    let png = lightpdf_core::ops::encode_bitmap(&bmp, lightpdf_core::ops::ImageFormat::Png).ok()?;
    let _ = std::fs::write(out, png);
    Some(egui::ColorImage::from_rgba_unmultiplied([bmp.w as usize, bmp.h as usize], &bmp.rgba))
}

fn hayro_cache<'a>() -> lightpdf_core::render::RenderCache<'a> {
    lightpdf_core::render::RenderCache::new()
}

fn sorted_docs(s: &Studio) -> Vec<DocMeta> {
    let lib = &s.library;
    let q = lib.search.to_lowercase();
    let mut docs: Vec<DocMeta> = s
        .storage
        .lib
        .docs
        .iter()
        .filter(|d| match &lib.filter {
            Filter::All => true,
            Filter::Starred => d.starred,
            Filter::Folder(f) => d.folder_id.as_deref() == Some(f),
        })
        .filter(|d| q.is_empty() || d.name.to_lowercase().contains(&q))
        .cloned()
        .collect();
    match s.storage.lib.settings.sort {
        SortBy::Recent => docs.sort_by(|a, b| b.opened_at.max(b.added_at).total_cmp(&a.opened_at.max(a.added_at))),
        SortBy::Added => docs.sort_by(|a, b| b.added_at.total_cmp(&a.added_at)),
        SortBy::Name => docs.sort_by_key(|a| a.name.to_lowercase()),
        SortBy::Size => docs.sort_by(|a, b| b.size.cmp(&a.size)),
    }
    docs
}

enum Act {
    Open(String),
    Star(String),
    Rename(String),
    Delete(Vec<String>),
    Move(Vec<String>, Option<String>),
    Export(String),
    Merge(Vec<String>),
}

/// View toggle and sort order (laid out right-to-left).
fn view_and_sort(s: &mut Studio, ui: &mut egui::Ui, with_label: bool) {
    let grid = s.storage.lib.settings.grid;
    if ui.button(if grid { "☰" } else { "▦" }).on_hover_text(tr("Ansicht")).clicked() {
        s.storage.lib.settings.grid = !grid;
        s.storage.save();
    }
    let sort = s.storage.lib.settings.sort;
    let combo = if with_label { egui::ComboBox::new("sort", tr("Sortieren")) } else { egui::ComboBox::from_id_salt("sort") };
    combo
        .selected_text(match sort {
            SortBy::Recent => tr("Zuletzt geöffnet"),
            SortBy::Added => tr("Hinzugefügt"),
            SortBy::Name => tr("Name"),
            SortBy::Size => tr("Größe"),
        })
        .show_ui(ui, |ui| {
            let st = &mut s.storage.lib.settings.sort;
            ui.selectable_value(st, SortBy::Recent, tr("Zuletzt geöffnet"));
            ui.selectable_value(st, SortBy::Added, tr("Hinzugefügt"));
            ui.selectable_value(st, SortBy::Name, tr("Name"));
            ui.selectable_value(st, SortBy::Size, tr("Größe"));
        });
    if s.storage.lib.settings.sort != sort {
        s.storage.save();
    }
}

pub fn show(s: &mut Studio, ui: &mut egui::Ui) {
    let ctx = ui.ctx().clone();
    s.library.poll(&ctx);
    let pal = s.pal;
    let narrow = ctx.content_rect().width() < 640.0;
    let mut acts: Vec<Act> = Vec::new();

    // ---------------------------------------------------------------- header
    egui::Panel::top("lib-top")
        .frame(egui::Frame::new().fill(pal.bg).inner_margin(egui::Margin { left: 16, right: 16, top: 12, bottom: 8 }))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new("📄").size(22.0));
                ui.label(RichText::new(crate::APP_NAME).heading().strong());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.menu_button(RichText::new("☰").size(18.0), |ui| {
                        if ui.button(format!("🗁  {}", tr("Öffnen …"))).clicked()
                            && let Some(f) = s.pick(BrowsePurpose::Open)
                        {
                            s.handle_picked(BrowsePurpose::Open, f);
                        }
                        if ui.button(format!("⧉  {}", tr("Zusammenführen"))).clicked()
                            && let Some(f) = s.pick(BrowsePurpose::Merge)
                        {
                            s.handle_picked(BrowsePurpose::Merge, f);
                        }
                        if ui.button(format!("🖼  {}", tr("Bilder → PDF"))).clicked()
                            && let Some(f) = s.pick(BrowsePurpose::ImagesToPdf)
                        {
                            s.handle_picked(BrowsePurpose::ImagesToPdf, f);
                        }
                        if ui.button(format!("🗋  {}", tr("Leeres PDF"))).clicked() {
                            crate::dialogs::create_blank(s);
                        }
                        ui.separator();
                        if ui.button(format!("⚙  {}", tr("Einstellungen"))).clicked() {
                            s.show_settings = true;
                        }
                        if ui.button(format!("ℹ  {}", tr("Über"))).clicked() {
                            s.show_about = true;
                        }
                    });
                    let open_label = if narrow { "🗁".to_string() } else { format!("🗁  {}", tr("Öffnen")) };
                    if theme::primary(ui, &pal, open_label).clicked()
                        && let Some(f) = s.pick(BrowsePurpose::Open)
                    {
                        s.handle_picked(BrowsePurpose::Open, f);
                    }
                    if !narrow {
                        view_and_sort(s, ui, true);
                        ui.add(
                            egui::TextEdit::singleline(&mut s.library.search)
                                .hint_text(format!("🔍 {}", tr("Dokumente suchen …")))
                                .desired_width(220.0),
                        );
                    }
                });
            });
            if narrow {
                // Phone: search, view and sort get their own row below the title
                ui.horizontal(|ui| {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        view_and_sort(s, ui, false);
                        ui.add(
                            egui::TextEdit::singleline(&mut s.library.search)
                                .hint_text(format!("🔍 {}", tr("Dokumente suchen …")))
                                .desired_width(ui.available_width()),
                        );
                    });
                });
            }
            // folders
            egui::ScrollArea::horizontal().id_salt("folders").show(ui, |ui| {
                ui.horizontal(|ui| {
                    let f = &mut s.library.filter;
                    if ui.selectable_label(*f == Filter::All, tr("Alle Dokumente")).clicked() {
                        *f = Filter::All;
                    }
                    if ui.selectable_label(*f == Filter::Starred, format!("★ {}", tr("Markiert"))).clicked() {
                        *f = Filter::Starred;
                    }
                    let folders = s.storage.lib.folders.clone();
                    let mut delete_folder = None;
                    for fo in &folders {
                        let sel = *f == Filter::Folder(fo.id.clone());
                        let r = ui.selectable_label(
                            sel,
                            RichText::new(format!("● {}", fo.name)).color(if sel { pal.text } else { theme::color_of(&fo.color) }),
                        );
                        if r.clicked() {
                            *f = Filter::Folder(fo.id.clone());
                        }
                        r.context_menu(|ui| {
                            if ui.button(tr("Ordner löschen")).clicked() {
                                delete_folder = Some(fo.id.clone());
                                ui.close();
                            }
                        });
                    }
                    if let Some(id) = delete_folder {
                        s.storage.lib.folders.retain(|x| x.id != id);
                        for d in &mut s.storage.lib.docs {
                            if d.folder_id.as_deref() == Some(&id) {
                                d.folder_id = None;
                            }
                        }
                        s.storage.save();
                        s.library.filter = Filter::All;
                    }
                    if ui.button(format!("+ {}", tr("Neuer Ordner"))).clicked() {
                        s.library.new_folder = Some(String::new());
                    }
                });
            });
        });

    // ---------------------------------------------------------------- selection bar
    if s.library.select_mode {
        egui::Panel::bottom("lib-select").frame(egui::Frame::new().fill(pal.panel).inner_margin(12)).show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                let sel: Vec<String> = s.library.selected.iter().cloned().collect();
                ui.label(trf("{0} ausgewählt", &[&sel.len()]));
                if ui.add_enabled(sel.len() >= 2, egui::Button::new(format!("⧉ {}", tr("Zusammenführen")))).clicked() {
                    acts.push(Act::Merge(sel.clone()));
                }
                ui.menu_button(format!("🗀 {}", tr("Verschieben")), |ui| {
                    if ui.button(tr("Kein Ordner")).clicked() {
                        acts.push(Act::Move(sel.clone(), None));
                    }
                    for f in &s.storage.lib.folders {
                        if ui.button(&f.name).clicked() {
                            acts.push(Act::Move(sel.clone(), Some(f.id.clone())));
                        }
                    }
                });
                if ui
                    .add_enabled(!sel.is_empty(), egui::Button::new(RichText::new(format!("🗑 {}", tr("Löschen"))).color(pal.danger)))
                    .clicked()
                {
                    acts.push(Act::Delete(sel.clone()));
                }
                if ui.button(tr("Fertig")).clicked() {
                    s.library.select_mode = false;
                    s.library.selected.clear();
                }
            });
        });
    }

    // ---------------------------------------------------------------- documents
    let docs = sorted_docs(s);
    egui::CentralPanel::no_frame().show(ui, |ui| {
        if docs.is_empty() {
            ui.vertical_centered(|ui| {
                ui.add_space(ui.available_height() * 0.25);
                ui.label(RichText::new("📄").size(56.0).color(pal.muted));
                ui.label(RichText::new(tr("Noch keine Dokumente")).heading());
                ui.label(RichText::new(tr("Öffne ein PDF, um loszulegen.")).color(pal.muted));
                ui.add_space(10.0);
                if theme::primary(ui, &pal, format!("🗁  {}", tr("Datei öffnen"))).clicked()
                    && let Some(f) = s.pick(BrowsePurpose::Open)
                {
                    s.handle_picked(BrowsePurpose::Open, f);
                }
                if !crate::platform::MOBILE {
                    ui.label(RichText::new(tr("Dateien hier ablegen")).small().color(pal.muted));
                }
            });
            return;
        }
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            egui::Frame::new().inner_margin(egui::Margin::symmetric(16, 8)).show(ui, |ui| {
                if s.storage.lib.settings.grid {
                    let w = ui.available_width();
                    let card_w = if narrow { ((w - 12.0) / 2.0).max(120.0) } else { 168.0 };
                    let cols = ((w + 14.0) / (card_w + 14.0)).floor().max(1.0) as usize;
                    egui::Grid::new("docs").spacing([14.0, 16.0]).show(ui, |ui| {
                        for (i, d) in docs.iter().enumerate() {
                            doc_card(s, ui, d, card_w, &pal, &mut acts);
                            if (i + 1) % cols == 0 {
                                ui.end_row();
                            }
                        }
                    });
                } else {
                    for d in &docs {
                        doc_row(s, ui, d, &pal, &mut acts);
                    }
                }
                ui.add_space(40.0);
            });
        });
    });

    dialogs(s, &ctx, &mut acts);

    // ---------------------------------------------------------------- actions
    for a in acts {
        match a {
            Act::Open(id) => {
                if s.library.select_mode {
                    if !s.library.selected.remove(&id) {
                        s.library.selected.insert(id);
                    }
                } else {
                    s.open_doc_id(&id);
                }
            }
            Act::Star(id) => {
                if let Some(d) = s.storage.lib.docs.iter_mut().find(|d| d.id == id) {
                    d.starred = !d.starred;
                }
                s.storage.save();
            }
            Act::Rename(id) => {
                let name = s.storage.get(&id).map(|d| d.name).unwrap_or_default();
                s.library.rename = Some((id, name));
            }
            Act::Delete(ids) => s.library.confirm_delete = Some(ids),
            Act::Move(ids, folder) => {
                for d in &mut s.storage.lib.docs {
                    if ids.contains(&d.id) {
                        d.folder_id = folder.clone();
                    }
                }
                s.storage.save();
                s.library.select_mode = false;
                s.library.selected.clear();
            }
            Act::Export(id) => {
                if let (Ok(bytes), Some(m)) = (s.storage.read_doc(&id), s.storage.get(&id)) {
                    match crate::platform::save_as(&format!("{}.pdf", m.name), &bytes, "pdf") {
                        Ok(Some(_)) => s.toast(tr("Gespeichert")),
                        Ok(None) => {}
                        Err(e) => s.toast_err(e),
                    }
                }
            }
            Act::Merge(ids) => {
                let files: Vec<crate::platform::IncomingFile> = ids
                    .iter()
                    .filter_map(|id| {
                        Some(crate::platform::IncomingFile {
                            name: s.storage.get(id)?.name,
                            bytes: s.storage.read_doc(id).ok()?,
                            origin: None,
                        })
                    })
                    .collect();
                s.library.select_mode = false;
                s.library.selected.clear();
                crate::dialogs::create_from_files(s, BrowsePurpose::Merge, files);
            }
        }
    }
}

fn doc_menu(ui: &mut egui::Ui, s: &Studio, d: &DocMeta, acts: &mut Vec<Act>) {
    if ui.button(format!("🗁  {}", tr("Öffnen"))).clicked() {
        acts.push(Act::Open(d.id.clone()));
        ui.close();
    }
    if ui
        .button(format!("{}  {}", if d.starred { "☆" } else { "★" }, if d.starred { tr("Markierung entfernen") } else { tr("Markieren") }))
        .clicked()
    {
        acts.push(Act::Star(d.id.clone()));
        ui.close();
    }
    if ui.button(format!("✏  {}", tr("Umbenennen"))).clicked() {
        acts.push(Act::Rename(d.id.clone()));
        ui.close();
    }
    ui.menu_button(format!("🗀  {}", tr("Verschieben")), |ui| {
        if ui.button(tr("Kein Ordner")).clicked() {
            acts.push(Act::Move(vec![d.id.clone()], None));
            ui.close();
        }
        for f in &s.storage.lib.folders {
            if ui.button(&f.name).clicked() {
                acts.push(Act::Move(vec![d.id.clone()], Some(f.id.clone())));
                ui.close();
            }
        }
    });
    if ui.button(format!("💾  {}", tr("Speichern unter …"))).clicked() {
        acts.push(Act::Export(d.id.clone()));
        ui.close();
    }
    ui.separator();
    if ui.button(format!("☑  {}", tr("Auswählen"))).clicked() {
        acts.push(Act::Open(d.id.clone())); // toggles selection in select mode
        ui.close();
    }
    if ui.button(RichText::new(format!("🗑  {}", tr("Löschen"))).color(s.pal.danger)).clicked() {
        acts.push(Act::Delete(vec![d.id.clone()]));
        ui.close();
    }
}

fn doc_card(s: &mut Studio, ui: &mut egui::Ui, d: &DocMeta, w: f32, pal: &Palette, acts: &mut Vec<Act>) {
    let ctx = ui.ctx().clone();
    let tex = {
        let storage = &s.storage;
        s.library.thumb(&ctx, storage, d)
    };
    let h = w * 1.32 + 52.0;
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(w, h), Sense::click());
    let sel = s.library.selected.contains(&d.id);
    let cover = egui::Rect::from_min_size(rect.min, Vec2::new(w, w * 1.32));
    let painter = ui.painter_at(rect.expand(4.0));
    painter.rect_filled(cover, CornerRadius::same(10), pal.panel);
    if let Some(t) = &tex {
        let sz = t.size_vec2();
        let k = (cover.width() / sz.x).min(cover.height() / sz.y);
        let r = egui::Rect::from_center_size(cover.center(), sz * k);
        painter.image(t.id(), r, egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), Color32::WHITE);
    } else {
        painter.text(
            cover.center(),
            egui::Align2::CENTER_CENTER,
            if d.encrypted { "🔒" } else { "📄" },
            egui::FontId::proportional(36.0),
            pal.muted,
        );
    }
    let stroke = if sel || resp.hovered() { Stroke::new(2.0, pal.accent) } else { Stroke::new(1.0, pal.line) };
    painter.rect_stroke(cover, CornerRadius::same(10), stroke, egui::StrokeKind::Inside);
    if d.starred {
        painter.text(
            cover.right_top() + egui::vec2(-10.0, 8.0),
            egui::Align2::RIGHT_TOP,
            "★",
            egui::FontId::proportional(18.0),
            Color32::from_rgb(246, 201, 69),
        );
    }
    if s.library.select_mode {
        painter.text(
            cover.left_top() + egui::vec2(8.0, 6.0),
            egui::Align2::LEFT_TOP,
            if sel { "☑" } else { "☐" },
            egui::FontId::proportional(20.0),
            pal.accent,
        );
    }
    let galley = painter.layout(d.name.clone(), egui::FontId::proportional(13.5), pal.text, w);
    let rows = galley.rows.len();
    let galley = if rows > 2 {
        let mut job = egui::text::LayoutJob::simple(d.name.clone(), egui::FontId::proportional(13.5), pal.text, w);
        job.wrap.max_rows = 2;
        job.wrap.break_anywhere = false;
        ctx.fonts_mut(|f| f.layout_job(job))
    } else {
        galley
    };
    painter.galley(egui::pos2(rect.min.x, cover.max.y + 6.0), galley, pal.text);
    painter.text(
        egui::pos2(rect.min.x, rect.max.y - 2.0),
        egui::Align2::LEFT_BOTTOM,
        format!("{} {} · {}", d.pages, tr("S."), human_size(d.size)),
        egui::FontId::proportional(11.5),
        pal.muted,
    );
    if resp.clicked() {
        acts.push(Act::Open(d.id.clone()));
    }
    if resp.long_touched() {
        s.library.select_mode = true;
        s.library.selected.insert(d.id.clone());
    }
    resp.context_menu(|ui| doc_menu(ui, s, d, acts));
}

fn doc_row(s: &mut Studio, ui: &mut egui::Ui, d: &DocMeta, pal: &Palette, acts: &mut Vec<Act>) {
    let sel = s.library.selected.contains(&d.id);
    let r = ui.add_sized(
        [ui.available_width(), 48.0],
        egui::Button::selectable(
            sel,
            format!(
                "{}{}  {}    {} {} · {}",
                if s.library.select_mode { if sel { "☑ " } else { "☐ " } } else { "" },
                if d.encrypted { "🔒" } else { "📄" },
                d.name,
                d.pages,
                tr("S."),
                human_size(d.size)
            ),
        )
        .wrap_mode(egui::TextWrapMode::Truncate),
    );
    let _ = pal;
    if r.clicked() {
        acts.push(Act::Open(d.id.clone()));
    }
    if r.long_touched() {
        s.library.select_mode = true;
        s.library.selected.insert(d.id.clone());
    }
    r.context_menu(|ui| doc_menu(ui, s, d, acts));
}

fn dialogs(s: &mut Studio, ctx: &egui::Context, acts: &mut Vec<Act>) {
    let pal = s.pal;
    let _ = acts;
    if let Some((id, name)) = &mut s.library.rename {
        let mut done = None;
        egui::Modal::new(egui::Id::unique("rename")).show(ctx, |ui| {
            ui.heading(tr("Umbenennen"));
            let r = ui.add(egui::TextEdit::singleline(name).desired_width(280.0));
            r.request_focus();
            ui.horizontal(|ui| {
                if theme::primary(ui, &pal, tr("OK")).clicked() || ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    done = Some(true);
                }
                if ui.button(tr("Abbrechen")).clicked() {
                    done = Some(false);
                }
            });
        });
        if let Some(ok) = done {
            if ok && !name.trim().is_empty() {
                let (id, name) = (id.clone(), name.trim().to_string());
                if let Some(d) = s.storage.lib.docs.iter_mut().find(|d| d.id == id) {
                    d.name = name;
                }
                s.storage.save();
            }
            s.library.rename = None;
        }
    }
    if let Some(name) = &mut s.library.new_folder {
        let mut done = None;
        egui::Modal::new(egui::Id::unique("newfolder")).show(ctx, |ui| {
            ui.heading(tr("Neuer Ordner"));
            ui.add(egui::TextEdit::singleline(name).hint_text(tr("Name")).desired_width(280.0)).request_focus();
            ui.horizontal(|ui| {
                if theme::primary(ui, &pal, tr("Anlegen")).clicked() || ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    done = Some(true);
                }
                if ui.button(tr("Abbrechen")).clicked() {
                    done = Some(false);
                }
            });
        });
        if let Some(ok) = done {
            if ok && !name.trim().is_empty() {
                let n = s.storage.lib.folders.len();
                s.storage.lib.folders.push(Folder {
                    id: lightpdf_core::model::uid(),
                    name: name.trim().to_string(),
                    color: FOLDER_COLORS[n % FOLDER_COLORS.len()].into(),
                    created_at: lightpdf_core::model::now_ms(),
                });
                s.storage.save();
            }
            s.library.new_folder = None;
        }
    }
    if let Some(ids) = s.library.confirm_delete.clone() {
        let mut done = None;
        egui::Modal::new(egui::Id::unique("confirmdel")).show(ctx, |ui| {
            ui.heading(if ids.len() == 1 { tr("Dokument löschen?") } else { trf("{0} Dokumente löschen?", &[&ids.len()]) });
            ui.label(RichText::new(tr("Die Originaldatei auf dem Gerät bleibt erhalten.")).color(pal.muted));
            ui.horizontal(|ui| {
                if theme::danger(ui, &pal, tr("Löschen")).clicked() {
                    done = Some(true);
                }
                if ui.button(tr("Abbrechen")).clicked() {
                    done = Some(false);
                }
            });
        });
        if let Some(ok) = done {
            if ok {
                for id in &ids {
                    s.storage.delete(id);
                    s.library.forget_thumb(id);
                    s.library.selected.remove(id);
                }
                s.library.select_mode = false;
            }
            s.library.confirm_delete = None;
        }
    }
}
