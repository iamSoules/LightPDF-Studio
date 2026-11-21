//! Page rendering and text extraction with `hayro` (pure Rust), on background threads.

use crate::geom::PageGeom;
use crate::model::Rect;
pub use hayro::RenderCache;
use hayro::hayro_interpret::font::Glyph as HGlyph;
use hayro::hayro_interpret::hayro_cmap::BfString;
use hayro::hayro_interpret::hayro_syntax::Pdf;
use hayro::hayro_interpret::hayro_syntax::page::{Page, Rotation};
use hayro::hayro_interpret::{
    BlendMode, ClipPath, Context, Device, DrawMode, DrawProps, Image, ImageDrawProps, InterpreterCache, InterpreterSettings, SoftMask,
    interpret_page,
};
use hayro::kurbo::{Affine, BezPath, Point};
use hayro::vello_cpu::color::palette::css::WHITE;
use hayro::vello_cpu::{Pixmap, RasterizerSettings, RenderContext, Resources, TargetInit};
use hayro::{RenderSettings, render_into};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PageInfo {
    /// Visible box in user space.
    pub view_box: Rect,
    /// /Rotate of the page.
    pub rotate: i32,
}

impl PageInfo {
    pub fn geom(&self, view_rotation: i32) -> PageGeom {
        PageGeom::new(self.view_box, self.rotate + view_rotation)
    }
}

fn rot_deg(r: Rotation) -> i32 {
    match r {
        Rotation::None => 0,
        Rotation::Horizontal => 90,
        Rotation::Flipped => 180,
        Rotation::FlippedHorizontal => 270,
    }
}

fn page_info(p: &Page<'_>) -> PageInfo {
    let b = p.intersected_crop_box();
    let mut vb = [b.x0.min(b.x1), b.y0.min(b.y1), b.x0.max(b.x1), b.y0.max(b.y1)];
    if vb[2] - vb[0] < 1.0 || vb[3] - vb[1] < 1.0 {
        let (w, h) = p.base_dimensions();
        vb = [0.0, 0.0, w as f64, h as f64];
    }
    PageInfo { view_box: vb, rotate: rot_deg(p.rotation()) }
}

pub fn open(bytes: Arc<Vec<u8>>, password: Option<&str>) -> Result<Pdf, crate::pdfdoc::Error> {
    let r = match password {
        Some(pw) => Pdf::new_with_password(bytes, pw),
        None => Pdf::new(bytes),
    };
    r.map_err(|e| match e {
        hayro::hayro_interpret::hayro_syntax::LoadPdfError::Decryption(_) => {
            crate::pdfdoc::Error::PasswordNeeded { wrong: password.is_some() }
        }
        _ => crate::pdfdoc::Error::Invalid("unreadable PDF".into()),
    })
}

pub fn page_infos(pdf: &Pdf) -> Vec<PageInfo> {
    pdf.pages().iter().map(page_info).collect()
}

/// An RGBA8 image (straight alpha; pages are rendered on white, so alpha is 255).
#[derive(Clone)]
pub struct Bitmap {
    pub w: u32,
    pub h: u32,
    pub rgba: Vec<u8>,
}

/// Renders a page (or a `region` of it, in view points) at `scale` pixels per point.
pub fn render_page<'a>(
    pdf: &'a Pdf,
    cache: &RenderCache<'a>,
    index: usize,
    scale: f64,
    view_rotation: i32,
    region: Option<Rect>,
) -> Option<Bitmap> {
    let page = pdf.pages().get(index)?;
    render_page_obj(page, cache, scale, view_rotation, region)
}

fn render_page_obj<'a>(
    page: &'a Page<'a>,
    cache: &RenderCache<'a>,
    scale: f64,
    view_rotation: i32,
    region: Option<Rect>,
) -> Option<Bitmap> {
    let g = page_info(page).geom(view_rotation);
    let [a, b, c, d, e, f] = g.affine();
    let mut t = Affine::scale(scale) * Affine::new([a, b, c, d, e, f]);
    let (w, h) = match region {
        Some(r) => {
            t = Affine::translate((-r[0] * scale, -r[1] * scale)) * t;
            ((r[2] - r[0]) * scale, (r[3] - r[1]) * scale)
        }
        None => (g.width() * scale, g.height() * scale),
    };
    let w = (w.round() as u32).clamp(1, 8192) as u16;
    let h = (h.round() as u32).clamp(1, 8192) as u16;
    let mut ctx = RenderContext::new(w, h);
    render_into(page, cache, &InterpreterSettings::default(), &RenderSettings::default(), &mut ctx, t);
    ctx.flush();
    let mut pixmap = Pixmap::new(w, h);
    ctx.render_with(
        &mut pixmap,
        &mut Resources::default(),
        RasterizerSettings { target_init: TargetInit::Clear(WHITE), ..Default::default() },
    );
    Some(Bitmap { w: w as u32, h: h as u32, rgba: pixmap.data_as_u8_slice().to_vec() })
}

