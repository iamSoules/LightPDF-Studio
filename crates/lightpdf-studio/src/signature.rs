//! Signatures (draw / type / image, saved for reuse) and stamps.
//!
//! Strokes and stamp texts are turned into transparent PNGs by drawing them into a tiny PDF
//! and rendering it with the same pure-Rust renderer as the documents (white → transparent).

use crate::app::Studio;
use crate::i18n::tr;
use crate::reader::{PendingImage, Reader, Tool};
use crate::theme::{Palette, color_of};
use eframe::egui::{self, Color32, Pos2, RichText, Sense, Stroke, TextureHandle, pos2, vec2};
use lightpdf_core::annotio::{Ops, n};
use lightpdf_core::model::{ImageKind, hex_to_rgb};
use lightpdf_core::{pdfdoc, render};
use std::sync::Arc;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Draw,
    Type,
    Saved,
}

pub struct SignaturePad {
    pub initials: bool,
    mode: Mode,
    strokes: Vec<Vec<Pos2>>,
    typed: String,
    color: &'static str,
    save: bool,
    saved_tex: Vec<(String, TextureHandle, f32)>,
    loaded: bool,
}

const INK: [&str; 3] = ["#1f1f1f", "#2f4fb0", "#c0473e"];

impl SignaturePad {
    pub fn new(initials: bool) -> Self {
        Self {
            initials,
            mode: Mode::Draw,
            strokes: vec![],
            typed: String::new(),
            color: INK[0],
            save: true,
            saved_tex: vec![],
            loaded: false,
        }
    }
}

/// Renders PDF drawing operators (in a w×h point box) to a transparent PNG; `rgb` is the ink.
pub fn rasterize(ops: Vec<u8>, w: f64, h: f64, scale: f64, rgb: [f64; 3], resources: lightpdf_core::lopdf::Dictionary) -> Option<Vec<u8>> {
    let mut d = pdfdoc::new_document();
    pdfdoc::add_page(&mut d, [w, h], ops, resources).ok()?;
    let bytes = pdfdoc::save(&mut d).ok()?;
    let pdf = render::open(Arc::new(bytes), None).ok()?;
    let bmp = render::render_at_dpi(&pdf, 0, 72.0 * scale, 0)?;
    let mut img = image::RgbaImage::new(bmp.w, bmp.h);
    for (i, px) in img.pixels_mut().enumerate() {
        let p = &bmp.rgba[i * 4..i * 4 + 3];
        // coverage from darkness relative to white
        let a = 255 - *p.iter().min().unwrap();
        *px = image::Rgba([(rgb[0] * 255.0) as u8, (rgb[1] * 255.0) as u8, (rgb[2] * 255.0) as u8, a]);
    }
    let img = crop_alpha(img);
    let mut out = std::io::Cursor::new(Vec::new());
    img.write_to(&mut out, image::ImageFormat::Png).ok()?;
    Some(out.into_inner())
}

fn crop_alpha(img: image::RgbaImage) -> image::RgbaImage {
    let (w, h) = img.dimensions();
    let (mut x0, mut y0, mut x1, mut y1) = (w, h, 0, 0);
    for (x, y, p) in img.enumerate_pixels() {
        if p[3] > 8 {
            x0 = x0.min(x);
            y0 = y0.min(y);
            x1 = x1.max(x);
            y1 = y1.max(y);
        }
    }
    if x1 < x0 || y1 < y0 {
        return img;
    }
    let pad = 4;
    let (x0, y0) = (x0.saturating_sub(pad), y0.saturating_sub(pad));
    let (x1, y1) = ((x1 + pad).min(w - 1), (y1 + pad).min(h - 1));
    image::imageops::crop_imm(&img, x0, y0, x1 - x0 + 1, y1 - y0 + 1).to_image()
}

/// Strokes (in a w×h box, y down) → PNG.
pub fn strokes_png(strokes: &[Vec<Pos2>], w: f32, h: f32, color: &str) -> Option<Vec<u8>> {
    let mut o = Ops::default();
    o.raw("0 0 0 RG 1 J 1 j").width(2.6);
    for s in strokes {
        let pts: Vec<[f64; 2]> = s.iter().map(|p| [p.x as f64, (h - p.y) as f64]).collect();
        o.path(&pts).raw("S");
    }
    rasterize(o.take(), w as f64, h as f64, 3.0, hex_to_rgb(color), lightpdf_core::lopdf::Dictionary::new())
}

