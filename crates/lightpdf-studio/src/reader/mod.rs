//! The document reader / editor.
//!
//! * `mod.rs`    – state, document handling (save, undo, page operations), render cache
//! * `canvas.rs` – layout, zoom/scroll, all pointer & touch input (through the gesture arbiter)
//! * `paint.rs`  – drawing pages, comments, selection
//! * `chrome.rs` – toolbars, side panel, search, selection bar

mod canvas;
mod chrome;
mod paint;

use crate::app::{BrowsePurpose, Screen, Studio};
use crate::dialogs::Dialog;
use crate::i18n::{tr, trf};
use crate::platform::IncomingFile;
use crate::storage::{DocMeta, Reading, ScrollMode, Storage};
use eframe::egui::{self, Rect, TextureHandle, Vec2};
use lightpdf_core::gesture::{Config, GestureArbiter, TapTracker};
use lightpdf_core::model::{Annotation, ImageKind, Kind, now_ms, uid};
use lightpdf_core::ops::{self, Link, OutlineItem};
use lightpdf_core::render::{Job, Output, PageInfo, PageText, RenderPool};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Tool {
    Hand,
    Select,
    Highlight,
    Underline,
    Strike,
    Ink,
    Highlighter,
    Rect,
    Ellipse,
    Line,
    Arrow,
    Text,
    Note,
    Eraser,
    Redact,
    Signature,
    Image,
    Stamp,
}

impl Tool {
    pub fn is_markup(self) -> bool {
        matches!(self, Tool::Highlight | Tool::Underline | Tool::Strike)
    }
    /// Tools that place an object with a tap.
    pub fn is_tap(self) -> bool {
        matches!(self, Tool::Text | Tool::Note | Tool::Signature | Tool::Image | Tool::Stamp)
    }
    pub fn label(self) -> String {
        tr(match self {
            Tool::Hand => "Hand",
            Tool::Select => "Auswählen",
            Tool::Highlight => "Markieren",
            Tool::Underline => "Unterstreichen",
            Tool::Strike => "Durchstreichen",
            Tool::Ink => "Stift",
            Tool::Highlighter => "Textmarker",
            Tool::Rect => "Rechteck",
            Tool::Ellipse => "Ellipse",
            Tool::Line => "Linie",
            Tool::Arrow => "Pfeil",
            Tool::Text => "Textfeld",
            Tool::Note => "Notiz",
            Tool::Eraser => "Radierer",
            Tool::Redact => "Schwärzen",
            Tool::Signature => "Unterschrift",
            Tool::Image => "Bild",
            Tool::Stamp => "Stempel",
        })
    }
    pub fn icon(self) -> &'static str {
        match self {
            Tool::Hand => "✋",
            Tool::Select => "⬉",
            Tool::Highlight => crate::icons::HIGHLIGHT,
            Tool::Underline => crate::icons::UNDERLINE,
            Tool::Strike => crate::icons::STRIKE,
            Tool::Ink => "✏",
            Tool::Highlighter => crate::icons::BRUSH,
            Tool::Rect => "⬜",
            Tool::Ellipse => "⭕",
            Tool::Line => "╱",
            Tool::Arrow => "➚",
            Tool::Text => crate::icons::TEXT,
            Tool::Note => "🗨",
            Tool::Eraser => "⌫",
            Tool::Redact => "⬛",
            Tool::Signature => "✍",
            Tool::Image => "🖼",
            Tool::Stamp => "🔖",
        }
    }
    fn default_color(self) -> &'static str {
        match self {
            Tool::Highlight | Tool::Highlighter => "#f6c945",
            Tool::Underline => "#2f6bd6",
            Tool::Strike => "#d64545",
            Tool::Note => "#f6c945",
            Tool::Text => "#1f1f1f",
            _ => "#d64545",
        }
    }
}

#[derive(Clone)]
pub struct PendingImage {
    pub src: Arc<str>,
    pub kind: ImageKind,
    pub aspect: f64,
}

