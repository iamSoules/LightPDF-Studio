//! Writes sample outputs for checking with other PDF readers: `cargo run --example dump -- <dir>`
use lightpdf_core::model::{Annotation, Kind};
use lightpdf_core::{ops, pdfdoc};
use std::collections::BTreeMap;

fn main() {
    let dir = std::path::PathBuf::from(std::env::args().nth(1).expect("output dir"));
    std::fs::create_dir_all(&dir).unwrap();
    let mut d = pdfdoc::new_document();
    let font = lightpdf_core::annotio::standard_font(&mut d, "Helvetica");
    for i in 0..3 {
        let c = format!("BT /F1 24 Tf 72 720 Td (Page {} Hello World) Tj ET", i + 1);
        pdfdoc::add_page(&mut d, [612.0, 792.0], c.into_bytes(), lopdf::dictionary! { "Font" => lopdf::dictionary! { "F1" => font } })
            .unwrap();
    }
    let base = pdfdoc::save(&mut d).unwrap();
    let a = |kind, page| Annotation {
        id: lightpdf_core::model::uid(),
        page,
        color: "#e5484d".into(),
        opacity: 1.0,
        created_at: 0.0,
        author: Some("Tester".into()),
        note: Some("Kommentar".into()),
        kind,
    };
    let annots = vec![
        a(Kind::Highlight { rects: vec![[72.0, 715.0, 300.0, 745.0]], text: None }, 1),
        a(Kind::Ink { paths: vec![vec![[100.0, 100.0], [200.0, 150.0]]], width: 3.0, highlighter: false }, 1),
        a(Kind::Text { rect: [300.0, 500.0, 500.0, 530.0], text: "Grüße – FreeText".into(), font_size: 12.0, bold: true }, 2),
        a(Kind::Note { at: [500.0, 700.0], text: "Notiz".into() }, 2),
        a(Kind::Arrow { from: [50.0, 80.0], to: [150.0, 90.0], width: 2.0 }, 3),
    ];
    let w = |n: &str, b: &[u8]| std::fs::write(dir.join(n), b).unwrap();
    w("annotated.pdf", &ops::export_pdf(&base, &annots, &ops::ExportOpts::default()).unwrap());
    w("flattened.pdf", &ops::export_pdf(&base, &annots, &ops::ExportOpts { flatten: true, password: None }).unwrap());
    w("encrypted.pdf", &ops::export_pdf(&base, &annots, &ops::ExportOpts { password: Some("geheim".into()), flatten: false }).unwrap());
    w("merged.pdf", &ops::merge(&[base.clone(), base.clone()]).unwrap());
    w("reordered.pdf", &ops::reorder_pages(&base, &[2, 0, 0]).unwrap());
    let mut r = BTreeMap::new();
    r.insert(0, vec![[60.0, 700.0, 400.0, 760.0]]);
    w("redacted.pdf", &ops::apply_redactions(&base, &r, 100.0).unwrap());
    w(
        "numbered.pdf",
        &ops::stamp_text(
            &ops::rotate_pages(&base, &[1], 90).unwrap(),
            &ops::StampTextOpts {
                template: "Seite {n} von {total}".into(),
                position: ops::Position::BottomRight,
                size: 10.0,
                color: "#000000".into(),
                start: 1,
                margin: 24.0,
                skip_first: false,
                name: String::new(),
                date: String::new(),
            },
        )
        .unwrap(),
    );
    println!("ok");
}