fn text_png(text: &str, font: &str, size: f64, color: &str, boxed: bool) -> Option<(Vec<u8>, f64)> {
    let t = lightpdf_core::text::safe_text(text);
    let bold = font.contains("Bold");
    let tw = lightpdf_core::text::text_width(&t, size, bold);
    let (pad_x, pad_y) = if boxed { (size * 0.6, size * 0.45) } else { (4.0, 4.0) };
    let (w, h) = (tw + 2.0 * pad_x, size * 1.2 + 2.0 * pad_y);
    let mut o = Ops::default();
    if boxed {
        let lw = size * 0.12;
        let r = size * 0.3;
        let (x0, y0, x1, y1) = (lw, lw, w - lw, h - lw);
        o.raw("0 0 0 RG").width(lw);
        // rounded rectangle
        o.raw(&format!("{} {} m", n(x0 + r), n(y0)));
        o.raw(&format!("{} {} l {} {} {} {} {} {} c", n(x1 - r), n(y0), n(x1), n(y0), n(x1), n(y0), n(x1), n(y0 + r)));
        o.raw(&format!("{} {} l {} {} {} {} {} {} c", n(x1), n(y1 - r), n(x1), n(y1), n(x1), n(y1), n(x1 - r), n(y1)));
        o.raw(&format!("{} {} l {} {} {} {} {} {} c", n(x0 + r), n(y1), n(x0), n(y1), n(x0), n(y1), n(x0), n(y1 - r)));
        o.raw(&format!("{} {} l {} {} {} {} {} {} c h S", n(x0), n(y0 + r), n(x0), n(y0), n(x0), n(y0), n(x0 + r), n(y0)));
    }
    o.raw("BT 0 g").raw(&format!("/F1 {} Tf {} {} Td", n(size), n(pad_x), n(pad_y + size * 0.22)));
    let mut b = lightpdf_core::text::pdf_literal(&lightpdf_core::text::encode_win_ansi(&t));
    b.extend_from_slice(b" Tj ET");
    o.bytes(&b);
    let font_dict = lightpdf_core::lopdf::dictionary! { "Type" => "Font", "Subtype" => "Type1", "BaseFont" => pdfdoc::name(font), "Encoding" => "WinAnsiEncoding" };
    let res = lightpdf_core::lopdf::dictionary! { "Font" => lightpdf_core::lopdf::dictionary! { "F1" => font_dict } };
    let png = rasterize(o.take(), w, h, 4.0, hex_to_rgb(color), res)?;
    Some((png, w / h))
}

/// A stamp ("GENEHMIGT", custom text …) as PNG.
pub fn stamp_png(text: &str, color: &str) -> Option<(Vec<u8>, f64)> {
    text_png(&text.to_uppercase(), "Helvetica-Bold", 28.0, color, true)
}

fn png_aspect(png: &[u8]) -> f64 {
    image::load_from_memory(png).map(|i| i.width() as f64 / i.height().max(1) as f64).unwrap_or(3.0)
}

/// Removes a (near-)white background from a photo of a signature.
pub fn remove_white(bytes: &[u8]) -> Option<Vec<u8>> {
    let img = image::load_from_memory(bytes).ok()?.to_rgba8();
    let (w, h) = img.dimensions();
    let k = (800.0 / w.max(h) as f32).min(1.0);
    let mut img = if k < 1.0 {
        image::imageops::resize(&img, (w as f32 * k) as u32, (h as f32 * k) as u32, image::imageops::FilterType::Triangle)
    } else {
        img
    };
    for p in img.pixels_mut() {
        let l = (p[0] as u32 + p[1] as u32 + p[2] as u32) / 3;
        let a = if l > 200 {
            0
        } else if l < 140 {
            255
        } else {
            ((200 - l) * 255 / 60) as u8 as u32
        };
        p[3] = (a.min(p[3] as u32)) as u8;
    }
    let img = crop_alpha(img);
    let mut out = std::io::Cursor::new(Vec::new());
    img.write_to(&mut out, image::ImageFormat::Png).ok()?;
    Some(out.into_inner())
}

