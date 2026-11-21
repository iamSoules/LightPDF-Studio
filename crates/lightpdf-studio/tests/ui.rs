//! UI tests: the real app, driven headlessly with synthetic touch / mouse events.
//! Above all they check that multi-touch never creates markup.

use eframe::egui::{self, Event, Pos2, TouchDeviceId, TouchId, TouchPhase, pos2};
use egui_kittest::Harness;
use lightpdf_core::lopdf::dictionary;
use lightpdf_core::model::Kind;
use lightpdf_core::{ops, pdfdoc};
use lightpdf_studio::app::{Screen, Studio};
use lightpdf_studio::platform::IncomingFile;
use lightpdf_studio::reader::{Reader, Tool};

fn fixture() -> Vec<u8> {
    let mut d = pdfdoc::new_document();
    let font =
        d.add_object(dictionary! { "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica", "Encoding" => "WinAnsiEncoding" });
    for i in 0..3 {
        let c = format!(
            "BT /F1 24 Tf 72 720 Td (Page {} Hello World) Tj ET\nBT /F1 12 Tf 72 680 Td (The quick brown fox jumps over the lazy dog.) Tj ET",
            i + 1
        );
        pdfdoc::add_page(&mut d, [612.0, 792.0], c.into_bytes(), dictionary! { "Font" => dictionary! { "F1" => font } }).unwrap();
    }
    pdfdoc::save(&mut d).unwrap()
}