#[derive(Clone)]
pub struct ToolOpts {
    pub colors: HashMap<Tool, String>,
    pub width: f64,
    pub opacity: f64,
    pub font_size: f64,
    pub fill: Option<String>,
    pub preset_text: Option<String>,
    pub pending_image: Option<PendingImage>,
}

impl ToolOpts {
    pub fn color(&self, t: Tool) -> String {
        self.colors.get(&t).cloned().unwrap_or_else(|| t.default_color().to_string())
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Read,
    Comment,
    Fill,
    Pages,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Thumbs,
    Outline,
    Comments,
}

#[derive(Clone)]
struct Snapshot {
    annots: Vec<Annotation>,
    base: Option<Arc<Vec<u8>>>,
}

pub struct PageTex {
    pub tex: TextureHandle,
    pub scale: f64,
    pub rotation: i32,
}

pub struct Detail {
    pub page: usize,
    pub region: lightpdf_core::model::Rect,
    pub scale: f64,
    pub rotation: i32,
    pub tex: TextureHandle,
}

#[derive(Default)]
pub struct Search {
    pub open: bool,
    pub query: String,
    pub active_query: String,
    /// (page, rects in PDF space)
    pub hits: Vec<(usize, Vec<lightpdf_core::model::Rect>)>,
    pub current: usize,
    pub scanned: HashSet<usize>,
    pub focus: bool,
}

/// Work in progress of the current gesture.
pub enum Draw {
    Draft { page: usize, ann: Annotation, start: [f64; 2] },
    Markup { page: usize, start: Option<usize>, start_pdf: [f64; 2], ann: Annotation },
    Move { id: String, orig: Annotation, start: [f64; 2], moved: bool },
    Resize { id: String, orig: Annotation, handle: canvas::Handle },
    Erase { before: Vec<Annotation>, erased: bool },
}

pub const THUMB_TAG: u64 = u64::MAX;

pub struct Reader {
    pub meta: DocMeta,
    pub base: Arc<Vec<u8>>,
    pub password: Option<String>,
    pub encrypted: bool,
    pub annots: Vec<Annotation>,
    undo: Vec<Snapshot>,
    redo: Vec<Snapshot>,
    pub dirty: bool,
    pool: Option<RenderPool>,
    pub pages: Vec<PageInfo>,
    pub links: Vec<Link>,
    pub outline: Vec<OutlineItem>,
    pub has_form: bool,

    // view
    pub zoom: f32,
    /// The user zoomed by hand: don't re-fit on window size changes.
    pub user_zoomed: bool,
    pub offset: Vec2,
    pub view_rot: i32,
    pub canvas: Rect,
    layout: Vec<Rect>,
    content: Vec2,
    layout_key: Option<(ScrollMode, bool, i32, usize)>,
    pending_fit: Option<canvas::Fit>,
    pending_goto: Option<usize>,
    textures: HashMap<usize, PageTex>,
    detail: Option<Detail>,
    pub thumbs: HashMap<usize, TextureHandle>,
    thumb_req: HashSet<usize>,
    requested: HashMap<usize, (f64, i32)>,
    detail_req: Option<(usize, lightpdf_core::model::Rect, f64, i32)>,
    tag: u64,
    last_zoom_change: f64,
    reading_applied: Reading,
    pub text: HashMap<usize, Arc<PageText>>,
    text_req: HashSet<usize>,
    image_tex: HashMap<String, TextureHandle>,

    // input
    arbiter: GestureArbiter,
    taps: TapTracker,
    last_touch: f64,
    mouse_down: bool,
    draw: Option<Draw>,
    fling: Vec2,
    last_frame: f64,