/// Renders a whole page to RGBA at `dpi` (used for export, redaction, compression).
pub fn render_at_dpi(pdf: &Pdf, index: usize, dpi: f64, view_rotation: i32) -> Option<Bitmap> {
    let cache = RenderCache::new();
    render_page(pdf, &cache, index, dpi / 72.0, view_rotation, None)
}

// ------------------------------------------------------------------ text

#[derive(Clone, Debug, PartialEq)]
pub struct TextGlyph {
    pub text: String,
    /// Bounding box in PDF user space.
    pub rect: Rect,
    /// Baseline start and end (advance) in user space.
    pub origin: [f64; 2],
    pub end: [f64; 2],
    /// Font size in user space.
    pub size: f64,
}

impl TextGlyph {
    fn dir(&self) -> [f64; 2] {
        let (dx, dy) = (self.end[0] - self.origin[0], self.end[1] - self.origin[1]);
        let l = (dx * dx + dy * dy).sqrt().max(1e-9);
        [dx / l, dy / l]
    }
}

#[derive(Clone, Debug, Default)]
pub struct PageText {
    pub glyphs: Vec<TextGlyph>,
}

struct TextDevice {
    glyphs: Vec<TextGlyph>,
}

impl<'a> Device<'a> for TextDevice {
    fn draw_path(&mut self, _: &BezPath, _: DrawProps<'a>, _: &DrawMode) {}
    fn push_clip_path(&mut self, _: &ClipPath) {}
    fn push_transparency_group(&mut self, _: f32, _: Option<SoftMask<'a>>, _: BlendMode) {}
    fn draw_glyph_run(&mut self, run: &hayro::hayro_interpret::font::GlyphRun<'_, 'a>, props: DrawProps<'a>, _: &DrawMode) {
        for g in run.glyphs() {
            let text = match g.as_unicode() {
                Some(BfString::Char(c)) => c.to_string(),
                Some(BfString::String(s)) => s,
                None => continue,
            };
            let adv = match &**g {
                HGlyph::Outline(o) => o.advance_width().unwrap_or(500.0),
                HGlyph::Type3(t) => t.advance_width().unwrap_or(500.0),
            } as f64;
            let adv = if adv <= 0.0 { 500.0 } else { adv };
            let t = props.transform * g.transform();
            let pts = [Point::new(0.0, -210.0), Point::new(adv, -210.0), Point::new(0.0, 790.0), Point::new(adv, 790.0)].map(|p| t * p);
            let r = crate::model::points_bounds(pts.map(|p| [p.x, p.y]));
            let o = t * Point::new(0.0, 0.0);
            let e = t * Point::new(adv, 0.0);
            let up = t * Point::new(0.0, 1000.0);
            let size = ((up.x - o.x).powi(2) + (up.y - o.y).powi(2)).sqrt();
            if r.iter().all(|v| v.is_finite()) && r[2] - r[0] < 2000.0 && size > 0.0 {
                self.glyphs.push(TextGlyph { text, rect: r, origin: [o.x, o.y], end: [e.x, e.y], size });
            }
        }
    }
    fn draw_image(&mut self, _: Image<'a, '_>, _: ImageDrawProps<'a>) {}
    fn pop_clip(&mut self) {}
    fn pop_transparency_group(&mut self) {}
}

pub fn page_text(pdf: &Pdf, index: usize) -> PageText {
    let Some(page) = pdf.pages().get(index) else { return PageText::default() };
    let cache = InterpreterCache::new();
    let info = page_info(page);
    let bbox = hayro::kurbo::Rect::new(info.view_box[0], info.view_box[1], info.view_box[2], info.view_box[3]);
    let mut ctx = Context::new(Affine::IDENTITY, bbox, &cache, page.xref(), InterpreterSettings::default());
    let mut dev = TextDevice { glyphs: Vec::new() };
    interpret_page(page, &mut ctx, &mut dev);
    PageText { glyphs: dev.glyphs }
}

impl PageText {
    /// Same text line: parallel baselines, close perpendicular distance.
    fn same_line(a: &TextGlyph, b: &TextGlyph) -> bool {
        let (da, db) = (a.dir(), b.dir());
        if da[0] * db[0] + da[1] * db[1] < 0.95 {
            return false;
        }
        let v = [b.origin[0] - a.origin[0], b.origin[1] - a.origin[1]];
        let perp = (v[0] * -da[1] + v[1] * da[0]).abs();
        perp < 0.5 * a.size.min(b.size)
    }

    /// Gap along the baseline between the end of `a` and the start of `b`.
    fn gap(a: &TextGlyph, b: &TextGlyph) -> f64 {
        let d = a.dir();
        (b.origin[0] - a.end[0]) * d[0] + (b.origin[1] - a.end[1]) * d[1]
    }

    /// Plain text with spaces / line breaks inferred from glyph positions, plus a map from
    /// each char of the string to its glyph index (`usize::MAX` for inferred chars).
    pub fn string(&self) -> (String, Vec<usize>) {
        let mut s = String::new();
        let mut map = Vec::new();
        let mut prev: Option<&TextGlyph> = None;
        for (i, g) in self.glyphs.iter().enumerate() {
            if let Some(p) = prev {
                let last_ws = s.ends_with(char::is_whitespace);
                let gap = Self::gap(p, g);
                if !Self::same_line(p, g) || gap < -p.size * 2.0 {
                    if !last_ws {
                        s.push('\n');
                        map.push(usize::MAX);
                    }
                } else if gap > 0.15 * p.size && !last_ws && !g.text.starts_with(' ') {
                    s.push(' ');
                    map.push(usize::MAX);
                }
            }
            for c in g.text.chars() {
                s.push(c);
                map.push(i);
            }
            prev = Some(g);
        }
        (s, map)
    }

    /// Merges consecutive glyphs on the same line into rectangles (for highlight markup).
    pub fn line_rects(&self, from: usize, to: usize) -> Vec<Rect> {
        let mut out: Vec<Rect> = Vec::new();
        let mut prev: Option<&TextGlyph> = None;
        for g in self.glyphs.get(from..=to.min(self.glyphs.len().saturating_sub(1))).unwrap_or(&[]) {
            if g.text.trim().is_empty() && out.is_empty() {
                continue;
            }
            let joined = prev.is_some_and(|p| Self::same_line(p, g) && Self::gap(p, g) > -p.size);
            prev = Some(g);
            match out.last_mut() {
                Some(r) if joined => {
                    r[0] = r[0].min(g.rect[0]);
                    r[1] = r[1].min(g.rect[1]);
                    r[2] = r[2].max(g.rect[2]);
                    r[3] = r[3].max(g.rect[3]);
                }
                _ => out.push(g.rect),
            }
        }
        out
    }

    pub fn text_between(&self, from: usize, to: usize) -> String {
        let t = PageText { glyphs: self.glyphs.get(from..=to.min(self.glyphs.len().saturating_sub(1))).unwrap_or(&[]).to_vec() };
        t.string().0.trim().to_string()
    }

    /// Index of the glyph nearest to a PDF-space point (within `max_dist`).
    pub fn nearest(&self, p: [f64; 2], max_dist: f64) -> Option<usize> {
        let mut best = None;
        let mut bd = max_dist;
        for (i, g) in self.glyphs.iter().enumerate() {
            let dx = (g.rect[0] - p[0]).max(p[0] - g.rect[2]).max(0.0);
            let dy = (g.rect[1] - p[1]).max(p[1] - g.rect[3]).max(0.0);
            let d = (dx * dx + dy * dy * 4.0).sqrt();
            if d < bd || (d == 0.0 && bd == 0.0) {
                bd = d;
                best = Some(i);
                if d == 0.0 {
                    break;
                }
            }
        }
        best
    }

    /// Case-insensitive search. Returns glyph-index ranges (inclusive).
    pub fn find(&self, query: &str) -> Vec<(usize, usize)> {
        let q: Vec<char> = query.chars().flat_map(char::to_lowercase).collect();
        if q.is_empty() {
            return vec![];
        }
        let (s, map) = self.string();
        let chars: Vec<(char, usize)> =
            s.chars().zip(map).map(|(c, m)| (if c == '\n' { ' ' } else { c.to_lowercase().next().unwrap_or(c) }, m)).collect();
        let mut out = Vec::new();
        let mut i = 0;
        while i + q.len() <= chars.len() {
            if chars[i..i + q.len()].iter().map(|c| c.0).eq(q.iter().copied()) {
                let gs: Vec<usize> = chars[i..i + q.len()].iter().map(|c| c.1).filter(|&m| m != usize::MAX).collect();
                if let (Some(a), Some(b)) = (gs.first(), gs.last()) {
                    out.push((*a, *b));
                }
                i += q.len();
            } else {
                i += 1;
            }
        }
        out
    }
}

// ------------------------------------------------------------------ worker pool

#[derive(Clone, Debug)]
pub enum Job {
    Render { page: usize, scale: f64, rotation: i32, region: Option<Rect>, tag: u64 },
    Text { page: usize },
}

pub enum Output {
    Rendered { page: usize, scale: f64, rotation: i32, region: Option<Rect>, tag: u64, bitmap: Bitmap },
    Text { page: usize, text: PageText },
}

struct Shared {
    queue: Mutex<(VecDeque<Job>, bool)>,
    cv: Condvar,
    min_tag: AtomicU64,
}

/// Background renderer: a few threads, each with its own parsed copy of the document.
pub struct RenderPool {
    shared: Arc<Shared>,
    pub results: std::sync::mpsc::Receiver<Output>,
    pub pages: Vec<PageInfo>,
}

impl RenderPool {
    /// `wake` is called (from a worker thread) whenever a result is ready.
    pub fn new(bytes: Arc<Vec<u8>>, password: Option<String>, wake: Arc<dyn Fn() + Send + Sync>) -> Result<Self, crate::pdfdoc::Error> {
        let pdf = open(bytes.clone(), password.as_deref())?;
        let pages = page_infos(&pdf);
        drop(pdf);
        let shared = Arc::new(Shared { queue: Mutex::new((VecDeque::new(), false)), cv: Condvar::new(), min_tag: AtomicU64::new(0) });
        let (tx, rx) = std::sync::mpsc::channel();
        let threads = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(2).saturating_sub(1).clamp(1, 3);
        for _ in 0..threads {
            let shared = shared.clone();
            let tx = tx.clone();
            let wake = wake.clone();
            let bytes = bytes.clone();
            let password = password.clone();
            std::thread::Builder::new()
                .name("lightpdf-render".into())
                .stack_size(16 << 20)
                .spawn(move || {
                    let Ok(pdf) = open(bytes, password.as_deref()) else { return };
                    let cache = RenderCache::new();
                    loop {
                        let job = {
                            let mut q = shared.queue.lock().unwrap();
                            loop {
                                if q.1 {
                                    return;
                                }
                                // newest first: what the user looks at right now
                                if let Some(j) = q.0.pop_back() {
                                    break j;
                                }
                                q = shared.cv.wait(q).unwrap();
                            }
                        };
                        let out = match job {
                            Job::Render { page, scale, rotation, region, tag } => {
                                if tag < shared.min_tag.load(Ordering::Relaxed) {
                                    continue;
                                }
                                let Some(bitmap) = render_page(&pdf, &cache, page, scale, rotation, region) else {
                                    continue;
                                };
                                Output::Rendered { page, scale, rotation, region, tag, bitmap }
                            }
                            Job::Text { page } => Output::Text { page, text: page_text(&pdf, page) },
                        };
                        if tx.send(out).is_err() {
                            return;
                        }
                        wake();
                    }
                })
                .expect("spawn render thread");
        }
        Ok(Self { shared, results: rx, pages })
    }

    pub fn submit(&self, job: Job) {
        self.shared.queue.lock().unwrap().0.push_back(job);
        self.shared.cv.notify_one();
    }

    /// Drops queued render jobs with a tag lower than `tag` (e.g. after zooming).
    pub fn discard_below(&self, tag: u64) {
        self.shared.min_tag.store(tag, Ordering::Relaxed);
        let mut q = self.shared.queue.lock().unwrap();
        q.0.retain(|j| !matches!(j, Job::Render { tag: t, .. } if *t < tag));
    }

    pub fn clear_queue(&self) {
        self.shared.queue.lock().unwrap().0.clear();
    }
}

impl Drop for RenderPool {
    fn drop(&mut self) {
        self.shared.queue.lock().unwrap().1 = true;
        self.shared.cv.notify_all();
    }
}
