//! Dialogs of the reader (tools, properties, forms …) and the library's "create" actions.

use crate::app::{BrowsePurpose, Studio};
use crate::i18n::{tr, trf};
use crate::platform::IncomingFile;
use crate::reader::Reader;
use crate::signature::{self, SignaturePad};
use crate::theme::{self, Palette, SWATCHES, color_of};
use eframe::egui::{self, RichText, Sense, Stroke, vec2};
use lightpdf_core::model::{ImageKind, Kind};
use lightpdf_core::ops::{self, Compression, FieldValue, FormField, Meta, Position};

pub enum Dialog {
    ConfirmClose,
    ConfirmRedact,
    Comment { id: String, text: String },
    Signature(SignaturePad),
    StampPicker(String),
    Properties(Meta),
    Form(Vec<FormField>),
    Watermark { text: String, size: f32, color: String, opacity: f32, angle: f32 },
    PageNumbers { template: String, position: Position, size: f32, start: usize, skip_first: bool },
    Protect { pw: String, pw2: String, print: bool, copy: bool, modify: bool },
    RemovePassword,
    Compress(Compression),
    Split { every: bool, n: usize, ranges: String },
    ExportImages { png: bool, dpi: f32 },
}

fn modal<R>(ctx: &egui::Context, id: &str, title: &str, f: impl FnOnce(&mut egui::Ui) -> R) -> R {
    let w = (ctx.content_rect().width() - 40.0).min(440.0);
    egui::Modal::new(egui::Id::unique(id))
        .show(ctx, |ui| {
            ui.set_width(w);
            ui.heading(title);
            ui.add_space(6.0);
            f(ui)
        })
        .inner
}

/// OK / Cancel row. Returns Some(true) for OK.
fn buttons(ui: &mut egui::Ui, pal: &Palette, ok: &str, enabled: bool) -> Option<bool> {
    let mut r = None;
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        if ui.add_enabled(enabled, egui::Button::new(RichText::new(ok).strong().color(pal.accent_ink)).fill(pal.accent)).clicked() {
            r = Some(true);
        }
        if ui.button(tr("Abbrechen")).clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            r = Some(false);
        }
    });
    r
}

pub fn properties(r: &Reader) -> Option<Dialog> {
    ops::get_meta(&r.base).ok().map(Dialog::Properties)
}

pub fn form_dialog(r: &Reader) -> Option<Dialog> {
    Some(Dialog::Form(ops::form_fields(&r.base)))
}

/// The "Werkzeuge" menu of the reader.
pub fn tools_menu(r: &mut Reader, s: &mut Studio, ui: &mut egui::Ui) {
    let ctx = ui.ctx().clone();
    let item = |ui: &mut egui::Ui, t: &str| ui.button(t).clicked();
    if item(ui, &format!("⧉  {}", tr("Dateien anhängen"))) {
        r.current_page = r.pages.len().saturating_sub(1);
        if let Some(f) = s.pick(BrowsePurpose::InsertPdf) {
            r.files_picked(BrowsePurpose::InsertPdf, f);
        }
        ui.close();
    }
    if item(ui, &format!("✂  {}", tr("Teilen"))) {
        r.dialog = Some(Dialog::Split { every: true, n: 1, ranges: String::new() });
        ui.close();
    }
    if item(ui, &format!("🗜  {}", tr("Komprimieren"))) {
        r.dialog = Some(Dialog::Compress(Compression::Lossless));
        ui.close();
    }
    if r.encrypted {
        if item(ui, &format!("🔓  {}", tr("Kennwort entfernen"))) {
            r.dialog = Some(Dialog::RemovePassword);
            ui.close();
        }
    } else if item(ui, &format!("🔒  {}", tr("Schützen"))) {
        r.dialog = Some(Dialog::Protect { pw: String::new(), pw2: String::new(), print: true, copy: true, modify: false });
        ui.close();
    }
    if item(ui, &format!("💧  {}", tr("Wasserzeichen"))) {
        r.dialog = Some(Dialog::Watermark { text: tr("ENTWURF"), size: 60.0, color: "#c0473e".into(), opacity: 0.25, angle: 45.0 });
        ui.close();
    }
    if item(ui, &format!("#  {}", tr("Seitenzahlen"))) {
        r.dialog = Some(Dialog::PageNumbers {
            template: tr("Seite {n} von {total}"),
            position: Position::BottomCenter,
            size: 10.0,
            start: 1,
            skip_first: false,
        });
        ui.close();
    }
    ui.separator();
    if item(ui, &format!("🖼  {}", tr("Als Bilder exportieren"))) {
        r.dialog = Some(Dialog::ExportImages { png: true, dpi: 150.0 });
        ui.close();
    }
    if item(ui, &format!("🗒  {}", tr("Als Text exportieren"))) {
        let base = r.base.clone();
        let name = r.meta.name.clone();
        s.tasks.spawn(&ctx, tr("Bitte warten …"), move |_| {
            let res = ops::export_text(&base, &tr("Seite"));
            Box::new(move |s: &mut Studio| match res {
                Ok(t) => match crate::platform::save_as(&format!("{name}.txt"), t.as_bytes(), "txt") {
                    Ok(Some(_)) => s.toast(tr("Gespeichert")),
                    Ok(None) => {}
                    Err(e) => s.toast_err(e),
                },
                Err(e) => s.toast_err(e.to_string()),
            })
        });
        ui.close();
    }
    if item(ui, &format!("ℹ  {}", tr("Eigenschaften"))) {
        r.dialog = properties(r);
        ui.close();
    }
    let _ = ctx;
}