    // ui
    pub tool: Tool,
    pub opts: ToolOpts,
    pub tab: Tab,
    pub selected: Option<String>,
    pub editing: Option<String>,
    pub edit_buf: String,
    pub search: Search,
    pub side: Option<Side>,
    pub dialog: Option<Dialog>,
    pub current_page: usize,
    pub close_requested: bool,
    pub closed: bool,
    pub fullscreen: bool,
    pub show_colors: bool,
    pub organizer: Option<crate::organizer::Organizer>,
    author: String,
    toast_hint: Option<String>,
    want_image: bool,
    pending_insert: Option<Vec<u8>>,
}

impl Reader {
    pub fn new(
        ctx: &egui::Context,
        meta: DocMeta,
        imp: ops::Imported,
        password: Option<String>,
        storage: &Storage,
    ) -> Result<Self, String> {
        let settings = &storage.lib.settings;
        let mut r = Self {
            current_page: meta.last_page.min(meta.pages.saturating_sub(1)),
            pending_goto: Some(meta.last_page),
            meta,
            base: Arc::new(imp.base),
            password,
            encrypted: imp.encrypted,
            annots: imp.annots,
            undo: Vec::new(),
            redo: Vec::new(),
            dirty: false,
            pool: None,
            pages: Vec::new(),
            links: Vec::new(),
            outline: Vec::new(),
            has_form: false,
            zoom: 1.0,
            user_zoomed: false,
            offset: Vec2::ZERO,
            view_rot: 0,
            canvas: Rect::NOTHING,
            layout: Vec::new(),
            content: Vec2::ZERO,
            layout_key: None,
            pending_fit: Some(canvas::Fit::Auto),
            textures: HashMap::new(),
            detail: None,
            thumbs: HashMap::new(),
            thumb_req: HashSet::new(),
            requested: HashMap::new(),
            detail_req: None,
            tag: 1,
            last_zoom_change: 0.0,
            reading_applied: settings.reading,
            text: HashMap::new(),
            text_req: HashSet::new(),
            image_tex: HashMap::new(),
            arbiter: GestureArbiter::new(Config { pen_only: settings.pen_only, ..Config::default() }),
            taps: TapTracker::default(),
            last_touch: -10.0,
            mouse_down: false,
            draw: None,
            fling: Vec2::ZERO,
            last_frame: 0.0,
            tool: Tool::Hand,
            opts: ToolOpts {
                colors: HashMap::new(),
                width: 2.0,
                opacity: 1.0,
                font_size: 14.0,
                fill: None,
                preset_text: None,
                pending_image: None,
            },
            tab: Tab::Read,
            selected: None,
            editing: None,
            edit_buf: String::new(),
            search: Search::default(),
            side: None,
            dialog: None,
            close_requested: false,
            closed: false,
            fullscreen: false,
            show_colors: false,
            organizer: None,
            author: settings.author.clone(),
            toast_hint: None,
            want_image: false,
            pending_insert: None,
        };
        r.load_document(ctx)?;
        Ok(r)
    }

    pub fn page_count(&self) -> usize {
        self.pages.len()
    }

    /// (Re)creates the renderer for the current `base`.
    fn load_document(&mut self, ctx: &egui::Context) -> Result<(), String> {
        let display = ops::display_copy(&self.base).unwrap_or_else(|_| self.base.to_vec());
        let ctx2 = ctx.clone();
        let pool = RenderPool::new(Arc::new(display), None, Arc::new(move || ctx2.request_repaint())).map_err(|e| e.to_string())?;
        self.pages = pool.pages.clone();
        self.pool = Some(pool);
        self.links = ops::links(&self.base);
        self.outline = ops::outline(&self.base);
        self.has_form = ops::has_form(&self.base);
        self.textures.clear();
        self.thumbs.clear();
        self.thumb_req.clear();
        self.requested.clear();
        self.detail = None;
        self.detail_req = None;
        self.text.clear();
        self.text_req.clear();
        self.layout_key = None;
        self.search.scanned.clear();
        self.search.hits.clear();
        self.search.active_query.clear();
        self.current_page = self.current_page.min(self.pages.len().saturating_sub(1));
        Ok(())
    }

