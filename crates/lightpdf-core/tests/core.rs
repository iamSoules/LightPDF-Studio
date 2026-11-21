//! End-to-end tests of the PDF engine (port of v2's tests/pdfops.test.ts, plus more).

use lightpdf_core::model::{Annotation, ImageKind, Kind};
use lightpdf_core::ops::{self, Compression, ExportOpts, Position};
use lightpdf_core::pdfdoc::{self, Error};
use lightpdf_core::render::{self, RenderPool};
use lopdf::{Object, Stream, dictionary};
use std::collections::BTreeMap;
use std::sync::Arc;

/// A PDF with `pages` pages of Helvetica text, optionally rotated, with an outline and a link.
fn fixture(pages: usize, rotate: i64) -> Vec<u8> {
    let mut d = pdfdoc::new_document();
    let font =
        d.add_object(dictionary! { "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica", "Encoding" => "WinAnsiEncoding" });
    for i in 0..pages {
        let content = format!(
            "BT /F1 24 Tf 72 720 Td (Page {} Hello World) Tj ET\nBT /F1 12 Tf 72 680 Td (The quick brown fox jumps over the lazy dog.) Tj ET\n0 0 1 rg 72 600 200 40 re f",
            i + 1
        );
        let p =
            pdfdoc::add_page(&mut d, [612.0, 792.0], content.into_bytes(), dictionary! { "Font" => dictionary! { "F1" => font } }).unwrap();
        if rotate != 0 {
            d.get_dictionary_mut(p).unwrap().set("Rotate", rotate);
        }
    }
    let ids = pdfdoc::pages(&d);
    // link on page 1 → last page
    let link = d.add_object(dictionary! {
        "Type" => "Annot", "Subtype" => "Link", "Rect" => vec![72.into(), 700.into(), 300.into(), 750.into()],
        "Dest" => vec![Object::Reference(*ids.last().unwrap()), "Fit".into()],
    });
    pdfdoc::set_page_annots(&mut d, ids[0], vec![Object::Reference(link)]);
    // outline
    let outlines = d.new_object_id();
    let item = d.add_object(dictionary! {
        "Title" => pdfdoc::text_hex("Kapitel Ä"), "Parent" => outlines,
        "Dest" => vec![Object::Reference(*ids.last().unwrap()), "Fit".into()],
    });
    d.objects.insert(outlines, Object::Dictionary(dictionary! { "Type" => "Outlines", "First" => item, "Last" => item, "Count" => 1 }));
    d.catalog_mut().unwrap().set("Outlines", outlines);
    pdfdoc::info_mut(&mut d).set("Title", pdfdoc::text_hex("Testdokument"));
    pdfdoc::save(&mut d).unwrap()
}

fn page_count(b: &[u8]) -> usize {
    pdfdoc::pages(&pdfdoc::load(b, None).unwrap()).len()
}

fn text_of(b: &[u8], page: usize) -> String {
    let pdf = render::open(Arc::new(b.to_vec()), None).unwrap();
    render::page_text(&pdf, page).string().0
}

fn ann(kind: Kind, page: u32) -> Annotation {
    Annotation {
        id: lightpdf_core::model::uid(),
        page,
        color: "#e5484d".into(),
        opacity: 0.9,
        created_at: 1_731_000_000_000.0,
        author: Some("Prüfer".into()),
        note: None,
        kind,
    }
}

fn png_1x1() -> Arc<str> {
    let mut img = image::RgbaImage::new(4, 2);
    img.put_pixel(0, 0, image::Rgba([0, 0, 0, 255]));
    let mut out = std::io::Cursor::new(Vec::new());
    img.write_to(&mut out, image::ImageFormat::Png).unwrap();
    lightpdf_core::annotio::to_data_url(out.get_ref()).into()
}