/// Shows the current reader dialog.
pub fn reader_dialog(r: &mut Reader, s: &mut Studio, ctx: &egui::Context) {
    let pal = s.pal;
    let Some(mut dialog) = r.dialog.take() else { return };
    let mut keep = true;
    match &mut dialog {
        Dialog::ConfirmClose => {
            let res = modal(ctx, "confirm-close", &tr("Änderungen speichern?"), |ui| {
                ui.label(RichText::new(&r.meta.name).color(pal.muted));
                let mut a = None;
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if theme::primary(ui, &pal, tr("Speichern")).clicked() {
                        a = Some(1);
                    }
                    if ui.button(RichText::new(tr("Nicht speichern")).color(pal.danger)).clicked() {
                        a = Some(2);
                    }
                    if ui.button(tr("Abbrechen")).clicked() {
                        a = Some(0);
                    }
                });
                a
            });
            match res {
                Some(1) => {
                    keep = false;
                    r.save(s, true);
                }
                Some(2) => {
                    keep = false;
                    r.closed = true;
                    s.screen = crate::app::Screen::Library;
                }
                Some(_) => keep = false,
                None => {}
            }
        }
        Dialog::ConfirmRedact => {
            let res = modal(ctx, "confirm-redact", &tr("Schwärzungen anwenden"), |ui| {
                ui.label(tr("Schwärzungen werden beim Anwenden endgültig in die Seiten gebrannt."));
                ui.label(
                    RichText::new(tr("Die betroffenen Seiten werden als Bild neu aufgebaut und sind danach nicht mehr durchsuchbar."))
                        .small()
                        .color(pal.muted),
                );
                buttons(ui, &pal, &tr("Schwärzen"), true)
            });
            if let Some(ok) = res {
                keep = false;
                if ok {
                    let by_page = ops::redactions_by_page(&r.annots);
                    let base = r.base.clone();
                    let rest: Vec<_> = r.annots.iter().filter(|a| !matches!(a.kind, Kind::Redact { .. })).cloned().collect();
                    s.tasks.spawn(ctx, tr("Schwärze …"), move |_| {
                        let res = ops::apply_redactions(&base, &by_page, 200.0);
                        Box::new(move |s: &mut Studio| match res {
                            Ok(b) => {
                                let ctx = s.ctx.clone();
                                if let crate::app::Screen::Reader(r) = &mut s.screen {
                                    r.set_base(&ctx, b, rest);
                                }
                                s.toast(tr("Geschwärzt"));
                            }
                            Err(e) => s.toast_err(e.to_string()),
                        })
                    });
                }
            }
        }
        Dialog::Comment { id, text } => {
            let res = modal(ctx, "comment", &tr("Kommentar"), |ui| {
                ui.add(egui::TextEdit::multiline(text).desired_rows(4).desired_width(f32::INFINITY)).request_focus();
                buttons(ui, &pal, &tr("OK"), true)
            });
            if let Some(ok) = res {
                keep = false;
                if ok && let Some(mut a) = r.find(id).cloned() {
                    a.note = (!text.trim().is_empty()).then(|| text.trim().to_string());
                    r.update(a);
                }
            }
        }
        Dialog::Signature(pad) => {
            if let Some(ok) = signature::show(pad, r, s, ctx) {
                keep = false;
                if !ok && r.opts.pending_image.is_none() {
                    r.tool = crate::reader::Tool::Select;
                }
            }
        }
        Dialog::StampPicker(custom) => {
            let mut chosen: Option<(String, String)> = None;
            let res = modal(ctx, "stamps", &tr("Stempel"), |ui| {
                ui.horizontal_wrapped(|ui| {
                    for (t, c) in crate::reader::chrome_stamps() {
                        let label = tr(t).to_uppercase();
                        let b = egui::Button::new(RichText::new(&label).strong().color(color_of(c))).stroke(Stroke::new(2.0, color_of(c)));
                        if ui.add(b).clicked() {
                            chosen = Some((label, c.to_string()));
                        }
                    }
                    for t in s.storage.lib.stamps.clone() {
                        if ui.add(egui::Button::new(RichText::new(t.to_uppercase()).strong()).stroke(Stroke::new(2.0, pal.text))).clicked()
                        {
                            chosen = Some((t.clone(), "#2f6bd6".into()));
                        }
                    }
                });
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    ui.add(egui::TextEdit::singleline(custom).hint_text(tr("Eigener Stempel …")).desired_width(200.0));
                    if ui.add_enabled(!custom.trim().is_empty(), egui::Button::new(tr("OK"))).clicked() {
                        let t = custom.trim().to_string();
                        if !s.storage.lib.stamps.contains(&t) {
                            s.storage.lib.stamps.push(t.clone());
                            s.storage.save();
                        }
                        chosen = Some((t, "#2f6bd6".into()));
                    }
                });
                buttons(ui, &pal, &tr("Fertig"), false)
            });
            if let Some((t, c)) = chosen {
                keep = false;
                if let Some((png, _)) = signature::stamp_png(&t, &c) {
                    signature::use_png(r, &png, ImageKind::Stamp);
                }
            } else if res.is_some() {
                keep = false;
                if r.opts.pending_image.is_none() {
                    r.tool = crate::reader::Tool::Select;
                }
            }
        }
        Dialog::Properties(m) => {
            let res = modal(ctx, "props", &tr("Eigenschaften"), |ui| {
                egui::Grid::new("meta").num_columns(2).spacing([12.0, 8.0]).show(ui, |ui| {
                    for (label, v) in [
                        (tr("Titel"), &mut m.title),
                        (tr("Autor"), &mut m.author),
                        (tr("Thema"), &mut m.subject),
                        (tr("Stichwörter"), &mut m.keywords),
                    ] {
                        ui.label(label);
                        ui.add(egui::TextEdit::singleline(v).desired_width(240.0));
                        ui.end_row();
                    }
                    for (label, v) in [
                        (tr("Erstellt mit"), &m.creator),
                        (tr("PDF-Programm"), &m.producer),
                        (tr("Erstellt"), &m.created),
                        (tr("Geändert"), &m.modified),
                    ] {
                        if !v.is_empty() {
                            ui.label(RichText::new(label).color(pal.muted));
                            ui.label(v.as_str());
                            ui.end_row();
                        }
                    }
                    ui.label(RichText::new(tr("Seiten")).color(pal.muted));
                    ui.label(format!("{} · PDF {} · {}", m.pages, m.version, crate::storage::human_size(r.base.len() as u64)));
                    ui.end_row();
                });
                buttons(ui, &pal, &tr("Übernehmen"), true)
            });
            if let Some(ok) = res {
                keep = false;
                if ok {
                    let m = m.clone();
                    r.page_op(&mut s.tasks, ctx, tr("Bitte warten …"), move |b| ops::set_meta(b, &m));
                }
            }
        }
        Dialog::Form(fields) => {
            let res = modal(ctx, "form", &tr("Formular ausfüllen"), |ui| {
                if fields.is_empty() {
                    ui.label(tr("Dieses Dokument hat keine Formularfelder."));
                }
                egui::ScrollArea::vertical().max_height(ctx.content_rect().height() * 0.6).show(ui, |ui| {
                    egui::Grid::new("fields").num_columns(2).spacing([12.0, 10.0]).show(ui, |ui| {
                        for f in fields.iter_mut() {
                            ui.label(&f.name);
                            ui.add_enabled_ui(!f.read_only, |ui| match &mut f.value {
                                FieldValue::Text(t) => {
                                    ui.add(egui::TextEdit::singleline(t).desired_width(220.0));
                                }
                                FieldValue::Check(b) => {
                                    ui.checkbox(b, "");
                                }
                                FieldValue::Choice { value, options } => {
                                    egui::ComboBox::new(&f.name, "").selected_text(value.clone()).show_ui(ui, |ui| {
                                        for o in options.iter() {
                                            ui.selectable_value(value, o.clone(), o);
                                        }
                                    });
                                }
                            });
                            ui.end_row();
                        }
                    });
                });
                let mut flatten = false;
                let r = buttons(ui, &pal, &tr("Übernehmen"), !fields.is_empty());
                if !fields.is_empty()
                    && ui.button(tr("Formular fixieren")).on_hover_text(tr("Die Felder werden zu festem Seiteninhalt.")).clicked()
                {
                    flatten = true;
                }
                (r, flatten)
            });
            match res {
                (Some(true), _) | (_, true) => {
                    keep = false;
                    let values: Vec<(String, FieldValue)> = fields.iter().map(|f| (f.name.clone(), f.value.clone())).collect();
                    let flatten = res.1;
                    r.page_op(&mut s.tasks, ctx, tr("Bitte warten …"), move |b| {
                        let filled = ops::fill_form(b, &values)?;
                        if flatten { ops::flatten_form(&filled) } else { Ok(filled) }
                    });
                }
                (Some(false), _) => keep = false,
                _ => {}
            }
        }
        Dialog::Watermark { text, size, color, opacity, angle } => {
            let res = modal(ctx, "wm", &tr("Wasserzeichen"), |ui| {
                ui.add(egui::TextEdit::singleline(text).desired_width(f32::INFINITY));
                ui.add(egui::Slider::new(size, 12.0..=160.0).text(tr("Schriftgröße")));
                ui.add(egui::Slider::new(angle, -90.0..=90.0).text(tr("Winkel")).suffix("°"));
                ui.add(egui::Slider::new(opacity, 0.05..=1.0).text(tr("Deckkraft")));
                color_row(ui, color, &pal);
                buttons(ui, &pal, &tr("Anwenden"), !text.trim().is_empty())
            });
            if let Some(ok) = res {
                keep = false;
                if ok {
                    let o = ops::WatermarkOpts {
                        text: text.clone(),
                        size: *size as f64,
                        color: color.clone(),
                        opacity: *opacity as f64,
                        angle: *angle as f64,
                        pages: vec![],
                    };
                    r.page_op(&mut s.tasks, ctx, tr("Bitte warten …"), move |b| ops::watermark(b, &o));
                }
            }
        }
        Dialog::PageNumbers { template, position, size, start, skip_first } => {
            let res = modal(ctx, "pn", &tr("Seitenzahlen"), |ui| {
                ui.add(egui::TextEdit::singleline(template).desired_width(f32::INFINITY));
                ui.label(RichText::new("{n} {total} {date} {name}").small().color(pal.muted));
                ui.horizontal_wrapped(|ui| {
                    use Position::*;
                    for (p, l) in
                        [(TopLeft, "↖"), (TopCenter, "↑"), (TopRight, "↗"), (BottomLeft, "↙"), (BottomCenter, "↓"), (BottomRight, "↘")]
                    {
                        ui.selectable_value(position, p, l);
                    }
                });
                ui.add(egui::Slider::new(size, 6.0..=24.0).text(tr("Schriftgröße")));
                ui.add(egui::DragValue::new(start).range(1..=9999).prefix(format!("{} ", tr("Beginnen mit"))));
                ui.checkbox(skip_first, tr("Erste Seite auslassen"));
                buttons(ui, &pal, &tr("Anwenden"), true)
            });
            if let Some(ok) = res {
                keep = false;
                if ok {
                    let o = ops::StampTextOpts {
                        template: template.clone(),
                        position: *position,
                        size: *size as f64,
                        color: "#333333".into(),
                        start: *start,
                        margin: 24.0,
                        skip_first: *skip_first,
                        name: r.meta.name.clone(),
                        date: crate::reader::today_str(),
                    };
                    r.page_op(&mut s.tasks, ctx, tr("Bitte warten …"), move |b| ops::stamp_text(b, &o));
                }
            }
        }
        Dialog::Protect { pw, pw2, print, copy, modify } => {
            let res = modal(ctx, "protect", &tr("Schützen"), |ui| {
                ui.label(tr("Das Dokument wird mit AES-Verschlüsselung gespeichert und lässt sich nur mit dem Kennwort öffnen."));
                ui.add(egui::TextEdit::singleline(pw).password(true).hint_text(tr("Kennwort")).desired_width(f32::INFINITY));
                ui.add(egui::TextEdit::singleline(pw2).password(true).hint_text(tr("Kennwort wiederholen")).desired_width(f32::INFINITY));
                if !pw2.is_empty() && pw != pw2 {
                    ui.colored_label(pal.danger, tr("Die Kennwörter stimmen nicht überein."));
                }
                ui.checkbox(print, tr("Drucken erlauben"));
                ui.checkbox(copy, tr("Kopieren erlauben"));
                ui.checkbox(modify, tr("Ändern erlauben"));
                buttons(ui, &pal, &tr("Schützen"), !pw.is_empty() && pw == pw2)
            });
            if let Some(ok) = res {
                keep = false;
                if ok {
                    let o = ops::ProtectOpts {
                        user_password: pw.clone(),
                        owner_password: String::new(),
                        allow_print: *print,
                        allow_copy: *copy,
                        allow_modify: *modify,
                    };
                    let bytes = r.export_bytes(false);
                    let id = r.meta.id.clone();
                    let pwc = pw.clone();
                    s.tasks.spawn(ctx, tr("Bitte warten …"), move |_| {
                        let res = bytes.map_err(lightpdf_core::pdfdoc::Error::Other).and_then(|b| ops::protect(&b, &o));
                        Box::new(move |s: &mut Studio| match res {
                            Ok(enc) => {
                                let _ = s.storage.write_doc(&id, &enc);
                                if let crate::app::Screen::Reader(r) = &mut s.screen {
                                    r.encrypted = true;
                                    r.password = Some(pwc);
                                    r.dirty = false;
                                    if let Some(origin) = &r.meta.path {
                                        let _ = crate::platform::write_back(origin, &enc);
                                    }
                                }
                                if let Some(m) = s.storage.lib.docs.iter_mut().find(|d| d.id == id) {
                                    m.encrypted = true;
                                }
                                s.storage.save();
                                s.toast(tr("Dokument ist jetzt kennwortgeschützt"));
                            }
                            Err(e) => s.toast_err(e.to_string()),
                        })
                    });
                }
            }
        }
        Dialog::RemovePassword => {
            let res = modal(ctx, "unprotect", &tr("Kennwort entfernen"), |ui| {
                ui.label(tr("Dieses Dokument ist mit einem Kennwort geschützt. Du kannst den Schutz entfernen – danach lässt es sich ohne Kennwort öffnen."));
                buttons(ui, &pal, &tr("Kennwort entfernen"), true)
            });
            if let Some(ok) = res {
                keep = false;
                if ok {
                    r.encrypted = false;
                    r.password = None;
                    r.dirty = true;
                    if let Some(m) = s.storage.lib.docs.iter_mut().find(|d| d.id == r.meta.id) {
                        m.encrypted = false;
                    }
                    r.save(s, false);
                }
            }
        }
        Dialog::Compress(level) => {
            let res = modal(ctx, "compress", &tr("Komprimieren"), |ui| {
                ui.radio_value(level, Compression::Lossless, tr("Verlustfrei"));
                ui.radio_value(level, Compression::Medium, tr("Mittel"));
                ui.radio_value(level, Compression::Strong, tr("Stark"));
                if *level != Compression::Lossless {
                    ui.label(
                        RichText::new(tr("Alle Seiten werden als Bild neu aufgebaut, Text ist danach nicht mehr markierbar."))
                            .small()
                            .color(pal.muted),
                    );
                }
                ui.label(RichText::new(trf("Aktuelle Größe: {0}", &[&crate::storage::human_size(r.base.len() as u64)])).small());
                buttons(ui, &pal, &tr("Komprimieren"), true)
            });
            if let Some(ok) = res {
                keep = false;
                if ok {
                    let lvl = *level;
                    let before = r.base.len();
                    let base = r.base.clone();
                    let annots = r.annots.clone();
                    s.tasks.spawn(ctx, tr("Komprimiere …"), move |p| {
                        let res = ops::compress(&base, lvl, &|i, n| p.set(i, n));
                        Box::new(move |s: &mut Studio| match res {
                            Ok(b) if b.len() < before => {
                                let after = b.len();
                                let ctx = s.ctx.clone();
                                if let crate::app::Screen::Reader(r) = &mut s.screen {
                                    r.set_base(&ctx, b, annots);
                                }
                                s.toast(format!(
                                    "{} → {}",
                                    crate::storage::human_size(before as u64),
                                    crate::storage::human_size(after as u64)
                                ));
                            }
                            Ok(_) => s.toast(tr("Die Datei ist bereits optimal – keine Verkleinerung möglich.")),
                            Err(e) => s.toast_err(e.to_string()),
                        })
                    });
                }
            }
        }
        Dialog::Split { every, n, ranges } => {
            let total = r.pages.len();
            let res = modal(ctx, "split", &tr("Teilen"), |ui| {
                ui.radio_value(every, true, tr("Alle N Seiten"));
                if *every {
                    ui.add(egui::DragValue::new(n).range(1..=total.max(1)));
                }
                ui.radio_value(every, false, tr("Nach Bereichen"));
                if !*every {
                    ui.add(egui::TextEdit::singleline(ranges).hint_text("1-3, 4-6, 7-").desired_width(f32::INFINITY));
                }
                ui.label(RichText::new(tr("Die Teile werden als neue Dokumente in deiner Bibliothek abgelegt.")).small().color(pal.muted));
                buttons(ui, &pal, &tr("Teilen"), *every || !ranges.trim().is_empty())
            });
            if let Some(ok) = res {
                keep = false;
                if ok {
                    let groups = if *every {
                        (0..total).collect::<Vec<_>>().chunks((*n).max(1)).map(|c| c.to_vec()).collect()
                    } else {
                        ops::parse_ranges(ranges, total)
                    };
                    let bytes = r.export_bytes(false);
                    let name = r.meta.name.clone();
                    s.tasks.spawn(ctx, tr("Bitte warten …"), move |_| {
                        let res = bytes.map_err(lightpdf_core::pdfdoc::Error::Other).and_then(|b| ops::split(&b, &groups));
                        Box::new(move |s: &mut Studio| match res {
                            Ok(parts) => {
                                let k = parts.len();
                                for (i, p) in parts.into_iter().enumerate() {
                                    let _ = s.storage.add(&format!("{name} ({}-{k})", i + 1), &p, None);
                                }
                                s.toast(trf("{0} Dokumente erstellt", &[&k]));
                            }
                            Err(e) => s.toast_err(e.to_string()),
                        })
                    });
                }
            }
        }
        Dialog::ExportImages { png, dpi } => {
            let res = modal(ctx, "expimg", &tr("Als Bilder exportieren"), |ui| {
                ui.horizontal(|ui| {
                    ui.selectable_value(png, true, "PNG");
                    ui.selectable_value(png, false, "JPG");
                });
                ui.add(egui::Slider::new(dpi, 50.0..=300.0).text(tr("Auflösung")).suffix(" dpi"));
                ui.label(RichText::new(tr("Die Bilder werden als ZIP-Archiv gespeichert.")).small().color(pal.muted));
                buttons(ui, &pal, &tr("Exportieren"), true)
            });
            if let Some(ok) = res {
                keep = false;
                if ok {
                    let fmt = if *png { ops::ImageFormat::Png } else { ops::ImageFormat::Jpeg };
                    let dpi = *dpi as f64;
                    let bytes = r.export_bytes(true);
                    let pages: Vec<usize> = (0..r.pages.len()).collect();
                    let name = r.meta.name.clone();
                    s.tasks.spawn(ctx, tr("Exportiere …"), move |p| {
                        let res = bytes
                            .map_err(lightpdf_core::pdfdoc::Error::Other)
                            .and_then(|b| ops::export_images(&b, fmt, dpi, &pages, &|i, n| p.set(i, n)));
                        Box::new(move |s: &mut Studio| match res {
                            Ok(zip) => match crate::platform::save_as(&format!("{name}.zip"), &zip, "zip") {
                                Ok(Some(_)) => s.toast(tr("Gespeichert")),
                                Ok(None) => {}
                                Err(e) => s.toast_err(e),
                            },
                            Err(e) => s.toast_err(e.to_string()),
                        })
                    });
                }
            }
        }
    }
    if keep && r.dialog.is_none() {
        r.dialog = Some(dialog);
    }
}

