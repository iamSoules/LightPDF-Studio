//! Comments ⇄ real PDF annotations (visible in Acrobat, Preview, Chrome …).
//!
//! Every annotation we write carries `/LPData` (JSON, same format as LightPDF Studio 2.x), so
//! it becomes an editable object again when the file is reopened.

use crate::model::{Annotation, ImageKind, Kind, Pt, Rect, hex_to_rgb};
use crate::pdfdoc::{self, Result, literal, name, reals, rect_obj, text_hex};
use crate::text;
use base64::Engine;
use lopdf::{Dictionary, Document, Object, ObjectId, Stream, dictionary};
use std::collections::HashMap;

const LP: &[u8] = b"LPData";

/// Number formatting for content streams.
pub fn n(v: f64) -> String {
    if !v.is_finite() {
        return "0".into();
    }
    let s = format!("{:.3}", v);
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s == "-0" || s.is_empty() { "0".into() } else { s.to_string() }
}

/// Small content-stream builder.
#[derive(Default)]
pub struct Ops(pub Vec<u8>);

impl Ops {
    pub fn raw(&mut self, s: &str) -> &mut Self {
        self.0.extend_from_slice(s.as_bytes());
        self.0.push(b'\n');
        self
    }
    pub fn bytes(&mut self, b: &[u8]) -> &mut Self {
        self.0.extend_from_slice(b);
        self.0.push(b'\n');
        self
    }
    pub fn q(&mut self) -> &mut Self {
        self.raw("q")
    }
    #[allow(non_snake_case)]
    pub fn Q(&mut self) -> &mut Self {
        self.raw("Q")
    }
    pub fn gs(&mut self, name: &str) -> &mut Self {
        self.raw(&format!("/{name} gs"))
    }
    pub fn fill_rgb(&mut self, c: [f64; 3]) -> &mut Self {
        self.raw(&format!("{} {} {} rg", n(c[0]), n(c[1]), n(c[2])))
    }
    pub fn stroke_rgb(&mut self, c: [f64; 3]) -> &mut Self {
        self.raw(&format!("{} {} {} RG", n(c[0]), n(c[1]), n(c[2])))
    }
    pub fn width(&mut self, w: f64) -> &mut Self {
        self.raw(&format!("{} w", n(w)))
    }
    pub fn round(&mut self) -> &mut Self {
        self.raw("1 J 1 j")
    }
    pub fn m(&mut self, p: Pt) -> &mut Self {
        self.raw(&format!("{} {} m", n(p[0]), n(p[1])))
    }
    pub fn l(&mut self, p: Pt) -> &mut Self {
        self.raw(&format!("{} {} l", n(p[0]), n(p[1])))
    }
    pub fn re(&mut self, x: f64, y: f64, w: f64, h: f64) -> &mut Self {
        self.raw(&format!("{} {} {} {} re", n(x), n(y), n(w), n(h)))
    }
    pub fn cm(&mut self, m: [f64; 6]) -> &mut Self {
        self.raw(&format!("{} {} {} {} {} {} cm", n(m[0]), n(m[1]), n(m[2]), n(m[3]), n(m[4]), n(m[5])))
    }
    pub fn ellipse(&mut self, cx: f64, cy: f64, rx: f64, ry: f64) -> &mut Self {
        const K: f64 = 0.552_284_749_8;
        let (ox, oy) = (rx * K, ry * K);
        self.raw(&format!("{} {} m", n(cx - rx), n(cy)));
        self.raw(&format!("{} {} {} {} {} {} c", n(cx - rx), n(cy + oy), n(cx - ox), n(cy + ry), n(cx), n(cy + ry)));
        self.raw(&format!("{} {} {} {} {} {} c", n(cx + ox), n(cy + ry), n(cx + rx), n(cy + oy), n(cx + rx), n(cy)));
        self.raw(&format!("{} {} {} {} {} {} c", n(cx + rx), n(cy - oy), n(cx + ox), n(cy - ry), n(cx), n(cy - ry)));
        self.raw(&format!("{} {} {} {} {} {} c", n(cx - ox), n(cy - ry), n(cx - rx), n(cy - oy), n(cx - rx), n(cy)));
        self.raw("h")
    }
    pub fn path(&mut self, pts: &[Pt]) -> &mut Self {
        for (i, p) in pts.iter().enumerate() {
            if i == 0 {
                self.m(*p)
            } else {
                self.l(*p)
            };
        }
        if pts.len() == 1 {
            self.l([pts[0][0] + 0.01, pts[0][1]]);
        }
        self
    }
    pub fn take(self) -> Vec<u8> {
        self.0
    }
}