fn all_kinds() -> Vec<Annotation> {
    vec![
        ann(Kind::Highlight { rects: vec![[72.0, 715.0, 300.0, 745.0]], text: Some("Hello".into()) }, 1),
        ann(Kind::Underline { rects: vec![[72.0, 675.0, 200.0, 690.0]], text: None }, 1),
        ann(Kind::Strike { rects: vec![[72.0, 675.0, 200.0, 690.0]], text: None }, 1),
        ann(Kind::Ink { paths: vec![vec![[100.0, 100.0], [150.0, 120.0], [200.0, 90.0]]], width: 2.0, highlighter: false }, 1),
        ann(Kind::Ink { paths: vec![vec![[100.0, 300.0], [300.0, 300.0]]], width: 10.0, highlighter: true }, 2),
        ann(Kind::Rect { rect: [300.0, 300.0, 400.0, 350.0], width: 2.0, fill: Some("#ffee00".into()) }, 1),
        ann(Kind::Ellipse { rect: [300.0, 400.0, 400.0, 450.0], width: 1.5, fill: None }, 1),
        ann(Kind::Line { from: [50.0, 50.0], to: [150.0, 60.0], width: 1.0 }, 2),
        ann(Kind::Arrow { from: [50.0, 80.0], to: [150.0, 90.0], width: 2.0 }, 2),
        ann(Kind::Text { rect: [300.0, 500.0, 500.0, 530.0], text: "Grüße aus Köln – 100 €".into(), font_size: 12.0, bold: false }, 1),
        ann(Kind::Note { at: [500.0, 700.0], text: "Notiz mit Umlauten: äöü".into() }, 2),
        ann(Kind::Image { rect: [100.0, 400.0, 200.0, 450.0], src: png_1x1(), kind: ImageKind::Signature }, 1),
        ann(Kind::Redact { rect: [72.0, 595.0, 280.0, 645.0] }, 2),
    ]
}

#[test]
fn renders_pages_and_finds_text() {
    let b = fixture(2, 0);
    let pdf = render::open(Arc::new(b.clone()), None).unwrap();
    let infos = render::page_infos(&pdf);
    assert_eq!(infos.len(), 2);
    assert_eq!(infos[0].view_box, [0.0, 0.0, 612.0, 792.0]);
    let bmp = render::render_at_dpi(&pdf, 0, 72.0, 0).unwrap();
    assert_eq!((bmp.w, bmp.h), (612, 792));
    // blue box at x 72..272, y (from top) 792-640..792-600 = 152..192
    let px = |x: u32, y: u32| &bmp.rgba[((y * bmp.w + x) * 4) as usize..][..4];
    assert_eq!(px(150, 170), &[0, 0, 255, 255]);
    assert_eq!(px(10, 10), &[255, 255, 255, 255]);
    let dark = bmp.rgba.chunks(4).filter(|p| p[0] < 100 && p[2] < 100).count();
    assert!(dark > 200, "text should be drawn ({dark} dark px)");

    let t = render::page_text(&pdf, 0);
    let (s, _) = t.string();
    assert!(s.contains("Page 1 Hello World"), "{s}");
    assert!(s.contains("quick brown fox"), "{s}");
    let hits = t.find("hello world");
    assert_eq!(hits.len(), 1);
    let rects = t.line_rects(hits[0].0, hits[0].1);
    assert_eq!(rects.len(), 1);
    let r = rects[0];
    assert!(r[0] > 140.0 && r[0] < 220.0 && r[1] > 700.0 && r[3] < 760.0 && r[2] > r[0] + 100.0, "{r:?}");
    assert_eq!(t.text_between(hits[0].0, hits[0].1), "Hello World");
}