fn color_row(ui: &mut egui::Ui, color: &mut String, pal: &Palette) {
    ui.horizontal_wrapped(|ui| {
        for c in SWATCHES {
            let (rect, resp) = ui.allocate_exact_size(vec2(24.0, 24.0), Sense::click());
            ui.painter().circle_filled(rect.center(), 10.0, color_of(c));
            if c.eq_ignore_ascii_case(color) {
                ui.painter().circle_stroke(rect.center(), 12.0, Stroke::new(2.0, pal.accent));
            }
            if resp.clicked() {
                *color = c.to_string();
            }
        }
    });
}

// ------------------------------------------------------------------ library: create documents

/// Merge PDFs or turn images into a PDF; the result is added to the library and opened.
pub fn create_from_files(s: &mut Studio, purpose: BrowsePurpose, files: Vec<IncomingFile>) {
    if purpose == BrowsePurpose::Merge && files.len() < 2 {
        s.toast_err(tr("Bitte mindestens zwei PDFs auswählen."));
        return;
    }
    let ctx = s.ctx.clone();
    let name = match purpose {
        BrowsePurpose::Merge => format!("{} ({})", files[0].name.trim_end_matches(".pdf"), tr("zusammengeführt")),
        _ => files.first().map(|f| f.name.rsplit_once('.').map(|x| x.0).unwrap_or(&f.name).to_string()).unwrap_or_else(|| tr("Bilder")),
    };
    s.tasks.spawn(&ctx, tr("Bitte warten …"), move |_| {
        let bytes: Vec<Vec<u8>> = files.into_iter().map(|f| f.bytes).collect();
        let res = match purpose {
            BrowsePurpose::Merge => ops::merge(&bytes),
            _ => ops::images_to_pdf(&bytes, ops::PageSize::A4, 24.0),
        };
        Box::new(move |s: &mut Studio| match res {
            Ok(pdf) => match s.storage.add(&name, &pdf, None) {
                Ok(m) => s.open_doc(m),
                Err(e) => s.toast_err(e.to_string()),
            },
            Err(lightpdf_core::pdfdoc::Error::PasswordNeeded { .. }) => {
                s.toast_err(tr("Kennwortgeschützte PDFs können nicht zusammengeführt werden."))
            }
            Err(e) => s.toast_err(e.to_string()),
        })
    });
}

pub fn create_blank(s: &mut Studio) {
    match ops::blank_pdf(1, ops::A4) {
        Ok(pdf) => match s.storage.add(&tr("Leeres Dokument"), &pdf, None) {
            Ok(m) => s.open_doc(m),
            Err(e) => s.toast_err(e.to_string()),
        },
        Err(e) => s.toast_err(e.to_string()),
    }
}