pub fn use_png(r: &mut Reader, png: &[u8], kind: ImageKind) {
    let aspect = png_aspect(png);
    let src: Arc<str> = lightpdf_core::annotio::to_data_url(png).into();
    r.opts.pending_image = Some(PendingImage { src, kind, aspect });
    r.tool = match kind {
        ImageKind::Stamp => Tool::Stamp,
        ImageKind::Signature => Tool::Signature,
        ImageKind::Image => Tool::Image,
    };
}

/// Shows the pad. Returns `Some(true)` when a signature was chosen, `Some(false)` on cancel.
pub fn show(pad: &mut SignaturePad, r: &mut Reader, s: &mut Studio, ctx: &egui::Context) -> Option<bool> {
    let pal: Palette = s.pal;
    if !pad.loaded {
        pad.loaded = true;
        for sig in s.storage.lib.signatures.iter().filter(|x| x.initials == pad.initials) {
            if let Some(img) = std::fs::read(s.storage.signature_path(&sig.id)).ok().and_then(|b| image::load_from_memory(&b).ok()) {
                let rgba = img.to_rgba8();
                let aspect = rgba.width() as f32 / rgba.height().max(1) as f32;
                let t = ctx.load_texture(
                    format!("sig{}", sig.id),
                    egui::ColorImage::from_rgba_unmultiplied([rgba.width() as usize, rgba.height() as usize], &rgba),
                    egui::TextureOptions::LINEAR,
                );
                pad.saved_tex.push((sig.id.clone(), t, aspect));
            }
        }
        if !pad.saved_tex.is_empty() {
            pad.mode = Mode::Saved;
        }
    }
    let mut result = None;
    let screen = ctx.content_rect();
    egui::Modal::new(egui::Id::unique("sigpad")).show(ctx, |ui| {
        let width = (screen.width() - 40.0).min(560.0);
        ui.set_width(width);
        ui.heading(if pad.initials { tr("Initialen") } else { tr("Unterschrift") });
        ui.horizontal(|ui| {
            ui.selectable_value(&mut pad.mode, Mode::Draw, format!("✏ {}", tr("Zeichnen")));
            ui.selectable_value(&mut pad.mode, Mode::Type, format!("⌨ {}", tr("Tippen")));
            if !pad.saved_tex.is_empty() {
                ui.selectable_value(&mut pad.mode, Mode::Saved, format!("★ {}", tr("Gespeicherte")));
            }
            if ui.button(format!("🖼 {}", tr("Bild"))).clicked()
                && let Some(f) = s.pick(crate::app::BrowsePurpose::Images).and_then(|f| f.into_iter().next())
                && let Some(png) = remove_white(&f.bytes)
            {
                if pad.save {
                    let _ = s.storage.add_signature(&png, pad.initials);
                }
                use_png(r, &png, ImageKind::Signature);
                result = Some(true);
            }
        });
        ui.add_space(6.0);
        let h = if pad.initials { 140.0 } else { 190.0 };
        match pad.mode {
            Mode::Draw => {
                let (rect, resp) = ui.allocate_exact_size(vec2(width, h), Sense::drag());
                let p = ui.painter_at(rect);
                p.rect_filled(rect, 10.0, Color32::WHITE);
                p.rect_stroke(rect, 10.0, Stroke::new(1.0, pal.line), egui::StrokeKind::Inside);
                p.line_segment(
                    [pos2(rect.min.x + 24.0, rect.max.y - 40.0), pos2(rect.max.x - 24.0, rect.max.y - 40.0)],
                    Stroke::new(1.0, Color32::from_gray(200)),
                );
                p.text(
                    rect.min + vec2(10.0, 8.0),
                    egui::Align2::LEFT_TOP,
                    tr("Zeichne deine Unterschrift"),
                    egui::FontId::proportional(12.0),
                    Color32::from_gray(150),
                );
                if resp.drag_started() {
                    pad.strokes.push(vec![]);
                }
                if resp.dragged()
                    && let (Some(pos), Some(st)) = (resp.interact_pointer_pos(), pad.strokes.last_mut())
                {
                    let local = (pos - rect.min).to_pos2();
                    if st.last().is_none_or(|l| l.distance(local) > 0.8) {
                        st.push(local);
                    }
                }
                let col = color_of(pad.color);
                for st in &pad.strokes {
                    let pts: Vec<Pos2> = st.iter().map(|q| rect.min + q.to_vec2()).collect();
                    if pts.len() > 1 {
                        p.add(egui::Shape::line(pts, Stroke::new(2.6, col)));
                    } else if let Some(q) = pts.first() {
                        p.circle_filled(*q, 1.3, col);
                    }
                }
            }
            Mode::Type => {
                ui.add(
                    egui::TextEdit::singleline(&mut pad.typed)
                        .hint_text(tr("Name"))
                        .font(egui::FontId::proportional(28.0))
                        .desired_width(f32::INFINITY),
                );
                ui.label(RichText::new(&pad.typed).italics().size(34.0).color(color_of(pad.color)));
            }
            Mode::Saved => {
                let mut delete = None;
                ui.horizontal_wrapped(|ui| {
                    for (id, t, aspect) in &pad.saved_tex {
                        let hh = 70.0;
                        let (rect, resp) = ui.allocate_exact_size(vec2((hh * aspect).clamp(60.0, 260.0), hh), Sense::click());
                        ui.painter().rect_filled(rect, 8.0, Color32::WHITE);
                        ui.painter().image(
                            t.id(),
                            rect.shrink(6.0),
                            egui::Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                            Color32::WHITE,
                        );
                        let hot = resp.hovered();
                        ui.painter().rect_stroke(
                            rect,
                            8.0,
                            Stroke::new(if hot { 2.0 } else { 1.0 }, if hot { pal.accent } else { pal.line }),
                            egui::StrokeKind::Inside,
                        );
                        if resp.clicked()
                            && let Ok(png) = std::fs::read(s.storage.signature_path(id))
                        {
                            use_png(r, &png, ImageKind::Signature);
                            result = Some(true);
                        }
                        resp.context_menu(|ui| {
                            if ui.button(tr("Löschen")).clicked() {
                                delete = Some(id.clone());
                                ui.close();
                            }
                        });
                    }
                });
                if let Some(id) = delete {
                    s.storage.delete_signature(&id);
                    pad.saved_tex.retain(|x| x.0 != id);
                }
            }
        }
        if pad.mode != Mode::Saved {
            ui.horizontal(|ui| {
                for c in INK {
                    let (rect, rr) = ui.allocate_exact_size(vec2(26.0, 26.0), Sense::click());
                    ui.painter().circle_filled(rect.center(), 10.0, color_of(c));
                    if pad.color == c {
                        ui.painter().circle_stroke(rect.center(), 12.0, Stroke::new(2.0, pal.accent));
                    }
                    if rr.clicked() {
                        pad.color = c;
                    }
                }
                ui.checkbox(&mut pad.save, tr("Speichern"));
                if ui.button(tr("Löschen")).clicked() {
                    pad.strokes.clear();
                    pad.typed.clear();
                }
            });
        }
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            let ready = match pad.mode {
                Mode::Draw => pad.strokes.iter().any(|s| !s.is_empty()),
                Mode::Type => !pad.typed.trim().is_empty(),
                Mode::Saved => false,
            };
            if ui
                .add_enabled(ready, egui::Button::new(RichText::new(tr("Übernehmen")).strong().color(pal.accent_ink)).fill(pal.accent))
                .clicked()
            {
                let png = match pad.mode {
                    Mode::Draw => strokes_png(&pad.strokes, width, h, pad.color),
                    Mode::Type => text_png(pad.typed.trim(), "Helvetica-Oblique", 40.0, pad.color, false).map(|x| x.0),
                    Mode::Saved => None,
                };
                if let Some(png) = png {
                    if pad.save {
                        let _ = s.storage.add_signature(&png, pad.initials);
                    }
                    use_png(r, &png, ImageKind::Signature);
                    result = Some(true);
                }
            }
            if ui.button(tr("Abbrechen")).clicked() {
                result = Some(false);
            }
        });
    });
    result
}