pub fn arrow_head(from: Pt, to: Pt, width: f64) -> [Pt; 2] {
    let ang = (to[1] - from[1]).atan2(to[0] - from[0]);
    let len = (width * 4.0).max(8.0);
    let s = 0.45;
    [[to[0] - len * (ang - s).cos(), to[1] - len * (ang - s).sin()], [to[0] - len * (ang + s).cos(), to[1] - len * (ang + s).sin()]]
}

/// For text and images: content that appears upright on rotated pages. Returns the matrix
/// from visible local coordinates (0..vw, 0..vh) into page space, and vw/vh.
pub fn upright(rotation: i32, r: Rect) -> ([f64; 6], f64, f64) {
    let [x1, y1, x2, y2] = r;
    let (w, h) = (x2 - x1, y2 - y1);
    let m = match rotation.rem_euclid(360) {
        90 => [0.0, 1.0, -1.0, 0.0, x1 + w, y1],
        180 => [-1.0, 0.0, 0.0, -1.0, x1 + w, y1 + h],
        270 => [0.0, -1.0, 1.0, 0.0, x1, y1 + h],
        _ => [1.0, 0.0, 0.0, 1.0, x1, y1],
    };
    let rot = rotation % 180 != 0;
    (m, if rot { h } else { w }, if rot { w } else { h })
}

// ------------------------------------------------------------------ images

pub struct DecodedImage {
    pub w: u32,
    pub h: u32,
    pub rgba: Vec<u8>,
}

pub fn data_url_bytes(src: &str) -> Option<Vec<u8>> {
    let b64 = src.split_once(',').map(|(_, b)| b).unwrap_or(src);
    base64::engine::general_purpose::STANDARD.decode(b64.trim()).ok()
}

pub fn to_data_url(png: &[u8]) -> String {
    format!("data:image/png;base64,{}", base64::engine::general_purpose::STANDARD.encode(png))
}

pub fn decode_image(bytes: &[u8]) -> Option<DecodedImage> {
    let img = image::load_from_memory(bytes).ok()?.to_rgba8();
    Some(DecodedImage { w: img.width(), h: img.height(), rgba: img.into_raw() })
}

/// Embeds an image file (PNG/JPEG/…) as an image XObject. JPEGs are passed through,
/// everything else becomes Flate RGB + soft mask when there is transparency.
pub fn embed_image(doc: &mut Document, bytes: &[u8]) -> Option<(ObjectId, u32, u32)> {
    let is_jpeg = bytes.starts_with(&[0xFF, 0xD8]);
    let img = image::load_from_memory(bytes).ok()?;
    let (w, h) = (img.width(), img.height());
    if is_jpeg {
        let cs = if img.color().channel_count() == 1 { "DeviceGray" } else { "DeviceRGB" };
        let s = Stream::new(
            dictionary! {
                "Type" => "XObject", "Subtype" => "Image", "Width" => w as i64, "Height" => h as i64,
                "ColorSpace" => name(cs), "BitsPerComponent" => 8, "Filter" => "DCTDecode",
            },
            bytes.to_vec(),
        );
        return Some((doc.add_object(s), w, h));
    }
    let rgba = img.to_rgba8();
    let mut rgb = Vec::with_capacity((w * h * 3) as usize);
    let mut alpha = Vec::with_capacity((w * h) as usize);
    let mut has_alpha = false;
    for p in rgba.pixels() {
        rgb.extend_from_slice(&p.0[..3]);
        alpha.push(p.0[3]);
        has_alpha |= p.0[3] != 255;
    }
    let mut dict = dictionary! {
        "Type" => "XObject", "Subtype" => "Image", "Width" => w as i64, "Height" => h as i64,
        "ColorSpace" => "DeviceRGB", "BitsPerComponent" => 8,
    };
    if has_alpha {
        let mut sm = Stream::new(
            dictionary! {
                "Type" => "XObject", "Subtype" => "Image", "Width" => w as i64, "Height" => h as i64,
                "ColorSpace" => "DeviceGray", "BitsPerComponent" => 8,
            },
            alpha,
        );
        let _ = sm.compress();
        dict.set("SMask", doc.add_object(sm));
    }
    let mut s = Stream::new(dict, rgb);
    let _ = s.compress();
    Some((doc.add_object(s), w, h))
}

