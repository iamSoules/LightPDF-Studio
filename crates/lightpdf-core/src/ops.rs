//! All document-level editing functions (port of v2's `pdfops.ts`).
//! Every function takes PDF bytes and returns new PDF bytes.

use crate::annotio::{self, Ops, n};
use crate::model::{Annotation, Kind, Rect, hex_to_rgb};
use crate::pdfdoc::{self, Bytes, Error, Result, load, save, text_hex};
use crate::render;
use crate::text;
use lopdf::{Dictionary, Document, Object, dictionary};
use std::collections::BTreeMap;
use std::io::Write;
use std::sync::Arc;

pub const A4: [f64; 2] = [595.28, 841.89];
pub const LETTER: [f64; 2] = [612.0, 792.0];

// ------------------------------------------------------------------ import / export

pub struct Imported {
    /// Unencrypted document without our own annotations (what gets rendered and edited).
    pub base: Bytes,
    pub annots: Vec<Annotation>,
    /// The original file was encrypted.
    pub encrypted: bool,
}

/// On open: decrypt, take out our own annotations so they stay editable.
pub fn import_pdf(bytes: &[u8], password: Option<&str>) -> Result<Imported> {
    let mut d = load(bytes, password)?;
    let encrypted = password.is_some() || d.trailer.get(b"Encrypt").is_ok() || is_encrypted_file(bytes);
    let annots = annotio::extract_own_annotations(&mut d);
    let base = if annots.is_empty() && !encrypted { bytes.to_vec() } else { save(&mut d)? };
    Ok(Imported { base, annots, encrypted })
}

fn is_encrypted_file(bytes: &[u8]) -> bool {
    // cheap check for "/Encrypt" in the trailer region
    let tail = &bytes[bytes.len().saturating_sub(4096)..];
    tail.windows(8).any(|w| w == b"/Encrypt")
}

#[derive(Clone, Default)]
pub struct ExportOpts {
    pub password: Option<String>,
    /// Burn comments (and form fields) into the page content.
    pub flatten: bool,
}

/// Final PDF: base + comments (+ optional password).
pub fn export_pdf(base: &[u8], annots: &[Annotation], o: &ExportOpts) -> Result<Bytes> {
    let mut d = load(base, None)?;
    let list: Vec<Annotation> = annots.iter().filter(|a| !(o.flatten && matches!(a.kind, Kind::Redact { .. }))).cloned().collect();
    annotio::write_annotations(&mut d, &list)?;
    if o.flatten {
        flatten_form_fields(&mut d);
        annotio::flatten_annotations(&mut d, |sub| sub == b"Link" || sub == b"Popup")?;
    }
    if let Some(pw) = o.password.as_deref().filter(|p| !p.is_empty()) {
        encrypt(&mut d, pw, pw, lopdf::Permissions::all())?;
    }
    save(&mut d)
}

/// Copy for display: all foreign annotations and form fields baked into the content, since
/// the renderer only draws page content. Links stay as they are.
pub fn display_copy(base: &[u8]) -> Result<Bytes> {
    let mut d = load(base, None)?;
    let any = pdfdoc::pages(&d).into_iter().any(|p| {
        pdfdoc::page_annots(&d, p).iter().any(|a| {
            pdfdoc::deref(&d, a)
                .as_dict()
                .ok()
                .and_then(|a| a.get(b"Subtype").ok())
                .and_then(|s| s.as_name().ok())
                .is_some_and(|s| s != b"Link" && s != b"Popup")
        })
    });
    if !any {
        return Ok(base.to_vec());
    }
    annotio::flatten_annotations(&mut d, |sub| sub == b"Link" || sub == b"Popup")?;
    let mut out = Vec::new();
    d.save_to(&mut out)?;
    Ok(out)
}

// ------------------------------------------------------------------ links