    pub fn set_pen_only(&mut self, on: bool) {
        self.arbiter.config.pen_only = on;
    }

    pub fn persist_position(&mut self, storage: &mut Storage) {
        if let Some(m) = storage.lib.docs.iter_mut().find(|d| d.id == self.meta.id) {
            m.last_page = self.current_page;
            m.pages = self.pages.len();
        }
        storage.save();
    }

    pub fn request_close(&mut self) {
        self.close_requested = true;
    }

    // ------------------------------------------------------------------ comments & undo

    fn snapshot(&self, with_base: bool) -> Snapshot {
        Snapshot { annots: self.annots.clone(), base: with_base.then(|| self.base.clone()) }
    }

    /// Call before every change.
    pub fn checkpoint(&mut self) {
        let s = self.snapshot(false);
        self.undo.push(s);
        if self.undo.len() > 200 {
            self.undo.remove(0);
        }
        self.redo.clear();
        self.dirty = true;
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }
    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    fn restore(&mut self, ctx: &egui::Context, s: Snapshot) -> Snapshot {
        let cur = self.snapshot(s.base.is_some());
        self.annots = s.annots;
        if let Some(b) = s.base {
            self.base = b;
            let _ = self.load_document(ctx);
        }
        self.selected = None;
        self.editing = None;
        self.dirty = true;
        cur
    }

    pub fn undo(&mut self, ctx: &egui::Context) {
        if let Some(s) = self.undo.pop() {
            let cur = self.restore(ctx, s);
            self.redo.push(cur);
        }
    }

    pub fn redo(&mut self, ctx: &egui::Context) {
        if let Some(s) = self.redo.pop() {
            let cur = self.restore(ctx, s);
            self.undo.push(cur);
        }
    }

    pub fn new_annot(&self, page: usize, color: String, opacity: f64, kind: Kind) -> Annotation {
        Annotation {
            id: uid(),
            page: page as u32 + 1,
            color,
            opacity,
            created_at: now_ms(),
            author: (!self.author.is_empty()).then(|| self.author.clone()),
            note: None,
            kind,
        }
    }

    pub fn add(&mut self, a: Annotation) {
        self.checkpoint();
        self.annots.push(a);
    }

    pub fn find(&self, id: &str) -> Option<&Annotation> {
        self.annots.iter().find(|a| a.id == id)
    }

    /// Replace without checkpoint (live dragging); call `checkpoint` first once.
    pub fn replace(&mut self, a: Annotation) {
        if let Some(x) = self.annots.iter_mut().find(|x| x.id == a.id) {
            *x = a;
        }
    }

    pub fn update(&mut self, a: Annotation) {
        self.checkpoint();
        self.replace(a);
    }

    pub fn delete(&mut self, id: &str) {
        if self.find(id).is_some() {
            self.checkpoint();
            self.annots.retain(|a| a.id != id);
        }
        if self.selected.as_deref() == Some(id) {
            self.selected = None;
        }
        if self.editing.as_deref() == Some(id) {
            self.editing = None;
        }
    }

    pub fn duplicate(&mut self, id: &str) {
        let Some(mut a) = self.find(id).cloned() else { return };
        a.id = uid();
        a.created_at = now_ms();
        let g = self.pages[a.page as usize - 1].geom(self.view_rot);
        let d = g.vec_to_pdf([12.0, 12.0]);
        a.translate(d[0], d[1]);
        self.selected = Some(a.id.clone());
        self.add(a);
    }

    // ------------------------------------------------------------------ document changes

    /// Replaces the underlying PDF (page operations, redaction, forms) – undoable.
    pub fn set_base(&mut self, ctx: &egui::Context, base: Vec<u8>, annots: Vec<Annotation>) {
        let s = self.snapshot(true);
        self.undo.push(s);
        self.redo.clear();
        self.base = Arc::new(base);
        self.annots = annots;
        self.dirty = true;
        self.selected = None;
        self.editing = None;
        if let Err(e) = self.load_document(ctx) {
            log::error!("reload: {e}");
        }
    }