#[test]
fn rotated_pages_render_upright_geometry() {
    let b = fixture(1, 90);
    let pdf = render::open(Arc::new(b), None).unwrap();
    let info = render::page_infos(&pdf)[0];
    assert_eq!(info.rotate, 90);
    let g = info.geom(0);
    assert_eq!((g.width(), g.height()), (792.0, 612.0));
    let bmp = render::render_at_dpi(&pdf, 0, 72.0, 0).unwrap();
    assert_eq!((bmp.w, bmp.h), (792, 612));
    // blue box: user (72..272, 600..640) → view via geom
    let v = g.rect_to_view([72.0, 600.0, 272.0, 640.0]);
    let (cx, cy) = (((v[0] + v[2]) / 2.0) as u32, ((v[1] + v[3]) / 2.0) as u32);
    assert_eq!(&bmp.rgba[((cy * bmp.w + cx) * 4) as usize..][..3], &[0, 0, 255]);
    // and with an extra view rotation, too
    let bmp2 = render::render_at_dpi(&pdf, 0, 72.0, 90).unwrap();
    let g2 = info.geom(90);
    let v = g2.rect_to_view([72.0, 600.0, 272.0, 640.0]);
    let (cx, cy) = (((v[0] + v[2]) / 2.0) as u32, ((v[1] + v[3]) / 2.0) as u32);
    assert_eq!(&bmp2.rgba[((cy * bmp2.w + cx) * 4) as usize..][..3], &[0, 0, 255]);
}

#[test]
fn annotations_roundtrip_and_stay_editable() {
    let b = fixture(2, 0);
    let annots = all_kinds();
    let out = ops::export_pdf(&b, &annots, &ExportOpts::default()).unwrap();
    // Other programs see real annotations
    let d = pdfdoc::load(&out, None).unwrap();
    let p1 = pdfdoc::pages(&d)[0];
    let subs: Vec<String> = pdfdoc::page_annots(&d, p1)
        .iter()
        .filter_map(|a| {
            pdfdoc::deref(&d, a).as_dict().ok()?.get(b"Subtype").ok()?.as_name().ok().map(|n| String::from_utf8_lossy(n).into_owned())
        })
        .collect();
    for s in ["Link", "Highlight", "Underline", "StrikeOut", "Ink", "Square", "Circle", "FreeText", "Stamp"] {
        assert!(subs.iter().any(|x| x == s), "missing {s} in {subs:?}");
    }
    // Reopen: our annotations come back as editable objects, the link stays in the PDF
    let imp = ops::import_pdf(&out, None).unwrap();
    assert_eq!(imp.annots.len(), annots.len());
    let mut annots = annots.clone();
    annots.sort_by(|a, b| a.id.cmp(&b.id));
    let mut back = imp.annots.clone();
    back.sort_by(|a, b| a.id.cmp(&b.id));
    for (a, b) in annots.iter().zip(&back) {
        assert_eq!(serde_json::to_value(a).unwrap(), serde_json::to_value(b).unwrap());
    }
    let d = pdfdoc::load(&imp.base, None).unwrap();
    assert_eq!(pdfdoc::page_annots(&d, pdfdoc::pages(&d)[0]).len(), 1, "only the link remains");
    assert!(!imp.encrypted);
}

#[test]
fn reads_annotations_written_by_v2() {
    // /LPData exactly as LightPDF Studio 2.x (pdf-lib) wrote it: UTF-16BE hex string
    let b = fixture(1, 0);
    let mut d = pdfdoc::load(&b, None).unwrap();
    let json = r##"{"id":"m3abc","page":1,"color":"#ffd60a","opacity":1,"createdAt":1731000000000,"type":"highlight","rects":[[72,715,300,745]],"text":"Hello"}"##;
    let a = d.add_object(dictionary! {
        "Type" => "Annot", "Subtype" => "Highlight", "Rect" => vec![72.into(), 715.into(), 300.into(), 745.into()],
        "LPData" => pdfdoc::text_hex(json), "Contents" => pdfdoc::text_hex("Edited in Acrobat"),
    });
    let p = pdfdoc::pages(&d)[0];
    let mut list = pdfdoc::page_annots(&d, p);
    list.push(Object::Reference(a));
    pdfdoc::set_page_annots(&mut d, p, list);
    let b = pdfdoc::save(&mut d).unwrap();
    let imp = ops::import_pdf(&b, None).unwrap();
    assert_eq!(imp.annots.len(), 1);
    assert_eq!(imp.annots[0].id, "m3abc");
    assert_eq!(imp.annots[0].note.as_deref(), Some("Edited in Acrobat"));
}