// ------------------------------------------------------------------ writing

/// ZapfDingbats code for single-symbol texts (check marks of "Fill & Sign").
fn dingbat(t: &str) -> Option<u8> {
    Some(match t.trim() {
        "✓" => b'3',
        "✔" => b'4',
        "✗" => b'7',
        "✘" => b'8',
        "●" => b'l',
        "■" => b'n',
        _ => return None,
    })
}

struct Ctx {
    font: ObjectId,
    font_bold: ObjectId,
    dingbats: ObjectId,
    gs: HashMap<String, ObjectId>,
    images: HashMap<String, ObjectId>,
}

fn gstate(doc: &mut Document, ctx: &mut Ctx, opacity: f64, multiply: bool) -> ObjectId {
    let key = format!("{opacity:.2}{}", if multiply { "m" } else { "" });
    if let Some(id) = ctx.gs.get(&key) {
        return *id;
    }
    let mut d = dictionary! { "Type" => "ExtGState", "CA" => opacity as f32, "ca" => opacity as f32 };
    if multiply {
        d.set("BM", "Multiply");
    }
    let id = doc.add_object(d);
    ctx.gs.insert(key, id);
    id
}

fn form_xobject(doc: &mut Document, bbox: Rect, content: Vec<u8>, resources: Dictionary) -> ObjectId {
    let mut s = Stream::new(
        dictionary! {
            "Type" => "XObject", "Subtype" => "Form", "BBox" => rect_obj(bbox), "Resources" => resources,
        },
        content,
    );
    let _ = s.compress();
    doc.add_object(s)
}

pub fn standard_font(doc: &mut Document, base: &str) -> ObjectId {
    doc.add_object(dictionary! {
        "Type" => "Font", "Subtype" => "Type1", "BaseFont" => name(base), "Encoding" => "WinAnsiEncoding",
    })
}