    /// Runs a page operation on the document *with* its comments, so they move with
    /// their pages (deleted, reordered, duplicated …).
    pub fn page_op(
        &self,
        s: &mut crate::tasks::Tasks,
        ctx: &egui::Context,
        label: String,
        op: impl FnOnce(&[u8]) -> Result<Vec<u8>, lightpdf_core::pdfdoc::Error> + Send + 'static,
    ) {
        let base = self.base.clone();
        let annots = self.annots.clone();
        s.spawn(ctx, label, move |_| {
            let r = (|| {
                let baked = ops::export_pdf(&base, &annots, &ops::ExportOpts::default())?;
                let out = op(&baked)?;
                let mut imp = ops::import_pdf(&out, None)?;
                // duplicated pages carry copies of the same comments: give them new ids
                let mut seen = HashSet::new();
                for a in &mut imp.annots {
                    if !seen.insert(a.id.clone()) {
                        a.id = uid();
                    }
                }
                Ok::<_, lightpdf_core::pdfdoc::Error>(imp)
            })();
            Box::new(move |s: &mut Studio| match r {
                Ok(imp) => {
                    let ctx = s.ctx.clone();
                    if let Screen::Reader(rd) = &mut s.screen {
                        rd.set_base(&ctx, imp.base, imp.annots);
                    }
                }
                Err(e) => s.toast_err(e.to_string()),
            })
        });
    }

    /// The final PDF bytes (comments as annotations, password re-applied).
    pub fn export_bytes(&self, flatten: bool) -> Result<Vec<u8>, String> {
        let pw = if self.encrypted { self.password.clone() } else { None };
        ops::export_pdf(&self.base, &self.annots, &ops::ExportOpts { password: pw, flatten }).map_err(|e| e.to_string())
    }

    pub fn save(&mut self, s: &mut Studio, then_close: bool) {
        let base = self.base.clone();
        let annots = self.annots.clone();
        let pw = if self.encrypted { self.password.clone() } else { None };
        let meta = self.meta.clone();
        let ctx = s.ctx.clone();
        s.tasks.spawn(&ctx, tr("Speichere …"), move |_| {
            let res = ops::export_pdf(&base, &annots, &ops::ExportOpts { password: pw, flatten: false });
            Box::new(move |s: &mut Studio| match res {
                Ok(bytes) => {
                    if let Err(e) = s.storage.write_doc(&meta.id, &bytes) {
                        s.toast_err(e.to_string());
                        return;
                    }
                    s.library.forget_thumb(&meta.id);
                    let mut msg = tr("Gespeichert");
                    if let Some(origin) = &meta.path
                        && let Err(e) = crate::platform::write_back(origin, &bytes)
                    {
                        msg = trf("In der Bibliothek gespeichert – Originaldatei nicht beschreibbar: {0}", &[&e]);
                    }
                    let mut close = false;
                    if let Screen::Reader(r) = &mut s.screen
                        && r.meta.id == meta.id
                    {
                        r.dirty = false;
                        close = then_close;
                    }
                    s.toast(msg);
                    if close {
                        s.close_reader();
                    }
                }
                Err(e) => s.toast_err(e.to_string()),
            })
        });
    }

    pub fn save_as(&mut self, s: &mut Studio, flatten: bool) {
        self.commit_editing();
        let name = format!("{}.pdf", self.meta.name);
        match self.export_bytes(flatten) {
            Ok(bytes) => match crate::platform::save_as(&name, &bytes, "pdf") {
                Ok(Some(path)) => {
                    if !flatten {
                        self.dirty = false;
                        if crate::platform::native_dialogs() {
                            self.meta.path = Some(path);
                            let m = self.meta.clone();
                            let _ = s.storage.write_doc(&m.id, &bytes);
                            s.storage.update(&m);
                        }
                    }
                    s.toast(if crate::platform::native_dialogs() { tr("Gespeichert") } else { tr("In Downloads exportiert") });
                }
                Ok(None) => {}
                Err(e) => s.toast_err(e),
            },
            Err(e) => s.toast_err(e),
        }
    }