#[derive(Clone, Debug, PartialEq)]
pub enum LinkTarget {
    Page(usize),
    Uri(String),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Link {
    pub page: usize,
    pub rect: Rect,
    pub target: LinkTarget,
}

fn dest_page(d: &Document, dest: &Object, page_index: &BTreeMap<lopdf::ObjectId, usize>) -> Option<usize> {
    match pdfdoc::deref(d, dest) {
        Object::Array(a) => match a.first()? {
            Object::Reference(r) => page_index.get(r).copied(),
            Object::Integer(i) => Some(*i as usize),
            _ => None,
        },
        Object::Name(nm) | Object::String(nm, _) => {
            // named destination
            let cat = d.catalog().ok()?;
            if let Ok(dests) = cat.get(b"Dests").map(|o| pdfdoc::deref(d, o))
                && let Ok(v) = dests.as_dict().and_then(|dd| dd.get(nm))
            {
                let v = pdfdoc::deref(d, v);
                let v = v.as_dict().ok().and_then(|x| x.get(b"D").ok()).unwrap_or(v);
                return dest_page(d, v, page_index);
            }
            let names = pdfdoc::deref(d, cat.get(b"Names").ok()?).as_dict().ok()?;
            let tree = pdfdoc::deref(d, names.get(b"Dests").ok()?);
            let v = name_tree_lookup(d, tree, nm, 0)?;
            let v = pdfdoc::deref(d, v);
            let v = v.as_dict().ok().and_then(|x| x.get(b"D").ok()).unwrap_or(v);
            dest_page(d, v, page_index)
        }
        Object::Dictionary(dd) => dest_page(d, dd.get(b"D").ok()?, page_index),
        _ => None,
    }
}

fn name_tree_lookup<'a>(d: &'a Document, node: &'a Object, key: &[u8], depth: u32) -> Option<&'a Object> {
    if depth > 32 {
        return None;
    }
    let node = pdfdoc::deref(d, node).as_dict().ok()?;
    if let Ok(Object::Array(names)) = node.get(b"Names").map(|o| pdfdoc::deref(d, o)) {
        for pair in names.chunks(2) {
            if let [Object::String(k, _), v] = pair
                && k == key
            {
                return Some(v);
            }
        }
    }
    if let Ok(Object::Array(kids)) = node.get(b"Kids").map(|o| pdfdoc::deref(d, o)) {
        for k in kids {
            if let Some(v) = name_tree_lookup(d, k, key, depth + 1) {
                return Some(v);
            }
        }
    }
    None
}

/// Clickable links of all pages.
pub fn links(bytes: &[u8]) -> Vec<Link> {
    let Ok(d) = load(bytes, None) else { return vec![] };
    let pages = pdfdoc::pages(&d);
    let index: BTreeMap<_, _> = pages.iter().enumerate().map(|(i, id)| (*id, i)).collect();
    let mut out = Vec::new();
    for (pi, &p) in pages.iter().enumerate() {
        for a in pdfdoc::page_annots(&d, p) {
            let Ok(a) = pdfdoc::deref(&d, &a).as_dict() else { continue };
            if a.get(b"Subtype").and_then(Object::as_name).ok() != Some(b"Link") {
                continue;
            }
            let Some(r) = a.get(b"Rect").ok().and_then(|r| pdfdoc::num_array(&d, r)).filter(|v| v.len() == 4) else {
                continue;
            };
            let rect = [r[0].min(r[2]), r[1].min(r[3]), r[0].max(r[2]), r[1].max(r[3])];
            let target = if let Ok(dest) = a.get(b"Dest") {
                dest_page(&d, dest, &index).map(LinkTarget::Page)
            } else if let Ok(act) = a.get(b"A").map(|o| pdfdoc::deref(&d, o)).and_then(Object::as_dict) {
                match act.get(b"S").and_then(Object::as_name).ok() {
                    Some(b"URI") => act.get(b"URI").ok().and_then(|u| pdfdoc::decode_text(&d, u)).map(LinkTarget::Uri),
                    Some(b"GoTo") => act.get(b"D").ok().and_then(|x| dest_page(&d, x, &index)).map(LinkTarget::Page),
                    _ => None,
                }
            } else {
                None
            };
            if let Some(target) = target {
                out.push(Link { page: pi, rect, target });
            }
        }
    }
    out
}

// ------------------------------------------------------------------ outline

#[derive(Clone, Debug)]
pub struct OutlineItem {
    pub title: String,
    pub page: Option<usize>,
    pub children: Vec<OutlineItem>,
}

pub fn outline(bytes: &[u8]) -> Vec<OutlineItem> {
    let Ok(d) = load(bytes, None) else { return vec![] };
    let index: BTreeMap<_, _> = pdfdoc::pages(&d).into_iter().enumerate().map(|(i, id)| (id, i)).collect();
    let Some(first) = d
        .catalog()
        .ok()
        .and_then(|c| c.get(b"Outlines").ok())
        .and_then(|o| pdfdoc::deref(&d, o).as_dict().ok())
        .and_then(|o| o.get(b"First").ok())
        .cloned()
    else {
        return vec![];
    };
    fn walk(d: &Document, first: &Object, index: &BTreeMap<lopdf::ObjectId, usize>, depth: u32, budget: &mut u32) -> Vec<OutlineItem> {
        let mut out = Vec::new();
        let mut cur = Some(first.clone());
        while let Some(o) = cur.take() {
            if *budget == 0 || depth > 16 {
                break;
            }
            *budget -= 1;
            let Ok(item) = pdfdoc::deref(d, &o).as_dict() else { break };
            let title = item.get(b"Title").ok().and_then(|t| pdfdoc::decode_text(d, t)).unwrap_or_default();
            let page = if let Ok(dest) = item.get(b"Dest") {
                dest_page(d, dest, index)
            } else {
                item.get(b"A")
                    .ok()
                    .and_then(|a| pdfdoc::deref(d, a).as_dict().ok())
                    .and_then(|a| a.get(b"D").ok())
                    .and_then(|x| dest_page(d, x, index))
            };
            let children = match item.get(b"First") {
                Ok(f) => walk(d, f, index, depth + 1, budget),
                Err(_) => vec![],
            };
            out.push(OutlineItem { title, page, children });
            cur = item.get(b"Next").ok().cloned();
        }
        out
    }
    let mut budget = 5000;
    walk(&d, &first, &index, 0, &mut budget)
}

// ------------------------------------------------------------------ organise pages

pub fn rotate_pages(bytes: &[u8], pages: &[usize], delta: i32) -> Result<Bytes> {
    let mut d = load(bytes, None)?;
    let ids = pdfdoc::pages(&d);
    for &i in pages {
        if let Some(&p) = ids.get(i) {
            let r = pdfdoc::rotation(&d, p);
            pdfdoc::set_rotation(&mut d, p, r + delta);
        }
    }
    save(&mut d)
}

pub fn delete_pages(bytes: &[u8], pages: &[usize]) -> Result<Bytes> {
    let mut d = load(bytes, None)?;
    let ids = pdfdoc::pages(&d);
    let order: Vec<_> = ids.iter().enumerate().filter(|(i, _)| !pages.contains(i)).map(|(_, id)| *id).collect();
    if order.is_empty() {
        return Err(Error::Other("a PDF needs at least one page".into()));
    }
    pdfdoc::set_pages(&mut d, &order)?;
    save(&mut d)
}

/// `order`: new order as a list of old indices (duplicates allowed).
pub fn reorder_pages(bytes: &[u8], order: &[usize]) -> Result<Bytes> {
    let mut d = load(bytes, None)?;
    let ids = pdfdoc::pages(&d);
    let new: Vec<_> = order.iter().filter_map(|&i| ids.get(i).copied()).collect();
    if new.is_empty() {
        return Err(Error::Other("a PDF needs at least one page".into()));
    }
    pdfdoc::set_pages(&mut d, &new)?;
    save(&mut d)
}

/// Inserts a blank page at `at` (0-based), sized like its neighbour.
pub fn insert_blank(bytes: &[u8], at: usize) -> Result<Bytes> {
    let mut d = load(bytes, None)?;
    let ids = pdfdoc::pages(&d);
    let size = ids
        .get(at.saturating_sub(1).min(ids.len().saturating_sub(1)))
        .map(|&p| {
            let b = pdfdoc::media_box(&d, p);
            let (w, h) = (b[2] - b[0], b[3] - b[1]);
            if pdfdoc::rotation(&d, p) % 180 != 0 { [h, w] } else { [w, h] }
        })
        .unwrap_or(A4);
    let page = pdfdoc::add_page(&mut d, size, Vec::new(), Dictionary::new())?;
    let mut order = pdfdoc::pages(&d);
    order.retain(|&p| p != page);
    order.insert(at.min(order.len()), page);
    pdfdoc::set_pages(&mut d, &order)?;
    save(&mut d)
}

pub fn insert_pdf(bytes: &[u8], other: &[u8], at: usize) -> Result<Bytes> {
    let mut d = load(bytes, None)?;
    let o = load(other, None)?;
    let n = pdfdoc::pages(&o).len();
    let ids = pdfdoc::import_pages(&mut d, &o, &(0..n).collect::<Vec<_>>())?;
    pdfdoc::insert_page_ids(&mut d, at, &ids)?;
    save(&mut d)
}

pub fn extract_pages(bytes: &[u8], pages: &[usize]) -> Result<Bytes> {
    let src = load(bytes, None)?;
    let mut d = pdfdoc::new_document();
    let ids = pdfdoc::import_pages(&mut d, &src, pages)?;
    pdfdoc::insert_page_ids(&mut d, 0, &ids)?;
    copy_info(&src, &mut d);
    save(&mut d)
}

fn copy_info(src: &Document, dst: &mut Document) {
    let title = pdfdoc::info_get(src, "Title");
    if !title.is_empty() {
        pdfdoc::info_mut(dst).set("Title", text_hex(&title));
    }
}

pub fn merge(list: &[Vec<u8>]) -> Result<Bytes> {
    let mut d = pdfdoc::new_document();
    for b in list {
        let s = load(b, None)?;
        let n = pdfdoc::pages(&s).len();
        let ids = pdfdoc::import_pages(&mut d, &s, &(0..n).collect::<Vec<_>>())?;
        let at = pdfdoc::pages(&d).len();
        pdfdoc::insert_page_ids(&mut d, at, &ids)?;
    }
    pdfdoc::info_mut(&mut d).set("Producer", text_hex("LightPDF Studio"));
    save(&mut d)
}

/// Ranges like "1-3, 5, 8-" → groups of 0-based page indices.
pub fn parse_ranges(spec: &str, total: usize) -> Vec<Vec<usize>> {
    let mut groups = Vec::new();
    for part in spec.split([',', ';']).map(str::trim).filter(|s| !s.is_empty()) {
        let (a, b) = match part.split_once('-') {
            Some((a, b)) => {
                let a = a.trim();
                let b = b.trim();
                (if a.is_empty() { 1 } else { a.parse().unwrap_or(0) }, if b.is_empty() { total } else { b.parse().unwrap_or(0) })
            }
            None => {
                let v = part.parse().unwrap_or(0);
                (v, v)
            }
        };
        if a == 0 || b == 0 || total == 0 {
            continue;
        }
        let a = a.clamp(1, total);
        let b = b.clamp(1, total);
        groups.push((a.min(b)..=a.max(b)).map(|i| i - 1).collect());
    }
    groups
}

pub fn split(bytes: &[u8], groups: &[Vec<usize>]) -> Result<Vec<Bytes>> {
    groups.iter().map(|g| extract_pages(bytes, g)).collect()
}

pub fn split_every(bytes: &[u8], n: usize) -> Result<Vec<Bytes>> {
    let d = load(bytes, None)?;
    let total = pdfdoc::pages(&d).len();
    let n = n.max(1);
    let groups: Vec<Vec<usize>> = (0..total).collect::<Vec<_>>().chunks(n).map(|c| c.to_vec()).collect();
    split(bytes, &groups)
}

// ------------------------------------------------------------------ create

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PageSize {
    Fit,
    A4,
    Letter,
}

/// Image files → PDF (one page per image).
pub fn images_to_pdf(files: &[Vec<u8>], size: PageSize, margin: f64) -> Result<Bytes> {
    let mut d = pdfdoc::new_document();
    for f in files {
        let Some((img, w, h)) = annotio::embed_image(&mut d, f) else {
            return Err(Error::Other("unsupported image".into()));
        };
        let (iw, ih) = (w as f64 * 0.75, h as f64 * 0.75);
        let (pw, ph, x, y, dw, dh) = match size {
            PageSize::Fit => (iw, ih, 0.0, 0.0, iw, ih),
            _ => {
                let [mut pw, mut ph] = if size == PageSize::A4 { A4 } else { LETTER };
                if w > h {
                    std::mem::swap(&mut pw, &mut ph);
                }
                let s = ((pw - 2.0 * margin) / w as f64).min((ph - 2.0 * margin) / h as f64);
                let (dw, dh) = (w as f64 * s, h as f64 * s);
                (pw, ph, (pw - dw) / 2.0, (ph - dh) / 2.0, dw, dh)
            }
        };
        let mut o = Ops::default();
        o.q().cm([dw, 0.0, 0.0, dh, x, y]).raw("/Im0 Do").Q();
        pdfdoc::add_page(&mut d, [pw, ph], o.take(), dictionary! { "XObject" => dictionary! { "Im0" => img } })?;
    }
    save(&mut d)
}

pub fn blank_pdf(pages: usize, size: [f64; 2]) -> Result<Bytes> {
    let mut d = pdfdoc::new_document();
    for _ in 0..pages.max(1) {
        pdfdoc::add_page(&mut d, size, Vec::new(), Dictionary::new())?;
    }
    save(&mut d)
}

// ------------------------------------------------------------------ stamping

/// Visible page geometry (honours /Rotate): visual coordinates → PDF coordinates.
struct Visual {
    r: i32,
    vw: f64,
    vh: f64,
    b: [f64; 4],
}

impl Visual {
    fn of(d: &Document, p: lopdf::ObjectId) -> Self {
        let r = pdfdoc::rotation(d, p);
        let b = pdfdoc::media_box(d, p);
        let (w, h) = (b[2] - b[0], b[3] - b[1]);
        Self { r, vw: if r % 180 != 0 { h } else { w }, vh: if r % 180 != 0 { w } else { h }, b }
    }
    fn to_user(&self, vx: f64, vy: f64) -> [f64; 2] {
        let [x0, y0, x1, y1] = self.b;
        let (w, h) = (x1 - x0, y1 - y0);
        match self.r {
            90 => [x0 + w - vy, y0 + vx],
            180 => [x0 + w - vx, y0 + h - vy],
            270 => [x0 + vy, y0 + h - vx],
            _ => [x0 + vx, y0 + vy],
        }
    }
}

/// Text drawing op (Helvetica, WinAnsi) at (x, y) with rotation in degrees.
fn text_op(o: &mut Ops, font: &str, size: f64, rgb: [f64; 3], x: f64, y: f64, deg: f64, s: &str) {
    let (sin, cos) = deg.to_radians().sin_cos();
    o.raw("BT").fill_rgb(rgb).raw(&format!("/{font} {} Tf", n(size)));
    o.raw(&format!("{} {} {} {} {} {} Tm", n(cos), n(sin), n(-sin), n(cos), n(x), n(y)));
    let mut b = text::pdf_literal(&text::encode_win_ansi(s));
    b.extend_from_slice(b" Tj");
    o.bytes(&b).raw("ET");
}

#[derive(Clone, Debug)]
pub struct WatermarkOpts {
    pub text: String,
    pub size: f64,
    pub color: String,
    pub opacity: f64,
    pub angle: f64,
    /// 0-based; empty = all pages
    pub pages: Vec<usize>,
}

pub fn watermark(bytes: &[u8], o: &WatermarkOpts) -> Result<Bytes> {
    let mut d = load(bytes, None)?;
    let font = annotio::standard_font(&mut d, "Helvetica-Bold");
    let gs = d.add_object(dictionary! { "Type" => "ExtGState", "ca" => o.opacity as f32, "CA" => o.opacity as f32 });
    let rgb = hex_to_rgb(&o.color);
    let ids = pdfdoc::pages(&d);
    let idx: Vec<usize> = if o.pages.is_empty() { (0..ids.len()).collect() } else { o.pages.clone() };
    let t = text::safe_text(&o.text);
    let tw = text::text_width(&t, o.size, true);
    for i in idx {
        let Some(&p) = ids.get(i) else { continue };
        let v = Visual::of(&d, p);
        let rad = o.angle.to_radians();
        // text start so that its centre lies in the page centre (visual coordinates)
        let vx = v.vw / 2.0 - tw / 2.0 * rad.cos() + o.size / 3.0 * rad.sin();
        let vy = v.vh / 2.0 - tw / 2.0 * rad.sin() - o.size / 3.0 * rad.cos();
        let [x, y] = v.to_user(vx, vy);
        let fname = pdfdoc::add_resource(&mut d, p, "Font", "LPF", Object::Reference(font))?;
        let gname = pdfdoc::add_resource(&mut d, p, "ExtGState", "LPG", Object::Reference(gs))?;
        let mut ops = Ops::default();
        ops.q().gs(&gname);
        text_op(&mut ops, &fname, o.size, rgb, x, y, o.angle + v.r as f64, &t);
        ops.Q();
        pdfdoc::append_content(&mut d, p, ops.take())?;
    }
    save(&mut d)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Position {
    TopLeft,
    TopCenter,
    TopRight,
    BottomLeft,
    BottomCenter,
    BottomRight,
}

#[derive(Clone, Debug)]
pub struct StampTextOpts {
    /// `{n}`, `{total}`, `{date}`, `{name}`
    pub template: String,
    pub position: Position,
    pub size: f64,
    pub color: String,
    pub start: usize,
    pub margin: f64,
    pub skip_first: bool,
    pub name: String,
    pub date: String,
}

/// Page numbers / header / footer.
pub fn stamp_text(bytes: &[u8], o: &StampTextOpts) -> Result<Bytes> {
    let mut d = load(bytes, None)?;
    let font = annotio::standard_font(&mut d, "Helvetica");
    let rgb = hex_to_rgb(&o.color);
    let ids = pdfdoc::pages(&d);
    let total = ids.len();
    for (i, &p) in ids.iter().enumerate() {
        if o.skip_first && i == 0 {
            continue;
        }
        let t = text::safe_text(
            &o.template
                .replace("{n}", &(i + o.start).to_string())
                .replace("{total}", &(total + o.start - 1).to_string())
                .replace("{date}", &o.date)
                .replace("{name}", &o.name),
        );
        let v = Visual::of(&d, p);
        let tw = text::text_width(&t, o.size, false);
        use Position::*;
        let vx = match o.position {
            TopLeft | BottomLeft => o.margin,
            TopRight | BottomRight => v.vw - o.margin - tw,
            _ => (v.vw - tw) / 2.0,
        };
        let vy = match o.position {
            TopLeft | TopCenter | TopRight => v.vh - o.margin - o.size,
            _ => o.margin,
        };
        let [x, y] = v.to_user(vx, vy);
        let fname = pdfdoc::add_resource(&mut d, p, "Font", "LPF", Object::Reference(font))?;
        let mut ops = Ops::default();
        ops.q();
        text_op(&mut ops, &fname, o.size, rgb, x, y, v.r as f64, &t);
        ops.Q();
        pdfdoc::append_content(&mut d, p, ops.take())?;
    }
    save(&mut d)
}

// ------------------------------------------------------------------ protect

#[derive(Clone, Debug)]
pub struct ProtectOpts {
    pub user_password: String,
    pub owner_password: String,
    pub allow_print: bool,
    pub allow_copy: bool,
    pub allow_modify: bool,
}

fn encrypt(d: &mut Document, user: &str, owner: &str, perms: lopdf::Permissions) -> Result<()> {
    use lopdf::encryption::crypt_filters::{Aes256CryptFilter, CryptFilter};
    if pdfdoc::pdfa_part(d).is_some() {
        pdfdoc::strip_pdfa(d); // PDF/A forbids encryption
    }
    // AES-256 needs a document ID
    if d.trailer.get(b"ID").is_err() {
        let id: [u8; 16] = rand::random();
        let s = Object::String(id.to_vec(), lopdf::StringFormat::Hexadecimal);
        d.trailer.set("ID", Object::Array(vec![s.clone(), s]));
    }
    let key: [u8; 32] = rand::random();
    let cf: Arc<dyn CryptFilter> = Arc::new(Aes256CryptFilter);
    let state = lopdf::EncryptionState::try_from(lopdf::EncryptionVersion::V5 {
        encrypt_metadata: true,
        crypt_filters: BTreeMap::from([(b"StdCF".to_vec(), cf)]),
        file_encryption_key: &key,
        stream_filter: b"StdCF".to_vec(),
        string_filter: b"StdCF".to_vec(),
        owner_password: owner,
        user_password: user,
        permissions: perms,
    })?;
    d.encrypt(&state)?;
    Ok(())
}

pub fn protect(bytes: &[u8], o: &ProtectOpts) -> Result<Bytes> {
    use lopdf::Permissions as P;
    let mut d = load(bytes, None)?;
    let mut perms = P::FILLABLE | P::COPYABLE_FOR_ACCESSIBILITY;
    if o.allow_print {
        perms |= P::PRINTABLE | P::PRINTABLE_IN_HIGH_QUALITY;
    }
    if o.allow_copy {
        perms |= P::COPYABLE;
    }
    if o.allow_modify {
        perms |= P::MODIFIABLE | P::ANNOTABLE | P::ASSEMBLABLE;
    }
    let owner = if o.owner_password.is_empty() { &o.user_password } else { &o.owner_password };
    encrypt(&mut d, &o.user_password, owner, perms)?;
    save(&mut d)
}

pub fn remove_password(bytes: &[u8], pw: &str) -> Result<Bytes> {
    let mut d = load(bytes, Some(pw))?;
    save(&mut d)
}

// ------------------------------------------------------------------ metadata

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Meta {
    pub title: String,
    pub author: String,
    pub subject: String,
    pub keywords: String,
    pub creator: String,
    pub producer: String,
    pub created: String,
    pub modified: String,
    pub version: String,
    pub pages: usize,
}

pub fn get_meta(bytes: &[u8]) -> Result<Meta> {
    let d = load(bytes, None)?;
    let g = |k| pdfdoc::info_get(&d, k);
    Ok(Meta {
        title: g("Title"),
        author: g("Author"),
        subject: g("Subject"),
        keywords: g("Keywords"),
        creator: g("Creator"),
        producer: g("Producer"),
        created: pdfdoc::format_pdf_date(&g("CreationDate")),
        modified: pdfdoc::format_pdf_date(&g("ModDate")),
        version: d.version.clone(),
        pages: pdfdoc::pages(&d).len(),
    })
}

pub fn set_meta(bytes: &[u8], m: &Meta) -> Result<Bytes> {
    let mut d = load(bytes, None)?;
    let info = pdfdoc::info_mut(&mut d);
    for (k, v) in [("Title", &m.title), ("Author", &m.author), ("Subject", &m.subject), ("Keywords", &m.keywords)] {
        if v.is_empty() {
            info.remove(k.as_bytes());
        } else {
            info.set(k, text_hex(v));
        }
    }
    info.set("ModDate", pdfdoc::literal(&pdfdoc::pdf_date(crate::model::now_ms())));
    info.set("Producer", text_hex("LightPDF Studio"));
    save(&mut d)
}

// ------------------------------------------------------------------ forms

#[derive(Clone, Debug, PartialEq)]
pub enum FieldValue {
    Text(String),
    Check(bool),
    Choice { value: String, options: Vec<String> },
}

#[derive(Clone, Debug, PartialEq)]
pub struct FormField {
    pub name: String,
    pub value: FieldValue,
    pub page: Option<usize>,
    pub rect: Option<Rect>,
    pub read_only: bool,
    /// object id of the terminal field
    pub id: (u32, u16),
}

fn field_name(d: &Document, mut id: lopdf::ObjectId) -> String {
    let mut parts = Vec::new();
    for _ in 0..32 {
        let Ok(f) = d.get_dictionary(id) else { break };
        if let Some(t) = f.get(b"T").ok().and_then(|t| pdfdoc::decode_text(d, t)) {
            parts.push(t);
        }
        match f.get(b"Parent").and_then(Object::as_reference) {
            Ok(p) => id = p,
            Err(_) => break,
        }
    }
    parts.reverse();
    parts.join(".")
}

fn field_attr<'a>(d: &'a Document, mut id: lopdf::ObjectId, key: &[u8]) -> Option<&'a Object> {
    for _ in 0..32 {
        let f = d.get_dictionary(id).ok()?;
        if let Ok(v) = f.get(key) {
            return Some(pdfdoc::deref(d, v));
        }
        id = f.get(b"Parent").and_then(Object::as_reference).ok()?;
    }
    None
}