fn build_annot(doc: &mut Document, ctx: &mut Ctx, page: ObjectId, rotation: i32, a: &Annotation) -> Option<Dictionary> {
    let rgb = hex_to_rgb(&a.color);
    let bbox = a.bounds();
    let mut d = dictionary! {
        "Type" => "Annot",
        "Rect" => rect_obj(bbox),
        "C" => reals(&rgb),
        "CA" => a.opacity as f32,
        "F" => 4,
        "NM" => literal(&format!("lp-{}", a.id)),
        "M" => literal(&pdfdoc::pdf_date(a.created_at)),
        "P" => Object::Reference(page),
    };
    if let Some(author) = &a.author {
        d.set("T", text_hex(author));
    }
    if let Some(c) = a.contents() {
        d.set("Contents", text_hex(c));
    }
    let multiply = matches!(a.kind, Kind::Highlight { .. } | Kind::Ink { highlighter: true, .. });
    let gs = gstate(doc, ctx, a.opacity, multiply);
    let res = || dictionary! { "ExtGState" => dictionary! { "GS0" => gs } };
    let mut o = Ops::default();
    let ap: ObjectId;
    match &a.kind {
        Kind::Highlight { rects, .. } | Kind::Underline { rects, .. } | Kind::Strike { rects, .. } => {
            let mut quads = Vec::new();
            o.q().gs("GS0");
            for q in rects {
                let [x1, y1, x2, y2] = *q;
                quads.extend_from_slice(&[x1, y2, x2, y2, x1, y1, x2, y1]);
                let h = y2 - y1;
                match a.kind {
                    Kind::Highlight { .. } => {
                        o.fill_rgb(rgb).re(x1, y1, x2 - x1, h).raw("f");
                    }
                    _ => {
                        let y = if matches!(a.kind, Kind::Underline { .. }) { y1 + h * 0.08 } else { y1 + h * 0.45 };
                        o.stroke_rgb(rgb).width((h * 0.07).max(0.6)).m([x1, y]).l([x2, y]).raw("S");
                    }
                }
            }
            o.Q();
            ap = form_xobject(doc, bbox, o.take(), res());
            let sub = match a.kind {
                Kind::Highlight { .. } => "Highlight",
                Kind::Underline { .. } => "Underline",
                _ => "StrikeOut",
            };
            d.set("Subtype", sub);
            d.set("QuadPoints", reals(&quads));
        }
        Kind::Ink { paths, width, .. } => {
            o.q().gs("GS0").stroke_rgb(rgb).width(*width).round();
            for p in paths {
                o.path(p).raw("S");
            }
            o.Q();
            ap = form_xobject(doc, bbox, o.take(), res());
            d.set("Subtype", "Ink");
            d.set("InkList", Object::Array(paths.iter().map(|p| reals(&p.iter().flatten().copied().collect::<Vec<_>>())).collect()));
            d.set("BS", dictionary! { "W" => *width as f32, "S" => "S" });
        }
        Kind::Rect { rect, width, fill } | Kind::Ellipse { rect, width, fill } => {
            let [x1, y1, x2, y2] = *rect;
            let fill_rgb = fill.as_deref().map(hex_to_rgb);
            o.q().gs("GS0");
            let ellipse = matches!(a.kind, Kind::Ellipse { .. });
            let shape = |o: &mut Ops| {
                if ellipse {
                    o.ellipse((x1 + x2) / 2.0, (y1 + y2) / 2.0, (x2 - x1) / 2.0, (y2 - y1) / 2.0);
                } else {
                    o.re(x1, y1, x2 - x1, y2 - y1);
                }
            };
            if let Some(f) = fill_rgb {
                o.fill_rgb(f);
                shape(&mut o);
                o.raw("f");
            }
            o.stroke_rgb(rgb).width(*width);
            shape(&mut o);
            o.raw("S").Q();
            ap = form_xobject(doc, bbox, o.take(), res());
            d.set("Subtype", if ellipse { "Circle" } else { "Square" });
            d.set("BS", dictionary! { "W" => *width as f32, "S" => "S" });
            if let Some(f) = fill_rgb {
                d.set("IC", reals(&f));
            }
        }
        Kind::Line { from, to, width } | Kind::Arrow { from, to, width } => {
            o.q().gs("GS0").stroke_rgb(rgb).width(*width).round().m(*from).l(*to).raw("S");
            let arrow = matches!(a.kind, Kind::Arrow { .. });
            if arrow {
                let [h1, h2] = arrow_head(*from, *to, *width);
                o.m(h1).l(*to).l(h2).raw("S");
            }
            o.Q();
            ap = form_xobject(doc, bbox, o.take(), res());
            d.set("Subtype", "Line");
            d.set("L", reals(&[from[0], from[1], to[0], to[1]]));
            d.set("BS", dictionary! { "W" => *width as f32, "S" => "S" });
            d.set("LE", Object::Array(vec![name("None"), name(if arrow { "OpenArrow" } else { "None" })]));
        }
        Kind::Text { rect, text: t, font_size, bold } if dingbat(t).is_some() => {
            // ✓ ✗ ● … are not in WinAnsi: use the standard ZapfDingbats font
            let size = *font_size;
            let (m, _vw, vh) = upright(rotation, *rect);
            let code = dingbat(t).unwrap();
            o.q().gs("GS0").cm(m).raw("BT").fill_rgb(rgb).raw(&format!("/F1 {} Tf", n(size)));
            o.raw(&format!("2 {} Td", n(vh - 2.0 - size * 0.9)));
            let mut b = text::pdf_literal(&[code]);
            b.extend_from_slice(b" Tj");
            o.bytes(&b).raw("ET").Q();
            let mut r = res();
            r.set("Font", dictionary! { "F1" => ctx.dingbats });
            ap = form_xobject(doc, *rect, o.take(), r);
            let _ = bold;
            d.set("Subtype", "FreeText");
            d.set("DA", literal(&format!("/ZaDb {} Tf {} {} {} rg", n(size), n(rgb[0]), n(rgb[1]), n(rgb[2]))));
            d.set("BS", dictionary! { "W" => 0 });
        }
        Kind::Text { rect, text: t, font_size, bold } => {
            let size = *font_size;
            let (m, vw, vh) = upright(rotation, *rect);
            let lines = text::wrap_text(&text::safe_text(t), size, *bold, (vw - 4.0).max(10.0));
            let lh = size * 1.22;
            o.q().gs("GS0").cm(m).raw("BT").fill_rgb(rgb).raw(&format!("/F1 {} Tf", n(size)));
            o.raw(&format!("2 {} Td", n(vh - 2.0 - size * 0.9)));
            for (i, ln) in lines.iter().enumerate() {
                if i > 0 {
                    o.raw(&format!("0 {} Td", n(-lh)));
                }
                let mut b = text::pdf_literal(&text::encode_win_ansi(ln));
                b.extend_from_slice(b" Tj");
                o.bytes(&b);
            }
            o.raw("ET").Q();
            let font = if *bold { ctx.font_bold } else { ctx.font };
            let mut r = res();
            r.set("Font", dictionary! { "F1" => font });
            ap = form_xobject(doc, *rect, o.take(), r);
            let da = format!("/Helv {} Tf {} {} {} rg", n(size), n(rgb[0]), n(rgb[1]), n(rgb[2]));
            d.set("Subtype", "FreeText");
            d.set("DA", literal(&da));
            d.set("Q", 0);
            d.set("BS", dictionary! { "W" => 0 });
            if rotation != 0 {
                d.set("Rotate", rotation as i64);
            }
        }
        Kind::Note { at, .. } => {
            let [x, y] = *at;
            let shape: [Pt; 7] = [
                [x + 1.0, y - 1.0],
                [x + 19.0, y - 1.0],
                [x + 19.0, y - 15.0],
                [x + 9.0, y - 15.0],
                [x + 5.0, y - 19.0],
                [x + 5.0, y - 15.0],
                [x + 1.0, y - 15.0],
            ];
            o.q().fill_rgb(rgb).stroke_rgb([rgb[0] * 0.6, rgb[1] * 0.6, rgb[2] * 0.6]).width(0.8);
            o.path(&shape).raw("h B");
            // three lines of "text"
            o.stroke_rgb([1.0, 1.0, 1.0]).width(1.0);
            for i in 0..3 {
                let yy = y - 5.0 - i as f64 * 3.2;
                o.m([x + 4.5, yy]).l([x + 15.5 - if i == 2 { 4.0 } else { 0.0 }, yy]).raw("S");
            }
            o.Q();
            ap = form_xobject(doc, bbox, o.take(), Dictionary::new());
            d.set("Subtype", "Text");
            d.set("Name", "Comment");
            d.set("Open", false);
        }
        Kind::Image { rect, kind, .. } => {
            let img = *ctx.images.get(&a.id)?;
            let (m, vw, vh) = upright(rotation, *rect);
            o.q().gs("GS0").cm(m).cm([vw, 0.0, 0.0, vh, 0.0, 0.0]).raw("/Im0 Do").Q();
            let mut r = res();
            r.set("XObject", dictionary! { "Im0" => img });
            ap = form_xobject(doc, *rect, o.take(), r);
            d.set("Subtype", "Stamp");
            d.set("Name", if *kind == ImageKind::Signature { "Signed" } else { "Draft" });
        }
        Kind::Redact { rect } => {
            let [x1, y1, x2, y2] = *rect;
            o.q().stroke_rgb([0.85, 0.1, 0.1]).width(1.0).re(x1, y1, x2 - x1, y2 - y1).raw("S").Q();
            ap = form_xobject(doc, *rect, o.take(), Dictionary::new());
            d.set("Subtype", "Redact");
            d.set("QuadPoints", reals(&[x1, y2, x2, y2, x1, y1, x2, y1]));
            d.set("IC", reals(&[0.0, 0.0, 0.0]));
        }
    }
    d.set("AP", dictionary! { "N" => ap });
    let json = serde_json::to_string(a).ok()?;
    d.set(LP, text_hex(&json));
    Some(d)
}