    /// Images / PDFs chosen in a file dialog for this document.
    pub fn files_picked(&mut self, purpose: BrowsePurpose, files: Vec<IncomingFile>) {
        match purpose {
            BrowsePurpose::Images => {
                if let Some(f) = files.into_iter().next()
                    && let Ok(img) = image::load_from_memory(&f.bytes)
                {
                    let mut png = std::io::Cursor::new(Vec::new());
                    // keep JPEG photos small, PNG for transparency
                    let rgba = img.to_rgba8();
                    let (w, h) = (rgba.width(), rgba.height());
                    let scale = (1600.0 / w.max(h) as f32).min(1.0);
                    let rgba = if scale < 1.0 {
                        image::imageops::resize(
                            &rgba,
                            (w as f32 * scale) as u32,
                            (h as f32 * scale) as u32,
                            image::imageops::FilterType::Triangle,
                        )
                    } else {
                        rgba
                    };
                    let _ = rgba.write_to(&mut png, image::ImageFormat::Png);
                    let src: Arc<str> = lightpdf_core::annotio::to_data_url(png.get_ref()).into();
                    self.opts.pending_image = Some(PendingImage { src, kind: ImageKind::Image, aspect: w as f64 / h.max(1) as f64 });
                    self.tool = Tool::Image;
                }
            }
            BrowsePurpose::InsertPdf => {
                if let Some(f) = files.into_iter().next() {
                    self.pending_insert = Some(f.bytes);
                }
            }
            _ => {}
        }
    }

    // ------------------------------------------------------------------ rendering

    fn pool(&self) -> Option<&RenderPool> {
        self.pool.as_ref()
    }

    pub fn request_text(&mut self, page: usize) {
        if self.text.contains_key(&page) || !self.text_req.insert(page) {
            return;
        }
        if let Some(p) = self.pool() {
            p.submit(Job::Text { page });
        }
    }

    fn upload(&self, ctx: &egui::Context, name: String, bmp: &lightpdf_core::render::Bitmap, reading: Reading) -> TextureHandle {
        let mut rgba = bmp.rgba.clone();
        if reading == Reading::Night {
            for p in rgba.chunks_mut(4) {
                // invert lightness, keep it a little warm
                let (r, g, b) = (255 - p[0], 255 - p[1], 255 - p[2]);
                p[0] = (r as f32 * 0.92 + 14.0) as u8;
                p[1] = (g as f32 * 0.90 + 14.0) as u8;
                p[2] = (b as f32 * 0.86 + 12.0) as u8;
            }
        }
        let img = egui::ColorImage::from_rgba_unmultiplied([bmp.w as usize, bmp.h as usize], &rgba);
        ctx.load_texture(name, img, egui::TextureOptions::LINEAR)
    }

    /// Takes finished renders from the pool.
    fn poll_renders(&mut self, ctx: &egui::Context, reading: Reading) {
        let Some(pool) = &self.pool else { return };
        let outs: Vec<Output> = pool.results.try_iter().collect();
        for o in outs {
            match o {
                Output::Rendered { page, scale, rotation, region, tag, bitmap } => {
                    if tag == THUMB_TAG {
                        let t = self.upload(ctx, format!("th{page}"), &bitmap, Reading::Normal);
                        self.thumbs.insert(page, t);
                    } else if let Some(region) = region {
                        if rotation == self.view_rot {
                            let tex = self.upload(ctx, format!("detail{page}"), &bitmap, reading);
                            self.detail = Some(Detail { page, region, scale, rotation, tex });
                        }
                    } else if rotation == self.view_rot {
                        let tex = self.upload(ctx, format!("p{page}"), &bitmap, reading);
                        self.textures.insert(page, PageTex { tex, scale, rotation });
                    }
                }
                Output::Text { page, text } => {
                    self.text_req.remove(&page);
                    self.text.insert(page, Arc::new(text));
                }
            }
        }
    }