fn app() -> Harness<'static, Studio> {
    let dir = std::env::temp_dir().join(format!("lightpdf-ui-{}", lightpdf_core::model::uid()));
    let mut h = Harness::builder().with_size([1000.0, 800.0]).build_eframe(move |cc| Studio::with_dir(cc, dir, false));
    h.state_mut().import_files(vec![IncomingFile { name: "Test.pdf".into(), bytes: fixture(), origin: None }], true);
    for _ in 0..400 {
        h.step();
        if let Screen::Reader(r) = &h.state().screen {
            if r.page_count() > 0 && r.canvas.width() > 100.0 {
                break;
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    for _ in 0..5 {
        h.step();
    }
    assert!(matches!(h.state().screen, Screen::Reader(_)), "document did not open");
    h
}

fn reader<'a>(h: &'a mut Harness<'static, Studio>) -> &'a mut Reader {
    match &mut h.state_mut().screen {
        Screen::Reader(r) => r,
        _ => panic!("no reader"),
    }
}

fn rd<'a>(h: &'a Harness<'static, Studio>) -> &'a Reader {
    match &h.state().screen {
        Screen::Reader(r) => r,
        _ => panic!("no reader"),
    }
}

fn touch(h: &mut Harness<'static, Studio>, id: u64, phase: TouchPhase, pos: Pos2) {
    h.event(Event::Touch { device_id: TouchDeviceId(1), id: TouchId(id), phase, pos, force: Some(0.5) });
    h.step();
}

/// A point on page 1 (screen coordinates).
fn on_page(h: &Harness<'static, Studio>, fx: f32, fy: f32) -> Pos2 {
    let r = rd(h).page_rect(0).intersect(rd(h).canvas);
    pos2(r.min.x + r.width() * fx, r.min.y + r.height() * fy)
}

fn set_tool(h: &mut Harness<'static, Studio>, t: Tool) {
    reader(h).tool = t;
    h.step();
}

fn pinch(h: &mut Harness<'static, Studio>, a: Pos2, b: Pos2, spread: f32) {
    touch(h, 1, TouchPhase::Start, a);
    touch(h, 1, TouchPhase::Move, a + egui::vec2(2.0, 1.0));
    touch(h, 2, TouchPhase::Start, b);
    for i in 1..=10 {
        let d = spread * i as f32 / 10.0;
        touch(h, 1, TouchPhase::Move, a - egui::vec2(d, d));
        touch(h, 2, TouchPhase::Move, b + egui::vec2(d, d));
    }
}

#[test]
fn finger_stroke_draws_ink() {
    let mut h = app();
    set_tool(&mut h, Tool::Ink);
    let p = on_page(&h, 0.3, 0.5);
    touch(&mut h, 1, TouchPhase::Start, p);
    for i in 1..=10 {
        touch(&mut h, 1, TouchPhase::Move, p + egui::vec2(i as f32 * 12.0, (i % 3) as f32 * 5.0));
    }
    touch(&mut h, 1, TouchPhase::End, p + egui::vec2(120.0, 0.0));
    let r = rd(&h);
    assert_eq!(r.annots.len(), 1);
    let Kind::Ink { paths, .. } = &r.annots[0].kind else { panic!() };
    assert!(paths[0].len() >= 8, "stroke starts where the finger landed and has all points");
    // first point = where the finger landed
    let first = r.pdf_to_screen(0, paths[0][0]);
    assert!(first.distance(p) < 1.0, "{first:?} vs {p:?}");
}

#[test]
fn pinch_zoom_with_every_drawing_tool_leaves_no_markup() {
    let mut h = app();
    for tool in [
        Tool::Ink,
        Tool::Highlighter,
        Tool::Rect,
        Tool::Ellipse,
        Tool::Line,
        Tool::Arrow,
        Tool::Highlight,
        Tool::Underline,
        Tool::Redact,
        Tool::Text,
        Tool::Note,
    ] {
        set_tool(&mut h, tool);
        let z0 = rd(&h).zoom;
        let (a, b) = (on_page(&h, 0.4, 0.4), on_page(&h, 0.6, 0.6));
        pinch(&mut h, a, b, 60.0);
        // lift one finger, keep moving the other (the classic "ghost stroke" case)
        touch(&mut h, 1, TouchPhase::End, a);
        for i in 1..=6 {
            touch(&mut h, 2, TouchPhase::Move, b + egui::vec2(60.0 + i as f32 * 15.0, 60.0));
        }
        touch(&mut h, 2, TouchPhase::End, b + egui::vec2(150.0, 60.0));
        h.step();
        let r = rd(&h);
        assert!(r.annots.is_empty(), "{tool:?}: pinch created {:?}", r.annots.iter().map(|a| a.type_name()).collect::<Vec<_>>());
        assert!(r.zoom > z0 * 1.2, "{tool:?}: pinch should zoom ({z0} → {})", r.zoom);
        assert!(r.editing.is_none(), "{tool:?}: no editor opened");
        reader(&mut h).zoom = z0;
        reader(&mut h).offset = egui::Vec2::ZERO;
        reader(&mut h).clamp_offset();
        h.step();
    }
}

#[test]
fn second_finger_cancels_a_stroke_in_progress() {
    let mut h = app();
    set_tool(&mut h, Tool::Ink);
    let p = on_page(&h, 0.3, 0.5);
    touch(&mut h, 1, TouchPhase::Start, p);
    for i in 1..=8 {
        touch(&mut h, 1, TouchPhase::Move, p + egui::vec2(i as f32 * 15.0, 0.0));
    }
    // a stroke is visibly in progress now; a second finger lands → must vanish completely
    touch(&mut h, 2, TouchPhase::Start, p + egui::vec2(0.0, 120.0));
    touch(&mut h, 2, TouchPhase::Move, p + egui::vec2(10.0, 140.0));
    touch(&mut h, 1, TouchPhase::End, p + egui::vec2(120.0, 0.0));
    touch(&mut h, 2, TouchPhase::End, p + egui::vec2(10.0, 140.0));
    assert!(rd(&h).annots.is_empty());
    assert!(!rd(&h).can_undo(), "nothing to undo either");
}

#[test]
fn two_finger_tap_places_nothing_single_tap_places_note() {
    let mut h = app();
    set_tool(&mut h, Tool::Note);
    let p = on_page(&h, 0.5, 0.5);
    touch(&mut h, 1, TouchPhase::Start, p);
    touch(&mut h, 2, TouchPhase::Start, p + egui::vec2(40.0, 0.0));
    touch(&mut h, 1, TouchPhase::End, p);
    touch(&mut h, 2, TouchPhase::End, p + egui::vec2(40.0, 0.0));
    assert!(rd(&h).annots.is_empty());
    touch(&mut h, 3, TouchPhase::Start, p);
    touch(&mut h, 3, TouchPhase::End, p);
    assert_eq!(rd(&h).annots.len(), 1);
    assert!(matches!(rd(&h).annots[0].kind, Kind::Note { .. }));
}

#[test]
fn eraser_pinch_erases_nothing() {
    let mut h = app();
    set_tool(&mut h, Tool::Rect);
    let p = on_page(&h, 0.4, 0.45);
    touch(&mut h, 1, TouchPhase::Start, p);
    for i in 1..=6 {
        touch(&mut h, 1, TouchPhase::Move, p + egui::vec2(i as f32 * 20.0, i as f32 * 20.0));
    }
    touch(&mut h, 1, TouchPhase::End, p + egui::vec2(120.0, 120.0));
    assert_eq!(rd(&h).annots.len(), 1);
    set_tool(&mut h, Tool::Eraser);
    let c = p + egui::vec2(60.0, 60.0);
    pinch(&mut h, c, c + egui::vec2(30.0, 30.0), 40.0);
    touch(&mut h, 1, TouchPhase::End, c);
    touch(&mut h, 2, TouchPhase::End, c);
    assert_eq!(rd(&h).annots.len(), 1, "pinching over a comment with the eraser must not erase it");
    // a deliberate single-finger swipe does erase
    touch(&mut h, 3, TouchPhase::Start, c - egui::vec2(60.0, 0.0));
    for i in 1..=8 {
        touch(&mut h, 3, TouchPhase::Move, c - egui::vec2(60.0, 0.0) + egui::vec2(i as f32 * 15.0, 0.0));
    }
    touch(&mut h, 3, TouchPhase::End, c + egui::vec2(60.0, 0.0));
    assert!(rd(&h).annots.is_empty());
}

#[test]
fn one_finger_pans_with_hand_tool_and_tap_tools() {
    let mut h = app();
    for tool in [Tool::Hand, Tool::Text] {
        set_tool(&mut h, tool);
        let y0 = rd(&h).offset.y;
        let p = on_page(&h, 0.5, 0.7);
        touch(&mut h, 1, TouchPhase::Start, p);
        for i in 1..=8 {
            touch(&mut h, 1, TouchPhase::Move, p - egui::vec2(0.0, i as f32 * 25.0));
        }
        touch(&mut h, 1, TouchPhase::End, p - egui::vec2(0.0, 200.0));
        assert!(rd(&h).offset.y < y0 - 100.0, "{tool:?}: page should scroll ({y0} → {})", rd(&h).offset.y);
        assert!(rd(&h).annots.is_empty(), "{tool:?}: scrolling must not place anything");
    }
}

#[test]
fn mouse_text_box_and_highlight_with_text() {
    let mut h = app();
    // text box: click, type, click elsewhere
    set_tool(&mut h, Tool::Text);
    let p = on_page(&h, 0.3, 0.6);
    h.event(Event::PointerMoved(p));
    h.event(Event::PointerButton { pos: p, button: egui::PointerButton::Primary, pressed: true, modifiers: Default::default() });
    h.event(Event::PointerButton { pos: p, button: egui::PointerButton::Primary, pressed: false, modifiers: Default::default() });
    h.step();
    h.step();
    assert!(rd(&h).editing.is_some(), "text editor opens");
    h.event(Event::Text("Hallo aus Rust".into()));
    h.step();
    h.step();
    reader(&mut h).commit_editing();
    let r = rd(&h);
    assert_eq!(r.annots.len(), 1);
    let Kind::Text { text, .. } = &r.annots[0].kind else { panic!() };
    assert_eq!(text, "Hallo aus Rust");

    // highlight by dragging over "Hello World" on page 1
    set_tool(&mut h, Tool::Highlight);
    reader(&mut h).request_text(0);
    for _ in 0..200 {
        h.step();
        if rd(&h).text.contains_key(&0) {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    // "Page 1 " ends at x≈155; drag from inside "H" to well past the end of the line
    let a = rd(&h).pdf_to_screen(0, [160.0, 728.0]);
    let b = rd(&h).pdf_to_screen(0, [330.0, 728.0]);
    h.event(Event::PointerMoved(a));
    h.event(Event::PointerButton { pos: a, button: egui::PointerButton::Primary, pressed: true, modifiers: Default::default() });
    for i in 1..=6 {
        h.event(Event::PointerMoved(a + (b - a) * (i as f32 / 6.0)));
    }
    h.event(Event::PointerButton { pos: b, button: egui::PointerButton::Primary, pressed: false, modifiers: Default::default() });
    h.step();
    let r = rd(&h);
    let hl = r.annots.iter().find(|a| matches!(a.kind, Kind::Highlight { .. })).expect("highlight created");
    let Kind::Highlight { text, rects } = &hl.kind else { unreachable!() };
    assert_eq!(text.as_deref(), Some("Hello World"));
    assert_eq!(rects.len(), 1);
}

#[test]
fn undo_redo_and_save_roundtrip() {
    let mut h = app();
    set_tool(&mut h, Tool::Ink);
    let p = on_page(&h, 0.3, 0.5);
    for k in 0..2u64 {
        let q = p + egui::vec2(0.0, k as f32 * 40.0);
        touch(&mut h, 10 + k, TouchPhase::Start, q);
        for i in 1..=5 {
            touch(&mut h, 10 + k, TouchPhase::Move, q + egui::vec2(i as f32 * 20.0, 0.0));
        }
        touch(&mut h, 10 + k, TouchPhase::End, q + egui::vec2(100.0, 0.0));
    }
    assert_eq!(rd(&h).annots.len(), 2);
    let ctx = h.ctx.clone();
    reader(&mut h).undo(&ctx);
    assert_eq!(rd(&h).annots.len(), 1);
    reader(&mut h).redo(&ctx);
    assert_eq!(rd(&h).annots.len(), 2);
    // save via the app (background task)
    let Screen::Reader(mut r) = std::mem::replace(&mut h.state_mut().screen, Screen::Library) else { panic!() };
    let id = r.meta.id.clone();
    r.save(h.state_mut(), false);
    h.state_mut().screen = Screen::Reader(r);
    for _ in 0..300 {
        h.step();
        if !rd(&h).dirty {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert!(!rd(&h).dirty, "saved");
    let bytes = h.state().storage.read_doc(&id).unwrap();
    let imp = ops::import_pdf(&bytes, None).unwrap();
    assert_eq!(imp.annots.len(), 2, "comments are real PDF annotations in the saved file");
}

#[test]
fn page_operations_keep_comments_on_their_pages() {
    let mut h = app();
    set_tool(&mut h, Tool::Note);
    let p = on_page(&h, 0.5, 0.5);
    touch(&mut h, 1, TouchPhase::Start, p);
    touch(&mut h, 1, TouchPhase::End, p);
    reader(&mut h).commit_editing();
    assert_eq!(rd(&h).annots[0].page, 1);
    // move page 1 to the end
    let ctx = h.ctx.clone();
    let Screen::Reader(r) = std::mem::replace(&mut h.state_mut().screen, Screen::Library) else { panic!() };
    r.page_op(&mut h.state_mut().tasks, &ctx, "test".into(), |b| ops::reorder_pages(b, &[1, 2, 0]));
    h.state_mut().screen = Screen::Reader(r);
    for _ in 0..300 {
        h.step();
        if rd(&h).annots.first().is_some_and(|a| a.page == 3) {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert_eq!(rd(&h).annots.len(), 1);
    assert_eq!(rd(&h).annots[0].page, 3, "the note moved with its page");
    reader(&mut h).undo(&ctx);
    assert_eq!(rd(&h).annots[0].page, 1, "page operations are undoable");
}