#[test]
fn display_copy_shows_foreign_annotations() {
    let b = fixture(1, 0);
    let with = ops::export_pdf(
        &b,
        &[ann(Kind::Rect { rect: [400.0, 50.0, 500.0, 150.0], width: 4.0, fill: Some("#00ff00".into()) }, 1)],
        &ExportOpts::default(),
    )
    .unwrap();
    let disp = ops::display_copy(&with).unwrap();
    let pdf = render::open(Arc::new(disp), None).unwrap();
    let bmp = render::render_at_dpi(&pdf, 0, 72.0, 0).unwrap();
    let (x, y) = (450u32, 792 - 100u32);
    let p = &bmp.rgba[((y * bmp.w + x) * 4) as usize..][..3];
    assert!(p[1] > 200 && p[0] < 80, "green fill expected, got {p:?}");
}

#[test]
fn flatten_burns_in_comments() {
    let b = fixture(1, 0);
    let out = ops::export_pdf(
        &b,
        &all_kinds().into_iter().filter(|a| a.page == 1).collect::<Vec<_>>(),
        &ExportOpts { flatten: true, password: None },
    )
    .unwrap();
    let d = pdfdoc::load(&out, None).unwrap();
    let left = pdfdoc::page_annots(&d, pdfdoc::pages(&d)[0]);
    assert_eq!(left.len(), 1, "only the link stays interactive");
    assert!(text_of(&out, 0).contains("Grüße aus Köln"));
}

#[test]
fn password_protection_roundtrip() {
    let b = fixture(2, 0);
    let enc = ops::protect(
        &b,
        &ops::ProtectOpts {
            user_password: "geheim".into(),
            owner_password: String::new(),
            allow_print: true,
            allow_copy: false,
            allow_modify: false,
        },
    )
    .unwrap();
    assert!(pdfdoc::needs_password(&enc));
    assert_eq!(pdfdoc::load(&enc, None).err(), Some(Error::PasswordNeeded { wrong: false }));
    assert_eq!(pdfdoc::load(&enc, Some("falsch")).err(), Some(Error::PasswordNeeded { wrong: true }));
    let imp = ops::import_pdf(&enc, Some("geheim")).unwrap();
    assert!(imp.encrypted);
    assert!(text_of(&imp.base, 0).contains("Hello World"));
    // hayro opens it with the password too
    assert!(render::open(Arc::new(enc.clone()), Some("geheim")).is_ok());
    assert!(matches!(render::open(Arc::new(enc.clone()), None), Err(Error::PasswordNeeded { .. })));
    // export with comments and password again
    let again = ops::export_pdf(&imp.base, &all_kinds(), &ExportOpts { password: Some("geheim".into()), flatten: false }).unwrap();
    let imp2 = ops::import_pdf(&again, Some("geheim")).unwrap();
    assert_eq!(imp2.annots.len(), all_kinds().len());
    let plain = ops::remove_password(&enc, "geheim").unwrap();
    assert!(!pdfdoc::needs_password(&plain));
}