/// Writes the comments as real annotations into the PDF.
pub fn write_annotations(doc: &mut Document, annots: &[Annotation]) -> Result<()> {
    if annots.is_empty() {
        return Ok(());
    }
    let mut ctx = Ctx {
        font: standard_font(doc, "Helvetica"),
        font_bold: standard_font(doc, "Helvetica-Bold"),
        dingbats: doc.add_object(dictionary! { "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "ZapfDingbats" }),
        gs: HashMap::new(),
        images: HashMap::new(),
    };
    for a in annots {
        if let Kind::Image { src, .. } = &a.kind
            && let Some(bytes) = data_url_bytes(src)
            && let Some((id, _, _)) = embed_image(doc, &bytes)
        {
            ctx.images.insert(a.id.clone(), id);
        }
    }
    let pages = pdfdoc::pages(doc);
    for a in annots {
        let Some(&page) = pages.get(a.page.saturating_sub(1) as usize) else { continue };
        let rot = pdfdoc::rotation(doc, page);
        let Some(dict) = build_annot(doc, &mut ctx, page, rot, a) else { continue };
        let id = doc.add_object(dict);
        let mut list = pdfdoc::page_annots(doc, page);
        list.push(Object::Reference(id));
        pdfdoc::set_page_annots(doc, page, list);
    }
    Ok(())
}

/// Reads our own annotations from the PDF and removes them there (they become editable again).
pub fn extract_own_annotations(doc: &mut Document) -> Vec<Annotation> {
    let mut out = Vec::new();
    for (idx, page) in pdfdoc::pages(doc).into_iter().enumerate() {
        let list = pdfdoc::page_annots(doc, page);
        if list.is_empty() {
            continue;
        }
        let mut keep = Vec::new();
        let mut removed = Vec::new();
        for entry in list {
            let parsed = (|| {
                let d = pdfdoc::deref(doc, &entry).as_dict().ok()?;
                let json = pdfdoc::decode_text(doc, d.get(LP).ok()?)?;
                let mut a: Annotation = serde_json::from_str(&json).ok()?;
                a.page = idx as u32 + 1;
                // Edited in another program? Take over the text.
                if let Some(t) = d.get(b"Contents").ok().and_then(|c| pdfdoc::decode_text(doc, c)) {
                    let written = a.contents().unwrap_or("").to_string();
                    if t != written {
                        match &mut a.kind {
                            Kind::Note { text, .. } | Kind::Text { text, .. } => *text = t,
                            _ => a.note = if t.is_empty() { None } else { Some(t) },
                        }
                    }
                }
                Some(a)
            })();
            match parsed {
                Some(a) => {
                    out.push(a);
                    if let Object::Reference(r) = entry {
                        removed.push(r);
                    }
                }
                None => keep.push(entry),
            }
        }
        for r in removed {
            doc.objects.remove(&r);
        }
        pdfdoc::set_page_annots(doc, page, keep);
    }
    if !out.is_empty() {
        doc.prune_objects();
    }
    out
}

/// Bakes annotation appearances into the page content.
/// `keep` decides per subtype which annotations stay interactive (e.g. links).
pub fn flatten_annotations(doc: &mut Document, keep: impl Fn(&[u8]) -> bool) -> Result<()> {
    for page in pdfdoc::pages(doc) {
        let list = pdfdoc::page_annots(doc, page);
        if list.is_empty() {
            continue;
        }
        let mut kept = Vec::new();
        let mut content = Ops::default();
        let mut draws = Vec::new();
        for entry in list {
            let Ok(a) = pdfdoc::deref(doc, &entry).as_dict() else { continue };
            let sub = a.get(b"Subtype").and_then(Object::as_name).unwrap_or(b"");
            if keep(sub) {
                kept.push(entry.clone());
                continue;
            }
            let flags = a.get(b"F").ok().and_then(pdfdoc::as_f64).unwrap_or(0.0) as i64;
            if flags & 2 != 0 || flags & 32 != 0 {
                continue; // hidden / no view
            }
            let Some(xo) = appearance_stream(doc, a) else { continue };
            let Some(rect) = a.get(b"Rect").ok().and_then(|r| pdfdoc::num_array(doc, r)).filter(|v| v.len() == 4) else {
                continue;
            };
            let rect = [rect[0].min(rect[2]), rect[1].min(rect[3]), rect[0].max(rect[2]), rect[1].max(rect[3])];
            let Ok(xs) = doc.get_object(xo).and_then(Object::as_stream) else { continue };
            let bb = xs.dict.get(b"BBox").ok().and_then(|b| pdfdoc::num_array(doc, b)).filter(|v| v.len() == 4);
            let bb = bb.map(|b| [b[0].min(b[2]), b[1].min(b[3]), b[0].max(b[2]), b[1].max(b[3])]).unwrap_or(rect);
            let mtx = xs.dict.get(b"Matrix").ok().and_then(|m| pdfdoc::num_array(doc, m)).filter(|v| v.len() == 6);
            // Transformed bbox (PDF 32000 12.5.5)
            let bb = match mtx {
                Some(m) => {
                    let pts = [[bb[0], bb[1]], [bb[2], bb[1]], [bb[0], bb[3]], [bb[2], bb[3]]]
                        .map(|p| [m[0] * p[0] + m[2] * p[1] + m[4], m[1] * p[0] + m[3] * p[1] + m[5]]);
                    crate::model::points_bounds(pts)
                }
                None => bb,
            };
            let sx = (rect[2] - rect[0]) / (bb[2] - bb[0]).max(1e-6);
            let sy = (rect[3] - rect[1]) / (bb[3] - bb[1]).max(1e-6);
            draws.push((xo, [sx, 0.0, 0.0, sy, rect[0] - bb[0] * sx, rect[1] - bb[1] * sy]));
        }
        for (xo, m) in draws {
            let nm = pdfdoc::add_resource(doc, page, "XObject", "LPFlat", Object::Reference(xo))?;
            content.q().cm(m).raw(&format!("/{nm} Do")).Q();
        }
        if !content.0.is_empty() {
            pdfdoc::append_content(doc, page, content.take())?;
        }
        pdfdoc::set_page_annots(doc, page, kept);
    }
    Ok(())
}

/// The normal appearance of an annotation (respecting /AS for state dictionaries).
fn appearance_stream(doc: &Document, a: &Dictionary) -> Option<ObjectId> {
    let ap = pdfdoc::deref(doc, a.get(b"AP").ok()?).as_dict().ok()?;
    let n = ap.get(b"N").ok()?;
    match n {
        Object::Reference(r) => match doc.get_object(*r).ok()? {
            Object::Stream(_) => Some(*r),
            Object::Dictionary(states) => {
                let st = a.get(b"AS").ok()?.as_name().ok()?;
                states.get(st).ok()?.as_reference().ok()
            }
            _ => None,
        },
        Object::Dictionary(states) => {
            let st = a.get(b"AS").ok()?.as_name().ok()?;
            states.get(st).ok()?.as_reference().ok()
        }
        _ => None,
    }
}