fn terminal_fields(d: &Document) -> Vec<lopdf::ObjectId> {
    let Some(fields) = d
        .catalog()
        .ok()
        .and_then(|c| c.get(b"AcroForm").ok())
        .and_then(|a| pdfdoc::deref(d, a).as_dict().ok())
        .and_then(|a| a.get(b"Fields").ok())
        .map(|f| pdfdoc::deref(d, f))
        .and_then(|f| f.as_array().ok())
    else {
        return vec![];
    };
    let mut out = Vec::new();
    let mut stack: Vec<lopdf::ObjectId> = fields.iter().filter_map(|f| f.as_reference().ok()).rev().collect();
    let mut guard = 0;
    while let Some(id) = stack.pop() {
        guard += 1;
        if guard > 20000 {
            break;
        }
        let Ok(f) = d.get_dictionary(id) else { continue };
        let kids: Vec<lopdf::ObjectId> = f
            .get(b"Kids")
            .ok()
            .map(|k| pdfdoc::deref(d, k))
            .and_then(|k| k.as_array().ok())
            .map(|k| k.iter().filter_map(|x| x.as_reference().ok()).collect())
            .unwrap_or_default();
        // kids that have /T are fields; otherwise they're widgets of this field
        let field_kids: Vec<_> = kids.iter().copied().filter(|k| d.get_dictionary(*k).is_ok_and(|kd| kd.has(b"T"))).collect();
        if field_kids.is_empty() {
            out.push(id);
        } else {
            stack.extend(field_kids.into_iter().rev());
        }
    }
    out
}