#[test]
fn page_operations() {
    let b = fixture(3, 0);
    let r = ops::rotate_pages(&b, &[0, 2], 90).unwrap();
    let d = pdfdoc::load(&r, None).unwrap();
    let ids = pdfdoc::pages(&d);
    assert_eq!([pdfdoc::rotation(&d, ids[0]), pdfdoc::rotation(&d, ids[1]), pdfdoc::rotation(&d, ids[2])], [90, 0, 90]);

    let del = ops::delete_pages(&b, &[1]).unwrap();
    assert_eq!(page_count(&del), 2);
    assert!(text_of(&del, 1).contains("Page 3"));
    assert!(ops::delete_pages(&b, &[0, 1, 2]).is_err());

    let re = ops::reorder_pages(&b, &[2, 0, 0, 1]).unwrap();
    assert_eq!(page_count(&re), 4);
    assert!(text_of(&re, 0).contains("Page 3"));
    assert!(text_of(&re, 2).contains("Page 1"));

    let bl = ops::insert_blank(&b, 1).unwrap();
    assert_eq!(page_count(&bl), 4);
    assert!(text_of(&bl, 1).trim().is_empty());
    assert!(text_of(&bl, 2).contains("Page 2"));

    let other = fixture(2, 0);
    let ins = ops::insert_pdf(&b, &other, 3).unwrap();
    assert_eq!(page_count(&ins), 5);

    let ex = ops::extract_pages(&b, &[2]).unwrap();
    assert_eq!(page_count(&ex), 1);
    assert!(text_of(&ex, 0).contains("Page 3"));
    assert!(ex.len() < b.len() * 2);

    let m = ops::merge(&[b.clone(), other.clone(), ex.clone()]).unwrap();
    assert_eq!(page_count(&m), 6);
    assert!(text_of(&m, 5).contains("Page 3"));

    assert_eq!(ops::parse_ranges("1-2, 3, 5-", 6), vec![vec![0, 1], vec![2], vec![4, 5]]);
    assert_eq!(ops::parse_ranges("-2; 9", 3), vec![vec![0, 1], vec![2]]);
    let parts = ops::split(&b, &ops::parse_ranges("1, 2-3", 3)).unwrap();
    assert_eq!(parts.iter().map(|p| page_count(p)).collect::<Vec<_>>(), vec![1, 2]);
    assert_eq!(ops::split_every(&b, 2).unwrap().len(), 2);
}

#[test]
fn watermark_page_numbers_metadata() {
    let b = fixture(2, 90);
    let w = ops::watermark(
        &b,
        &ops::WatermarkOpts { text: "ENTWURF".into(), size: 60.0, color: "#ff0000".into(), opacity: 0.3, angle: 45.0, pages: vec![] },
    )
    .unwrap();
    assert!(text_of(&w, 1).contains("ENTWURF"));
    let s = ops::stamp_text(
        &w,
        &ops::StampTextOpts {
            template: "Seite {n} von {total}".into(),
            position: Position::BottomCenter,
            size: 10.0,
            color: "#333333".into(),
            start: 1,
            margin: 24.0,
            skip_first: false,
            name: String::new(),
            date: "1.1.2026".into(),
        },
    )
    .unwrap();
    assert!(text_of(&s, 1).contains("Seite 2 von 2"));
    let m = ops::get_meta(&s).unwrap();
    assert_eq!(m.title, "Testdokument");
    let s2 = ops::set_meta(&s, &ops::Meta { title: "Neu – Ü".into(), author: "Ich".into(), ..m }).unwrap();
    let m2 = ops::get_meta(&s2).unwrap();
    assert_eq!((m2.title.as_str(), m2.author.as_str(), m2.producer.as_str()), ("Neu – Ü", "Ich", "LightPDF Studio"));
}

#[test]
fn redaction_really_removes_text() {
    let b = fixture(2, 0);
    let mut by = BTreeMap::new();
    by.insert(0usize, vec![[60.0, 700.0, 400.0, 760.0]]);
    let out = ops::apply_redactions(&b, &by, 100.0).unwrap();
    assert_eq!(page_count(&out), 2);
    assert!(!text_of(&out, 0).contains("Hello"), "redacted page must not contain text");
    assert!(text_of(&out, 1).contains("Page 2 Hello"), "other pages untouched");
    let pdf = render::open(Arc::new(out.clone()), None).unwrap();
    let bmp = render::render_at_dpi(&pdf, 0, 72.0, 0).unwrap();
    let p = &bmp.rgba[(((792 - 730) * bmp.w + 200) * 4) as usize..][..3];
    assert!(p.iter().all(|&c| c < 30), "black box expected, got {p:?}");
    // the link of page 1 survives
    assert_eq!(ops::links(&out).len(), 1);
}