    pub fn request_thumb(&mut self, page: usize, width_px: f32) {
        if self.thumbs.contains_key(&page) || !self.thumb_req.insert(page) {
            return;
        }
        let Some(info) = self.pages.get(page) else { return };
        let g = info.geom(0);
        let scale = width_px as f64 / g.width().max(1.0);
        if let Some(p) = self.pool() {
            p.submit(Job::Render { page, scale, rotation: 0, region: None, tag: THUMB_TAG });
        }
    }

    // ------------------------------------------------------------------ search

    pub fn run_search(&mut self) {
        let q = self.search.query.trim().to_string();
        if q != self.search.active_query {
            self.search.active_query = q.clone();
            self.search.hits.clear();
            self.search.scanned.clear();
            self.search.current = 0;
        }
        if q.is_empty() {
            return;
        }
        for page in 0..self.pages.len() {
            if self.search.scanned.contains(&page) {
                continue;
            }
            match self.text.get(&page).cloned() {
                Some(t) => {
                    self.search.scanned.insert(page);
                    for (a, b) in t.find(&q) {
                        self.search.hits.push((page, t.line_rects(a, b)));
                    }
                    self.search.hits.sort_by_key(|h| h.0);
                }
                None => self.request_text(page),
            }
        }
    }

    pub fn search_step(&mut self, dir: i32) {
        let n = self.search.hits.len();
        if n == 0 {
            return;
        }
        self.search.current = ((self.search.current as i32 + dir).rem_euclid(n as i32)) as usize;
        let (page, rects) = self.search.hits[self.search.current].clone();
        if let Some(r) = rects.first() {
            self.scroll_to_pdf_rect(page, *r);
        }
    }

    // ------------------------------------------------------------------ misc

    pub fn image_texture(&mut self, ctx: &egui::Context, a: &Annotation) -> Option<TextureHandle> {
        let Kind::Image { src, .. } = &a.kind else { return None };
        let key = format!("{}:{}", a.id, src.len());
        if let Some(t) = self.image_tex.get(&key) {
            return Some(t.clone());
        }
        let bytes = lightpdf_core::annotio::data_url_bytes(src)?;
        let img = lightpdf_core::annotio::decode_image(&bytes)?;
        let t = ctx.load_texture(
            key.clone(),
            egui::ColorImage::from_rgba_unmultiplied([img.w as usize, img.h as usize], &img.rgba),
            egui::TextureOptions::LINEAR,
        );
        self.image_tex.insert(key, t.clone());
        Some(t)
    }

    pub fn copy_text(&self, ctx: &egui::Context, text: &str) {
        ctx.copy_text(text.to_string());
    }
}

/// Insert-PDF payload waiting for the user to pick a position (see organizer).
impl Reader {
    pub fn take_pending_insert(&mut self) -> Option<Vec<u8>> {
        self.pending_insert.take()
    }
}

/// Entry from the app: shows the reader screen.
pub fn show(s: &mut Studio, ui: &mut egui::Ui) {
    let Screen::Reader(mut r) = std::mem::replace(&mut s.screen, Screen::Library) else { return };
    r.ui(s, ui);
    // the reader may have been replaced or closed during the frame
    if !r.closed && matches!(s.screen, Screen::Library) {
        s.screen = Screen::Reader(r);
    } else if !r.closed {
        // another document was opened: keep the new one
    } else {
        r.persist_position(&mut s.storage);
    }
}

pub fn chrome_stamps() -> &'static [(&'static str, &'static str)] {
    &chrome::STAMPS
}

pub fn today_str() -> String {
    chrome::today()
}