fn widget_of(d: &Document, id: lopdf::ObjectId) -> lopdf::ObjectId {
    d.get_dictionary(id)
        .ok()
        .and_then(|f| f.get(b"Kids").ok())
        .map(|k| pdfdoc::deref(d, k))
        .and_then(|k| k.as_array().ok())
        .and_then(|k| k.first())
        .and_then(|k| k.as_reference().ok())
        .unwrap_or(id)
}

pub fn form_fields(bytes: &[u8]) -> Vec<FormField> {
    let Ok(d) = load(bytes, None) else { return vec![] };
    let page_index = pdfdoc::object_ids_of_pages(&d);
    let mut out = Vec::new();
    for id in terminal_fields(&d) {
        let ft = field_attr(&d, id, b"FT").and_then(|o| o.as_name().ok()).unwrap_or(b"");
        let flags = field_attr(&d, id, b"Ff").and_then(pdfdoc::as_f64).unwrap_or(0.0) as i64;
        let v = field_attr(&d, id, b"V");
        let value = match ft {
            b"Tx" => FieldValue::Text(v.and_then(|v| pdfdoc::decode_text(&d, v)).unwrap_or_default()),
            b"Btn" => {
                if flags & (1 << 16) != 0 {
                    continue; // push button
                }
                let on = v.and_then(|v| v.as_name().ok()).is_some_and(|n| n != b"Off");
                FieldValue::Check(on)
            }
            b"Ch" => {
                let options = field_attr(&d, id, b"Opt")
                    .and_then(|o| o.as_array().ok())
                    .map(|a| {
                        a.iter()
                            .filter_map(|x| match pdfdoc::deref(&d, x) {
                                Object::Array(pair) => pair.get(1).and_then(|p| pdfdoc::decode_text(&d, p)),
                                o => pdfdoc::decode_text(&d, o),
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                FieldValue::Choice { value: v.and_then(|v| pdfdoc::decode_text(&d, v)).unwrap_or_default(), options }
            }
            _ => continue,
        };
        let w = widget_of(&d, id);
        let wd = d.get_dictionary(w).ok();
        let page = wd.and_then(|w| w.get(b"P").ok()).and_then(|p| p.as_reference().ok()).and_then(|p| page_index.get(&p).copied());
        let rect = wd
            .and_then(|w| w.get(b"Rect").ok())
            .and_then(|r| pdfdoc::num_array(&d, r))
            .filter(|v| v.len() == 4)
            .map(|r| [r[0].min(r[2]), r[1].min(r[3]), r[0].max(r[2]), r[1].max(r[3])]);
        out.push(FormField { name: field_name(&d, id), value, page, rect, read_only: flags & 1 != 0, id });
    }
    out
}

fn on_state(d: &Document, widget: lopdf::ObjectId) -> Vec<u8> {
    d.get_dictionary(widget)
        .ok()
        .and_then(|w| w.get(b"AP").ok())
        .and_then(|ap| pdfdoc::deref(d, ap).as_dict().ok())
        .and_then(|ap| ap.get(b"N").ok())
        .and_then(|n| pdfdoc::deref(d, n).as_dict().ok())
        .and_then(|n| n.iter().map(|(k, _)| k.clone()).find(|k| k != b"Off"))
        .unwrap_or_else(|| b"Yes".to_vec())
}

/// Writes field values and regenerates simple appearances for text/choice fields.
pub fn fill_form(bytes: &[u8], values: &[(String, FieldValue)]) -> Result<Bytes> {
    let mut d = load(bytes, None)?;
    let fields = terminal_fields(&d);
    let font = annotio::standard_font(&mut d, "Helvetica");
    for id in fields {
        let nm = field_name(&d, id);
        let Some((_, val)) = values.iter().find(|(k, _)| *k == nm) else { continue };
        let widgets: Vec<lopdf::ObjectId> = {
            let f = d.get_dictionary(id)?;
            match f.get(b"Kids").ok().map(|k| pdfdoc::deref(&d, k)).and_then(|k| k.as_array().ok()) {
                Some(k) => k.iter().filter_map(|x| x.as_reference().ok()).collect(),
                None => vec![id],
            }
        };
        match val {
            FieldValue::Check(on) => {
                for &w in &widgets {
                    let st = if *on { on_state(&d, w) } else { b"Off".to_vec() };
                    d.get_dictionary_mut(w)?.set("AS", Object::Name(st.clone()));
                    if *on {
                        d.get_dictionary_mut(id)?.set("V", Object::Name(st));
                    }
                }
                if !*on {
                    d.get_dictionary_mut(id)?.set("V", Object::Name(b"Off".to_vec()));
                }
            }
            FieldValue::Text(s) | FieldValue::Choice { value: s, .. } => {
                d.get_dictionary_mut(id)?.set("V", text_hex(s));
                let da = field_attr(&d, id, b"DA").and_then(|o| pdfdoc::decode_text(&d, o)).unwrap_or_default();
                let size = da
                    .split_whitespace()
                    .collect::<Vec<_>>()
                    .windows(2)
                    .find(|w| w[1] == "Tf")
                    .and_then(|w| w[0].parse::<f64>().ok())
                    .unwrap_or(0.0);
                let multiline = field_attr(&d, id, b"Ff").and_then(pdfdoc::as_f64).unwrap_or(0.0) as i64 & (1 << 12) != 0;
                for &w in &widgets {
                    let Some(r) = d
                        .get_dictionary(w)
                        .ok()
                        .and_then(|wd| wd.get(b"Rect").ok())
                        .and_then(|r| pdfdoc::num_array(&d, r))
                        .filter(|v| v.len() == 4)
                    else {
                        continue;
                    };
                    let (bw, bh) = ((r[2] - r[0]).abs(), (r[3] - r[1]).abs());
                    let size = if size > 0.0 { size } else { (bh * 0.65).clamp(6.0, 14.0) };
                    let mut o = Ops::default();
                    o.raw("/Tx BMC").q().re(1.0, 1.0, bw - 2.0, bh - 2.0).raw("W n").raw("BT").fill_rgb([0.0; 3]);
                    o.raw(&format!("/F1 {} Tf", n(size)));
                    let safe = text::safe_text(s);
                    let lines = if multiline { text::wrap_text(&safe, size, false, bw - 4.0) } else { vec![safe.replace('\n', " ")] };
                    let y0 = if multiline { bh - 2.0 - size } else { (bh - size * 0.75) / 2.0 };
                    o.raw(&format!("2 {} Td", n(y0)));
                    for (i, l) in lines.iter().enumerate() {
                        if i > 0 {
                            o.raw(&format!("0 {} Td", n(-size * 1.15)));
                        }
                        let mut b = text::pdf_literal(&text::encode_win_ansi(l));
                        b.extend_from_slice(b" Tj");
                        o.bytes(&b);
                    }
                    o.raw("ET").Q().raw("EMC");
                    let xo = d.add_object(lopdf::Stream::new(
                        dictionary! {
                            "Type" => "XObject", "Subtype" => "Form",
                            "BBox" => pdfdoc::rect_obj([0.0, 0.0, bw, bh]),
                            "Resources" => dictionary! { "Font" => dictionary! { "F1" => font } },
                        },
                        o.take(),
                    ));
                    d.get_dictionary_mut(w)?.set("AP", dictionary! { "N" => xo });
                }
            }
        }
    }
    save(&mut d)
}

fn flatten_form_fields(d: &mut Document) {
    let _ = annotio::flatten_annotations(d, |sub| sub != b"Widget");
    if let Ok(c) = d.catalog_mut() {
        c.remove(b"AcroForm");
    }
}

/// Form fields become fixed page content.
pub fn flatten_form(bytes: &[u8]) -> Result<Bytes> {
    let mut d = load(bytes, None)?;
    flatten_form_fields(&mut d);
    save(&mut d)
}

pub fn has_form(bytes: &[u8]) -> bool {
    !form_fields(bytes).is_empty()
}

// ------------------------------------------------------------------ rasterising: redact & compress

fn jpeg(bmp: &render::Bitmap, quality: u8) -> Result<Vec<u8>> {
    let rgb: Vec<u8> = bmp.rgba.chunks(4).flat_map(|p| [p[0], p[1], p[2]]).collect();
    let mut out = Vec::new();
    let mut enc = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, quality);
    enc.encode(&rgb, bmp.w, bmp.h, image::ExtendedColorType::Rgb8).map_err(|e| Error::Other(e.to_string()))?;
    Ok(out)
}

/// Replaces page `index` of `d` by an image of it (page box & rotation kept).
fn image_page(d: &mut Document, index: usize, jpg: &[u8], view_box: Rect, rotate: i32) -> Result<lopdf::ObjectId> {
    let (img, _, _) = annotio::embed_image(d, jpg).ok_or_else(|| Error::Other("image".into()))?;
    let [x0, y0, x1, y1] = view_box;
    let (w, h) = (x1 - x0, y1 - y0);
    // the bitmap was rendered unrotated (view rotation = -rotate), so it covers the box 1:1
    let mut o = Ops::default();
    o.q().cm([w, 0.0, 0.0, h, x0, y0]).raw("/Im0 Do").Q();
    let page = pdfdoc::add_page(d, [w, h], o.take(), dictionary! { "XObject" => dictionary! { "Im0" => img } })?;
    {
        let pd = d.get_dictionary_mut(page)?;
        pd.set("MediaBox", pdfdoc::rect_obj(view_box));
        pd.set("Rotate", rotate as i64);
    }
    let mut order = pdfdoc::pages(d);
    order.retain(|&p| p != page);
    let old = order[index];
    order[index] = page;
    // keep links of the old page
    let links: Vec<Object> = pdfdoc::page_annots(d, old)
        .into_iter()
        .filter(|a| pdfdoc::deref(d, a).as_dict().ok().and_then(|a| a.get(b"Subtype").ok()).and_then(|s| s.as_name().ok()) == Some(b"Link"))
        .collect();
    pdfdoc::set_page_annots(d, page, links);
    pdfdoc::set_pages(d, &order)?;
    Ok(page)
}

/// True redaction: affected pages are rebuilt as images with black boxes, the text below is
/// really gone (not just covered). `by_page`: 0-based page → rects in PDF space.
pub fn apply_redactions(bytes: &[u8], by_page: &BTreeMap<usize, Vec<Rect>>, dpi: f64) -> Result<Bytes> {
    let shared = Arc::new(bytes.to_vec());
    let pdf = render::open(shared, None)?;
    let infos = render::page_infos(&pdf);
    let mut d = load(bytes, None)?;
    for (&pi, rects) in by_page {
        let Some(info) = infos.get(pi) else { continue };
        let Some(mut bmp) = render::render_at_dpi(&pdf, pi, dpi, -info.rotate) else { continue };
        let g = info.geom(-info.rotate);
        let s = dpi / 72.0;
        for r in rects {
            let v = g.rect_to_view(*r);
            let (x0, y0) = ((v[0] * s).floor().max(0.0) as u32, (v[1] * s).floor().max(0.0) as u32);
            let (x1, y1) = (((v[2] * s).ceil() as u32).min(bmp.w), ((v[3] * s).ceil() as u32).min(bmp.h));
            for y in y0..y1 {
                for x in x0..x1 {
                    let i = ((y * bmp.w + x) * 4) as usize;
                    bmp.rgba[i..i + 3].fill(0);
                }
            }
        }
        image_page(&mut d, pi, &jpeg(&bmp, 90)?, info.view_box, info.rotate)?;
    }
    pdfdoc::info_mut(&mut d).set("ModDate", pdfdoc::literal(&pdfdoc::pdf_date(crate::model::now_ms())));
    d.prune_objects();
    save(&mut d)
}

/// Redactions from the comment list (applied, then removed from the list).
pub fn redactions_by_page(annots: &[Annotation]) -> BTreeMap<usize, Vec<Rect>> {
    let mut m: BTreeMap<usize, Vec<Rect>> = BTreeMap::new();
    for a in annots {
        if let Kind::Redact { rect } = a.kind {
            m.entry(a.page as usize - 1).or_default().push(rect);
        }
    }
    m
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Compression {
    Lossless,
    Medium,
    Strong,
}

/// Compress: lossless = optimise structure; otherwise rebuild pages as JPEG.
pub fn compress(bytes: &[u8], level: Compression, progress: &dyn Fn(usize, usize)) -> Result<Bytes> {
    let out = match level {
        Compression::Lossless => {
            let mut d = load(bytes, None)?;
            d.prune_objects();
            d.delete_zero_length_streams();
            d.compress();
            save(&mut d)?
        }
        _ => {
            let (dpi, q) = if level == Compression::Medium { (150.0, 72) } else { (96.0, 55) };
            let pdf = render::open(Arc::new(bytes.to_vec()), None)?;
            let infos = render::page_infos(&pdf);
            let mut d = pdfdoc::new_document();
            for (i, info) in infos.iter().enumerate() {
                let bmp = render::render_at_dpi(&pdf, i, dpi, -info.rotate).ok_or_else(|| Error::Other("render".into()))?;
                let jpg = jpeg(&bmp, q)?;
                let (img, _, _) = annotio::embed_image(&mut d, &jpg).ok_or_else(|| Error::Other("image".into()))?;
                let [x0, y0, x1, y1] = info.view_box;
                let mut o = Ops::default();
                o.q().cm([x1 - x0, 0.0, 0.0, y1 - y0, 0.0, 0.0]).raw("/Im0 Do").Q();
                let p = pdfdoc::add_page(&mut d, [x1 - x0, y1 - y0], o.take(), dictionary! { "XObject" => dictionary! { "Im0" => img } })?;
                pdfdoc::set_rotation(&mut d, p, info.rotate);
                progress(i + 1, infos.len());
            }
            save(&mut d)?
        }
    };
    Ok(if out.len() < bytes.len() { out } else { bytes.to_vec() })
}

// ------------------------------------------------------------------ export

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImageFormat {
    Png,
    Jpeg,
}

pub fn encode_bitmap(bmp: &render::Bitmap, fmt: ImageFormat) -> Result<Vec<u8>> {
    match fmt {
        ImageFormat::Jpeg => jpeg(bmp, 90),
        ImageFormat::Png => {
            let mut out = Vec::new();
            image::codecs::png::PngEncoder::new(&mut out).write_image_rgba(bmp).map_err(|e| Error::Other(e.to_string()))?;
            Ok(out)
        }
    }
}

trait PngExt {
    fn write_image_rgba(self, bmp: &render::Bitmap) -> image::ImageResult<()>;
}
impl<W: Write> PngExt for image::codecs::png::PngEncoder<W> {
    fn write_image_rgba(self, bmp: &render::Bitmap) -> image::ImageResult<()> {
        use image::ImageEncoder;
        self.write_image(&bmp.rgba, bmp.w, bmp.h, image::ExtendedColorType::Rgba8)
    }
}

/// Pages (0-based) as images in a ZIP file.
pub fn export_images(bytes: &[u8], fmt: ImageFormat, dpi: f64, pages: &[usize], progress: &dyn Fn(usize, usize)) -> Result<Bytes> {
    let pdf = render::open(Arc::new(bytes.to_vec()), None)?;
    let mut buf = std::io::Cursor::new(Vec::new());
    {
        let mut zip = zip::ZipWriter::new(&mut buf);
        let opts = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
        for (k, &p) in pages.iter().enumerate() {
            let bmp = render::render_at_dpi(&pdf, p, dpi, 0).ok_or_else(|| Error::Other("render".into()))?;
            let data = encode_bitmap(&bmp, fmt)?;
            let ext = if fmt == ImageFormat::Jpeg { "jpg" } else { "png" };
            zip.start_file(format!("Seite-{:03}.{ext}", p + 1), opts).map_err(|e| Error::Other(e.to_string()))?;
            zip.write_all(&data)?;
            progress(k + 1, pages.len());
        }
        zip.finish().map_err(|e| Error::Other(e.to_string()))?;
    }
    Ok(buf.into_inner())
}

pub fn export_text(bytes: &[u8], page_label: &str) -> Result<String> {
    let pdf = render::open(Arc::new(bytes.to_vec()), None)?;
    let n = pdf.pages().len();
    let mut parts = Vec::with_capacity(n);
    for i in 0..n {
        parts.push(format!("--- {page_label} {} ---\n{}", i + 1, render::page_text(&pdf, i).string().0));
    }
    Ok(parts.join("\n\n"))
}

/// Annotation list with pages remapped after a page operation: `map[old] = Some(new)`.
pub fn remap_annotations(annots: &[Annotation], map: &[Option<usize>]) -> Vec<Annotation> {
    annots
        .iter()
        .filter_map(|a| {
            let new = map.get(a.page as usize - 1).copied().flatten()?;
            let mut a = a.clone();
            a.page = new as u32 + 1;
            Some(a)
        })
        .collect()
}