#[test]
fn compress_images_blank_export() {
    let b = fixture(3, 0);
    let l = ops::compress(&b, Compression::Lossless, &|_, _| {}).unwrap();
    assert!(l.len() <= b.len());
    assert!(text_of(&l, 0).contains("Hello"));
    let s = ops::compress(&b, Compression::Strong, &|_, _| {}).unwrap();
    assert_eq!(page_count(&s), 3);

    let mut png = std::io::Cursor::new(Vec::new());
    image::RgbImage::from_pixel(400, 300, image::Rgb([10, 200, 30])).write_to(&mut png, image::ImageFormat::Png).unwrap();
    let mut jpg = std::io::Cursor::new(Vec::new());
    image::RgbImage::from_pixel(300, 400, image::Rgb([200, 10, 30])).write_to(&mut jpg, image::ImageFormat::Jpeg).unwrap();
    let ip = ops::images_to_pdf(&[png.into_inner(), jpg.into_inner()], ops::PageSize::A4, 24.0).unwrap();
    assert_eq!(page_count(&ip), 2);
    let pdf = render::open(Arc::new(ip), None).unwrap();
    let infos = render::page_infos(&pdf);
    assert!(infos[0].view_box[2] > infos[0].view_box[3], "landscape image → landscape page");
    let bmp = render::render_at_dpi(&pdf, 0, 36.0, 0).unwrap();
    let c = &bmp.rgba[((bmp.h / 2 * bmp.w + bmp.w / 2) * 4) as usize..][..3];
    assert!(c[1] > 180 && c[0] < 40, "{c:?}");

    assert_eq!(page_count(&ops::blank_pdf(3, ops::A4).unwrap()), 3);
    let zip = ops::export_images(&b, ops::ImageFormat::Png, 50.0, &[0, 2], &|_, _| {}).unwrap();
    let z = zip::ZipArchive::new(std::io::Cursor::new(zip)).unwrap();
    assert_eq!(z.len(), 2);
    let t = ops::export_text(&b, "Seite").unwrap();
    assert!(t.contains("--- Seite 3 ---") && t.contains("Page 3 Hello"));
}

#[test]
fn links_and_outline() {
    let b = fixture(3, 0);
    let l = ops::links(&b);
    assert_eq!(l, vec![ops::Link { page: 0, rect: [72.0, 700.0, 300.0, 750.0], target: ops::LinkTarget::Page(2) }]);
    let o = ops::outline(&b);
    assert_eq!(o.len(), 1);
    assert_eq!(o[0].title, "Kapitel Ä");
    assert_eq!(o[0].page, Some(2));
}

#[test]
fn forms_fill_and_flatten() {
    let b = fixture(1, 0);
    let mut d = pdfdoc::load(&b, None).unwrap();
    let p = pdfdoc::pages(&d)[0];
    let tx = d.add_object(dictionary! {
        "Type" => "Annot", "Subtype" => "Widget", "FT" => "Tx", "T" => pdfdoc::text_hex("Name"),
        "Rect" => vec![72.into(), 500.into(), 300.into(), 520.into()], "P" => p, "DA" => pdfdoc::literal("/Helv 11 Tf 0 g"),
    });
    let on = d.add_object(Stream::new(
        dictionary! { "Type" => "XObject", "Subtype" => "Form", "BBox" => vec![0.into(), 0.into(), 12.into(), 12.into()] },
        b"0 g 2 2 8 8 re f".to_vec(),
    ));
    let off = d.add_object(Stream::new(
        dictionary! { "Type" => "XObject", "Subtype" => "Form", "BBox" => vec![0.into(), 0.into(), 12.into(), 12.into()] },
        Vec::new(),
    ));
    let cb = d.add_object(dictionary! {
        "Type" => "Annot", "Subtype" => "Widget", "FT" => "Btn", "T" => pdfdoc::text_hex("Agree"),
        "Rect" => vec![72.into(), 450.into(), 84.into(), 462.into()], "P" => p, "V" => "Off", "AS" => "Off",
        "AP" => dictionary! { "N" => dictionary! { "Ja" => on, "Off" => off } },
    });
    let mut annots = pdfdoc::page_annots(&d, p);
    annots.extend([Object::Reference(tx), Object::Reference(cb)]);
    pdfdoc::set_page_annots(&mut d, p, annots);
    d.catalog_mut().unwrap().set("AcroForm", dictionary! { "Fields" => vec![Object::Reference(tx), Object::Reference(cb)] });
    let b = pdfdoc::save(&mut d).unwrap();

    let f = ops::form_fields(&b);
    assert_eq!(f.len(), 2);
    assert_eq!(f[0].name, "Name");
    assert_eq!(f[0].page, Some(0));
    let filled =
        ops::fill_form(&b, &[("Name".into(), ops::FieldValue::Text("Max Müller".into())), ("Agree".into(), ops::FieldValue::Check(true))])
            .unwrap();
    let f = ops::form_fields(&filled);
    assert_eq!(f[0].value, ops::FieldValue::Text("Max Müller".into()));
    assert_eq!(f[1].value, ops::FieldValue::Check(true));
    // the display copy and the flattened file show the value
    assert!(text_of(&ops::display_copy(&filled).unwrap(), 0).contains("Max Müller"));
    let flat = ops::flatten_form(&filled).unwrap();
    assert!(!ops::has_form(&flat));
    assert!(text_of(&flat, 0).contains("Max Müller"));
}

#[test]
fn render_pool_works_in_background() {
    let b = Arc::new(fixture(4, 0));
    let woke = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let w2 = woke.clone();
    let pool = RenderPool::new(
        b,
        None,
        Arc::new(move || {
            w2.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        }),
    )
    .unwrap();
    assert_eq!(pool.pages.len(), 4);
    for i in 0..4 {
        pool.submit(render::Job::Render { page: i, scale: 0.5, rotation: 0, region: None, tag: 1 });
    }
    pool.submit(render::Job::Render { page: 0, scale: 2.0, rotation: 0, region: Some([0.0, 0.0, 100.0, 50.0]), tag: 1 });
    pool.submit(render::Job::Text { page: 3 });
    let mut got = 0;
    let mut text = false;
    while got < 6 {
        match pool.results.recv_timeout(std::time::Duration::from_secs(30)).unwrap() {
            render::Output::Rendered { region: Some(_), bitmap, .. } => {
                assert_eq!((bitmap.w, bitmap.h), (200, 100));
            }
            render::Output::Rendered { bitmap, .. } => assert_eq!((bitmap.w, bitmap.h), (306, 396)),
            render::Output::Text { page, text: t } => {
                assert_eq!(page, 3);
                assert!(t.string().0.contains("Page 4"));
                text = true;
            }
        }
        got += 1;
    }
    assert!(text);
    std::thread::sleep(std::time::Duration::from_millis(200));
    assert!(woke.load(std::sync::atomic::Ordering::SeqCst) >= 6);
}

#[test]
fn real_world_pdf_if_available() {
    // A real PDF from the cargo registry (not committed); skipped when absent.
    let Some(path) = std::fs::read_dir(
        std::env::var("CARGO_HOME").unwrap_or_else(|_| format!("{}/.cargo", std::env::var("HOME").unwrap_or_default())) + "/registry/src",
    )
    .ok()
    .and_then(|mut r| r.next())
    .and_then(|e| e.ok())
    .map(|e| e.path().join("qoi-0.4.1/doc/qoi-specification.pdf"))
    .filter(|p| p.exists()) else {
        return;
    };
    let b = std::fs::read(path).unwrap();
    let imp = ops::import_pdf(&b, None).unwrap();
    let pdf = render::open(Arc::new(imp.base.clone()), None).unwrap();
    assert!(render::render_at_dpi(&pdf, 0, 50.0, 0).is_some());
    assert!(render::page_text(&pdf, 0).string().0.to_lowercase().contains("qoi"));
    let list: Vec<_> = all_kinds().into_iter().filter(|a| a.page == 1).collect();
    let out = ops::export_pdf(&imp.base, &list, &ExportOpts::default()).unwrap();
    assert_eq!(ops::import_pdf(&out, None).unwrap().annots.len(), list.len());
}
