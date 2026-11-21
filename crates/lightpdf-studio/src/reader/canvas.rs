//! Layout, zoom & scroll, and all input on the document.
//!
//! Every pointer – finger, pen, mouse – goes through [`GestureArbiter`]: it alone decides
//! whether a gesture draws or navigates. That's what keeps pinch-zoom from leaving ink,
//! shapes or notes behind (the v2 multi-touch bug).

use super::*;
use eframe::egui::{Pos2, pos2, vec2};
use lightpdf_core::geom::PageGeom;
use lightpdf_core::gesture::{Action, Intent, PointerId, PointerKind};
use lightpdf_core::model::{Pt, Rect as PRect, norm_rect};

pub const PAD: f32 = 16.0;
pub const GAP: f32 = 12.0;
const MOUSE: PointerId = PointerId(u64::MAX);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Fit {
    Auto,
    Width,
    Page,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Handle {
    N,
    S,
    E,
    W,
    NE,
    NW,
    SE,
    SW,
    From,
    To,
}

impl Handle {
    fn has(self, c: char) -> bool {
        let s = match self {
            Handle::N => "n",
            Handle::S => "s",
            Handle::E => "e",
            Handle::W => "w",
            Handle::NE => "ne",
            Handle::NW => "nw",
            Handle::SE => "se",
            Handle::SW => "sw",
            _ => "",
        };
        s.contains(c)
    }
    fn corner(self) -> bool {
        matches!(self, Handle::NE | Handle::NW | Handle::SE | Handle::SW)
    }
}

impl Reader {
    // ------------------------------------------------------------------ layout

    pub fn geom(&self, page: usize) -> PageGeom {
        self.pages[page].geom(self.view_rot)
    }

    pub(super) fn ensure_layout(&mut self, mode: ScrollMode, spread: bool) {
        let key = (mode, spread, self.view_rot, self.pages.len());
        if self.layout_key == Some(key) {
            return;
        }
        self.layout_key = Some(key);
        let sizes: Vec<Vec2> = (0..self.pages.len())
            .map(|i| {
                let g = self.geom(i);
                vec2(g.width() as f32, g.height() as f32)
            })
            .collect();
        let mut rects = vec![Rect::NOTHING; sizes.len()];
        match mode {
            ScrollMode::Horizontal => {
                let maxh = sizes.iter().map(|s| s.y).fold(0.0, f32::max);
                let mut x = PAD;
                for (i, s) in sizes.iter().enumerate() {
                    rects[i] = Rect::from_min_size(pos2(x, PAD + (maxh - s.y) / 2.0), *s);
                    x += s.x + GAP;
                }
                self.content = vec2(x - GAP + PAD, maxh + 2.0 * PAD);
            }
            _ => {
                // rows: one page, or two pages side by side (first page alone, like a book)
                let mut rows: Vec<Vec<usize>> = Vec::new();
                if spread {
                    rows.push(vec![0]);
                    let mut i = 1;
                    while i < sizes.len() {
                        rows.push((i..(i + 2).min(sizes.len())).collect());
                        i += 2;
                    }
                    rows.retain(|r| !r.is_empty() && r[0] < sizes.len());
                } else {
                    rows = (0..sizes.len()).map(|i| vec![i]).collect();
                }
                let row_w = |r: &Vec<usize>| r.iter().map(|&i| sizes[i].x).sum::<f32>() + GAP * (r.len() as f32 - 1.0);
                let maxw = rows.iter().map(row_w).fold(0.0, f32::max);
                let mut y = PAD;
                for r in &rows {
                    let h = r.iter().map(|&i| sizes[i].y).fold(0.0, f32::max);
                    let mut x = PAD + (maxw - row_w(r)) / 2.0;
                    for &i in r {
                        rects[i] = Rect::from_min_size(pos2(x, y + (h - sizes[i].y) / 2.0), sizes[i]);
                        x += sizes[i].x + GAP;
                    }
                    y += h + GAP;
                }
                self.content = vec2(maxw + 2.0 * PAD, y - GAP + PAD);
            }
        }
        self.layout = rects;
    }

    /// Screen rect of a page.
    pub fn page_rect(&self, page: usize) -> Rect {
        let r = self.layout[page];
        Rect::from_min_size(self.canvas.min + self.offset + r.min.to_vec2() * self.zoom, r.size() * self.zoom)
    }

    pub fn screen_to_pdf(&self, page: usize, p: Pos2) -> Pt {
        let r = self.page_rect(page);
        let v = (p - r.min) / self.zoom;
        self.geom(page).to_pdf([v.x as f64, v.y as f64])
    }

    pub fn pdf_to_screen(&self, page: usize, p: Pt) -> Pos2 {
        let r = self.page_rect(page);
        let v = self.geom(page).to_view(p);
        r.min + vec2(v[0] as f32, v[1] as f32) * self.zoom
    }

    pub fn pdf_rect_to_screen(&self, page: usize, r: PRect) -> Rect {
        Rect::from_two_pos(self.pdf_to_screen(page, [r[0], r[1]]), self.pdf_to_screen(page, [r[2], r[3]]))
    }

    /// Page under a screen position (with a little tolerance).
    pub fn page_at(&self, p: Pos2, tolerance: f32) -> Option<usize> {
        let mut best = None;
        let mut bd = tolerance;
        for i in self.visible_pages(0.0) {
            let r = self.page_rect(i);
            let d = r.distance_to_pos(p);
            if d <= bd {
                bd = d;
                best = Some(i);
                if d == 0.0 {
                    break;
                }
            }
        }
        best
    }

    pub fn visible_pages(&self, margin: f32) -> Vec<usize> {
        let view = self.canvas.expand(margin);
        let mut out = Vec::new();
        for i in 0..self.layout.len() {
            if self.page_rect(i).intersects(view) {
                out.push(i);
            }
        }
        out
    }

    pub fn clamp_offset(&mut self) {
        let size = self.canvas.size();
        let c = self.content * self.zoom;
        for axis in 0..2 {
            if c[axis] <= size[axis] {
                self.offset[axis] = (size[axis] - c[axis]) / 2.0;
            } else {
                self.offset[axis] = self.offset[axis].clamp(size[axis] - c[axis], 0.0);
            }
        }
    }

    pub fn zoom_at(&mut self, center: Pos2, factor: f32, now: f64) {
        let new = (self.zoom * factor).clamp(0.08, 12.0);
        if (new - self.zoom).abs() < 1e-6 {
            return;
        }
        let doc = (center - self.canvas.min - self.offset) / self.zoom;
        self.zoom = new;
        self.user_zoomed = true;
        self.offset = center - self.canvas.min - doc * new;
        self.last_zoom_change = now;
        self.clamp_offset();
    }

    pub fn set_zoom(&mut self, z: f32, now: f64) {
        let c = self.canvas.center();
        self.zoom_at(c, z / self.zoom, now);
    }

    pub fn fit(&mut self, f: Fit, now: f64) {
        if self.pages.is_empty() || self.canvas.width() < 10.0 {
            self.pending_fit = Some(f);
            return;
        }
        let page = self.current_page.min(self.pages.len() - 1);
        let r = self.layout[page];
        let avail = self.canvas.size() - vec2(2.0 * PAD, 2.0 * PAD);
        let zw = avail.x / r.width();
        let zp = zw.min(avail.y / r.height());
        let z = match f {
            Fit::Width => zw,
            Fit::Page => zp,
            // phones: page width; big screens: width but at most 1.4×
            Fit::Auto => {
                if self.canvas.width() < 760.0 {
                    zw
                } else {
                    zw.min(1.4)
                }
            }
        };
        self.zoom = z.clamp(0.08, 12.0);
        self.user_zoomed = f != Fit::Auto;
        self.last_zoom_change = now;
        self.go_to(page);
    }

    pub fn go_to(&mut self, page: usize) {
        if self.layout.is_empty() {
            self.pending_goto = Some(page);
            return;
        }
        let page = page.min(self.layout.len() - 1);
        let r = self.layout[page];
        self.offset.y = -(r.min.y - PAD / 2.0) * self.zoom;
        if self.layout_key.is_some_and(|k| k.0 == ScrollMode::Horizontal) {
            self.offset.x = -(r.min.x - PAD / 2.0) * self.zoom;
        }
        self.current_page = page;
        self.fling = Vec2::ZERO;
        self.clamp_offset();
    }

    pub fn scroll_to_pdf_rect(&mut self, page: usize, r: PRect) {
        let s = self.pdf_rect_to_screen(page, r);
        if !self.canvas.shrink(30.0).contains_rect(s) {
            let target = self.canvas.center();
            self.offset += target - s.center();
            self.clamp_offset();
        }
    }

    fn update_current_page(&mut self) {
        let c = self.canvas.center();
        let mut best = (f32::MAX, self.current_page);
        for i in self.visible_pages(0.0) {
            let d = self.page_rect(i).distance_sq_to_pos(c);
            if d < best.0 {
                best = (d, i);
            }
        }
        self.current_page = best.1;
    }

    // ------------------------------------------------------------------ input

    /// What a single pointer at `p` would do with the current tool.
    fn intent_at(&self, p: Pos2) -> Intent {
        match self.tool {
            Tool::Hand => Intent::Navigate,
            Tool::Select => {
                if self.handle_at(p).is_some() || self.annot_at(p).is_some() {
                    Intent::Draw
                } else {
                    Intent::Navigate
                }
            }
            t if t.is_tap() => Intent::Tap,
            _ => {
                if self.page_at(p, 0.0).is_some() {
                    Intent::Draw
                } else {
                    Intent::Navigate
                }
            }
        }
    }

    /// Processes this frame's raw events. `blocked(pos)` is true where other UI lies on top.
    pub(super) fn handle_input(&mut self, ctx: &egui::Context, blocked: &dyn Fn(Pos2) -> bool) {
        let (events, now, mods) = ctx.input(|i| (i.events.clone(), i.time, i.modifiers));
        let mut actions: Vec<Action> = Vec::new();
        let slop = self.arbiter.config.slop;
        let touch_recent = |t: f64| now - t < 0.8;

        for e in &events {
            match e {
                egui::Event::Touch { device_id, id, phase, pos, .. } => {
                    self.last_touch = now;
                    let pid = PointerId(id.0.wrapping_add(device_id.0.wrapping_mul(0x9E37_79B9_7F4A_7C15)) & !(1 << 63));
                    match phase {
                        egui::TouchPhase::Start => {
                            if !self.canvas.contains(*pos) || blocked(*pos) {
                                continue;
                            }
                            self.fling = Vec2::ZERO;
                            let first = self.arbiter.is_idle();
                            let intent = self.intent_at(*pos);
                            actions.extend(self.arbiter.down(pid, PointerKind::Touch, [pos.x, pos.y], now, intent));
                            self.taps.down(pid, [pos.x, pos.y], now, first);
                        }
                        egui::TouchPhase::Move => {
                            self.taps.moved(pid, [pos.x, pos.y], slop);
                            actions.extend(self.arbiter.moved(pid, [pos.x, pos.y], now));
                        }
                        egui::TouchPhase::End => {
                            let nav = self.arbiter.is_navigating() || self.arbiter.active_pointers() == 0;
                            actions.extend(self.arbiter.up(pid, [pos.x, pos.y], now));
                            if let Some(tp) = self.taps.up(pid, now, self.arbiter.config.tap_time)
                                && (nav || self.intent_at(pos2(tp[0], tp[1])) == Intent::Navigate)
                            {
                                actions.push(Action::Tap { pos: tp, kind: PointerKind::Touch });
                            }
                        }
                        egui::TouchPhase::Cancel => {
                            self.taps.up(pid, -1e9, 0.0);
                            actions.extend(self.arbiter.cancel(pid, now));
                        }
                    }
                }
                // Mouse (and pens that the OS reports as a mouse). Touch-emulated pointer
                // events are ignored – the touch events above already handled them.
                egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, .. } if !touch_recent(self.last_touch) => {
                    if *pressed {
                        if !self.canvas.contains(*pos) || blocked(*pos) {
                            continue;
                        }
                        self.mouse_down = true;
                        self.fling = Vec2::ZERO;
                        let intent = self.intent_at(*pos);
                        actions.extend(self.arbiter.down(MOUSE, PointerKind::Mouse, [pos.x, pos.y], now, intent));
                        self.taps.down(MOUSE, [pos.x, pos.y], now, true);
                    } else if self.mouse_down {
                        self.mouse_down = false;
                        let nav = self.arbiter.is_navigating();
                        actions.extend(self.arbiter.up(MOUSE, [pos.x, pos.y], now));
                        if let Some(tp) = self.taps.up(MOUSE, now, 0.5)
                            && nav
                        {
                            actions.push(Action::Tap { pos: tp, kind: PointerKind::Mouse });
                        }
                    }
                }
                egui::Event::PointerButton { pos, button: egui::PointerButton::Middle, pressed, .. } if !touch_recent(self.last_touch) => {
                    if *pressed && self.canvas.contains(*pos) && !blocked(*pos) {
                        self.mouse_down = true;
                        actions.extend(self.arbiter.down(MOUSE, PointerKind::Mouse, [pos.x, pos.y], now, Intent::Navigate));
                    } else if !*pressed && self.mouse_down {
                        self.mouse_down = false;
                        actions.extend(self.arbiter.up(MOUSE, [pos.x, pos.y], now));
                    }
                }
                egui::Event::PointerMoved(pos) if self.mouse_down && !touch_recent(self.last_touch) => {
                    self.taps.moved(MOUSE, [pos.x, pos.y], 3.0);
                    actions.extend(self.arbiter.moved(MOUSE, [pos.x, pos.y], now));
                }
                egui::Event::PointerGone if self.mouse_down && !touch_recent(self.last_touch) => {
                    self.mouse_down = false;
                    actions.extend(self.arbiter.cancel(MOUSE, now));
                }
                egui::Event::MouseWheel { unit, delta, modifiers, .. } => {
                    let Some(p) = ctx.input(|i| i.pointer.hover_pos()) else { continue };
                    if !self.canvas.contains(p) || blocked(p) {
                        continue;
                    }
                    let px = match unit {
                        egui::MouseWheelUnit::Point => *delta,
                        egui::MouseWheelUnit::Line => *delta * 40.0,
                        egui::MouseWheelUnit::Page => *delta * self.canvas.height(),
                    };
                    if modifiers.ctrl || modifiers.command {
                        self.zoom_at(p, (px.y / 300.0).exp(), now);
                    } else if modifiers.shift && px.x == 0.0 {
                        self.offset.x += px.y;
                    } else {
                        self.offset += px;
                    }
                    self.clamp_offset();
                }
                egui::Event::Zoom(f) => {
                    // trackpad pinch (macOS) / ctrl+wheel emulation
                    if let Some(p) = ctx.input(|i| i.pointer.hover_pos()).filter(|p| self.canvas.contains(*p)) {
                        self.zoom_at(p, *f, now);
                    }
                }
                _ => {}
            }
        }
        actions.extend(self.arbiter.tick(now));

        for a in actions {
            match a {
                Action::Begin { pos, .. } => self.begin(ctx, pos2(pos[0], pos[1]), mods),
                Action::Move { pos } => self.drag(pos2(pos[0], pos[1]), mods),
                Action::End { pos } => self.end(ctx, pos2(pos[0], pos[1])),
                Action::Cancel => self.cancel(),
                Action::Tap { pos, kind } => self.tap(ctx, pos2(pos[0], pos[1]), kind, now),
                Action::DoubleTap { .. } => {}
                Action::Navigate { pan, zoom, center } => {
                    self.offset += vec2(pan[0], pan[1]);
                    if zoom != 1.0 {
                        self.zoom_at(pos2(center[0], center[1]), zoom, now);
                    }
                    self.clamp_offset();
                }
                Action::NavigateEnd { velocity } => {
                    let v = vec2(velocity[0], velocity[1]);
                    if v.length() > 120.0 {
                        self.fling = if v.length() > 6000.0 { v.normalized() * 6000.0 } else { v };
                    }
                }
            }
        }

        // fling (kinetic scrolling)
        let dt = (now - self.last_frame).clamp(0.0, 0.05) as f32;
        self.last_frame = now;
        if self.fling.length() > 8.0 {
            let before = self.offset;
            self.offset += self.fling * dt;
            self.clamp_offset();
            if (self.offset - before).length() < 0.01 {
                self.fling = Vec2::ZERO;
            }
            self.fling *= (-dt * 3.2).exp();
            ctx.request_repaint();
        } else {
            self.fling = Vec2::ZERO;
        }
        if !self.arbiter.is_idle() {
            ctx.request_repaint();
        }
        self.update_current_page();
    }

    // ------------------------------------------------------------------ hit tests

    pub fn annot_screen_bounds(&self, a: &Annotation) -> Rect {
        let page = a.page as usize - 1;
        match &a.kind {
            Kind::Note { at, .. } => {
                let p = self.pdf_to_screen(page, *at);
                Rect::from_min_size(p, vec2(22.0, 22.0))
            }
            _ => self.pdf_rect_to_screen(page, a.bounds()),
        }
    }

    pub fn annot_at(&self, p: Pos2) -> Option<String> {
        let tol = if self.last_touch > 0.0 { 10.0 } else { 5.0 };
        for a in self.annots.iter().rev() {
            let page = a.page as usize - 1;
            if page >= self.pages.len() || !self.page_rect(page).expand(30.0).contains(p) {
                continue;
            }
            let hit = match &a.kind {
                Kind::Ink { paths, width, .. } => {
                    let w = (*width as f32 * self.zoom / 2.0).max(tol);
                    paths.iter().any(|path| {
                        let pts: Vec<Pos2> = path.iter().map(|q| self.pdf_to_screen(page, *q)).collect();
                        if pts.len() == 1 {
                            return pts[0].distance(p) <= w;
                        }
                        pts.windows(2).any(|s| dist_to_segment(p, s[0], s[1]) <= w)
                    })
                }
                Kind::Line { from, to, width } | Kind::Arrow { from, to, width } => {
                    dist_to_segment(p, self.pdf_to_screen(page, *from), self.pdf_to_screen(page, *to))
                        <= (*width as f32 * self.zoom / 2.0).max(tol)
                }
                Kind::Highlight { rects, .. } | Kind::Underline { rects, .. } | Kind::Strike { rects, .. } => {
                    rects.iter().any(|r| self.pdf_rect_to_screen(page, *r).expand(2.0).contains(p))
                }
                _ => self.annot_screen_bounds(a).expand(tol / 2.0).contains(p),
            };
            if hit {
                return Some(a.id.clone());
            }
        }
        None
    }

    pub fn handle_size(&self) -> f32 {
        if self.last_touch > 0.0 { 22.0 } else { 11.0 }
    }

    pub fn handles(&self, a: &Annotation) -> Vec<(Handle, Pos2)> {
        let page = a.page as usize - 1;
        match &a.kind {
            Kind::Line { from, to, .. } | Kind::Arrow { from, to, .. } => {
                vec![(Handle::From, self.pdf_to_screen(page, *from)), (Handle::To, self.pdf_to_screen(page, *to))]
            }
            _ if a.rect().is_some() => {
                let b = self.annot_screen_bounds(a);
                vec![
                    (Handle::NW, b.left_top()),
                    (Handle::N, b.center_top()),
                    (Handle::NE, b.right_top()),
                    (Handle::E, b.right_center()),
                    (Handle::SE, b.right_bottom()),
                    (Handle::S, b.center_bottom()),
                    (Handle::SW, b.left_bottom()),
                    (Handle::W, b.left_center()),
                ]
            }
            _ => vec![],
        }
    }

    pub fn handle_at(&self, p: Pos2) -> Option<Handle> {
        let a = self.find(self.selected.as_deref()?)?;
        let hs = self.handle_size();
        self.handles(a).into_iter().find(|(_, q)| q.distance(p) <= hs * 0.8).map(|h| h.0)
    }

    // ------------------------------------------------------------------ tools

    fn begin(&mut self, ctx: &egui::Context, p: Pos2, _mods: egui::Modifiers) {
        self.commit_editing();
        let tool = self.tool;
        if tool == Tool::Select {
            if let Some(h) = self.handle_at(p) {
                let id = self.selected.clone().unwrap();
                let orig = self.find(&id).cloned().unwrap();
                self.checkpoint();
                self.draw = Some(Draw::Resize { id, orig, handle: h });
                return;
            }
            if let Some(id) = self.annot_at(p) {
                let orig = self.find(&id).cloned().unwrap();
                let page = orig.page as usize - 1;
                self.selected = Some(id.clone());
                let start = self.screen_to_pdf(page, p);
                self.draw = Some(Draw::Move { id, orig, start, moved: false });
            }
            return;
        }
        if tool == Tool::Eraser {
            self.draw = Some(Draw::Erase { before: self.annots.clone(), erased: false });
            self.erase_at(p);
            return;
        }
        let Some(page) = self.page_at(p, 0.0) else { return };
        let pt = self.screen_to_pdf(page, p);
        let color = self.opts.color(tool);
        let o = self.opts.clone();
        let kind = match tool {
            Tool::Ink => Kind::Ink { paths: vec![vec![pt]], width: o.width, highlighter: false },
            Tool::Highlighter => Kind::Ink { paths: vec![vec![pt]], width: (o.width * 4.0).max(8.0), highlighter: true },
            Tool::Rect => Kind::Rect { rect: [pt[0], pt[1], pt[0], pt[1]], width: o.width, fill: o.fill.clone() },
            Tool::Ellipse => Kind::Ellipse { rect: [pt[0], pt[1], pt[0], pt[1]], width: o.width, fill: o.fill.clone() },
            Tool::Line => Kind::Line { from: pt, to: pt, width: o.width },
            Tool::Arrow => Kind::Arrow { from: pt, to: pt, width: o.width },
            Tool::Redact => Kind::Redact { rect: [pt[0], pt[1], pt[0], pt[1]] },
            Tool::Highlight | Tool::Underline | Tool::Strike => {
                let rects = vec![];
                let kind = match tool {
                    Tool::Highlight => Kind::Highlight { rects, text: None },
                    Tool::Underline => Kind::Underline { rects, text: None },
                    _ => Kind::Strike { rects, text: None },
                };
                self.request_text(page);
                let start = self.text.get(&page).and_then(|t| t.nearest(pt, 14.0));
                let ann = self.new_annot(page, color, o.opacity, kind);
                self.draw = Some(Draw::Markup { page, start, start_pdf: pt, ann });
                let _ = ctx;
                return;
            }
            _ => return,
        };
        let (color, opacity) = match tool {
            Tool::Highlighter => (color, 0.45),
            Tool::Redact => ("#000000".into(), 1.0),
            _ => (color, o.opacity),
        };
        let ann = self.new_annot(page, color, opacity, kind);
        self.draw = Some(Draw::Draft { page, ann, start: pt });
    }

    fn drag(&mut self, p: Pos2, mods: egui::Modifiers) {
        let zoom = self.zoom as f64;
        match self.draw.take() {
            Some(Draw::Draft { page, mut ann, start }) => {
                let g = self.geom(page);
                let pt = self.screen_to_pdf(page, p);
                match &mut ann.kind {
                    Kind::Ink { paths, highlighter, .. } => {
                        let path = &mut paths[0];
                        let last = g.to_view(*path.last().unwrap());
                        let v = g.to_view(pt);
                        if ((v[0] - last[0]).powi(2) + (v[1] - last[1]).powi(2)).sqrt() * zoom >= 1.2 {
                            if mods.shift && *highlighter {
                                path.truncate(1);
                            }
                            path.push(pt);
                        }
                    }
                    Kind::Rect { rect, .. } | Kind::Ellipse { rect, .. } | Kind::Redact { rect } => {
                        let (s, mut e) = (g.to_view(start), g.to_view(pt));
                        if mods.shift {
                            let d = (e[0] - s[0]).abs().max((e[1] - s[1]).abs());
                            e = [s[0] + d * (e[0] - s[0]).signum(), s[1] + d * (e[1] - s[1]).signum()];
                        }
                        *rect = g.rect_to_pdf(norm_rect(s, e));
                    }
                    Kind::Line { to, .. } | Kind::Arrow { to, .. } => {
                        let (s, mut e) = (g.to_view(start), g.to_view(pt));
                        if mods.shift {
                            let step = std::f64::consts::FRAC_PI_4;
                            let ang = ((e[1] - s[1]).atan2(e[0] - s[0]) / step).round() * step;
                            let len = ((e[0] - s[0]).powi(2) + (e[1] - s[1]).powi(2)).sqrt();
                            e = [s[0] + ang.cos() * len, s[1] + ang.sin() * len];
                        }
                        *to = g.to_pdf(e);
                    }
                    _ => {}
                }
                self.draw = Some(Draw::Draft { page, ann, start });
            }
            Some(Draw::Markup { page, mut start, start_pdf, mut ann }) => {
                let pt = self.screen_to_pdf(page, p);
                if start.is_none() {
                    start = self.text.get(&page).and_then(|t| t.nearest(start_pdf, 14.0));
                }
                let rects = match (start, self.text.get(&page)) {
                    // the end snaps to the nearest glyph however far away (drag past the line end)
                    (Some(s), Some(t)) => match t.nearest(pt, f64::INFINITY) {
                        Some(e) => t.line_rects(s.min(e), s.max(e)),
                        None => vec![],
                    },
                    // no text layer (scan): mark a free rectangle
                    (None, Some(t)) if t.glyphs.is_empty() => vec![norm_rect(start_pdf, pt)],
                    _ => vec![],
                };
                match &mut ann.kind {
                    Kind::Highlight { rects: r, .. } | Kind::Underline { rects: r, .. } | Kind::Strike { rects: r, .. } => *r = rects,
                    _ => {}
                }
                self.draw = Some(Draw::Markup { page, start, start_pdf, ann });
            }
            Some(Draw::Move { id, orig, start, moved }) => {
                let page = orig.page as usize - 1;
                let pt = self.screen_to_pdf(page, p);
                let moved_now = moved || self.pdf_to_screen(page, start).distance(p) > 3.0;
                if moved_now {
                    if !moved {
                        self.checkpoint();
                    }
                    let mut a = orig.clone();
                    if !a.is_markup() {
                        a.translate(pt[0] - start[0], pt[1] - start[1]);
                        self.replace(a);
                    }
                }
                self.draw = Some(Draw::Move { id, orig, start, moved: moved_now });
            }
            Some(Draw::Resize { id, orig, handle }) => {
                let a = self.resized(&orig, handle, p, mods.shift);
                self.replace(a);
                self.draw = Some(Draw::Resize { id, orig, handle });
            }
            Some(Draw::Erase { before, erased }) => {
                self.draw = Some(Draw::Erase { before, erased });
                self.erase_at(p);
            }
            None => {}
        }
    }

    fn end(&mut self, ctx: &egui::Context, p: Pos2) {
        self.drag(p, egui::Modifiers::NONE);
        match self.draw.take() {
            Some(Draw::Draft { page, ann, .. }) => {
                let b = self.pdf_rect_to_screen(page, ann.bounds());
                let big = b.width() > 4.0 || b.height() > 4.0;
                if matches!(ann.kind, Kind::Ink { .. }) || big {
                    self.add(ann);
                }
            }
            Some(Draw::Markup { page, start, ann, .. }) => {
                let mut ann = ann;
                let text = match (start, self.text.get(&page)) {
                    (Some(s), Some(t)) => t.nearest(self.screen_to_pdf(page, p), f64::INFINITY).map(|e| t.text_between(s.min(e), s.max(e))),
                    _ => None,
                };
                let empty = match &mut ann.kind {
                    Kind::Highlight { rects, text: t } | Kind::Underline { rects, text: t } | Kind::Strike { rects, text: t } => {
                        *t = text.filter(|x| !x.is_empty());
                        rects.is_empty()
                    }
                    _ => true,
                };
                if empty {
                    self.toast_hint = Some(tr("Kein Text an dieser Stelle"));
                } else {
                    self.add(ann);
                }
            }
            Some(Draw::Move { id, moved, orig, .. }) => {
                if !moved && (matches!(orig.kind, Kind::Text { .. } | Kind::Note { .. })) {
                    self.start_editing(&id);
                }
            }
            Some(Draw::Erase { before, erased }) => {
                if erased {
                    self.undo.push(super::Snapshot { annots: before, base: None });
                    self.redo.clear();
                    self.dirty = true;
                }
            }
            Some(Draw::Resize { .. }) | None => {}
        }
        let _ = ctx;
    }

    /// A second finger arrived (or the OS cancelled): nothing of this gesture remains.
    fn cancel(&mut self) {
        match self.draw.take() {
            Some(Draw::Move { orig, moved, .. }) => {
                if moved {
                    self.replace(orig);
                    self.undo.pop();
                }
            }
            Some(Draw::Resize { orig, .. }) => {
                self.replace(orig);
                self.undo.pop();
            }
            Some(Draw::Erase { before, erased }) => {
                if erased {
                    self.annots = before;
                }
            }
            Some(Draw::Draft { .. }) | Some(Draw::Markup { .. }) | None => {}
        }
    }

    fn tap(&mut self, ctx: &egui::Context, p: Pos2, _kind: PointerKind, now: f64) {
        // double tap → zoom in / back
        if (self.tool == Tool::Hand || !self.tool.is_tap())
            && let Some(Action::DoubleTap { pos }) = self.arbiter.classify_tap([p.x, p.y], now)
        {
            let target = if self.zoom_relative() > 1.5 { 1.0 / self.zoom_relative() } else { 2.0 };
            self.zoom_at(pos2(pos[0], pos[1]), target, now);
            return;
        }
        let had_edit = self.editing.is_some();
        self.commit_editing();
        if self.tool.is_tap() {
            if had_edit {
                return;
            }
            if let Some(page) = self.page_at(p, 0.0) {
                self.place(ctx, page, p);
            }
            return;
        }
        // links
        if let Some(page) = self.page_at(p, 0.0) {
            let pt = self.screen_to_pdf(page, p);
            if let Some(l) = self
                .links
                .iter()
                .find(|l| l.page == page && pt[0] >= l.rect[0] && pt[0] <= l.rect[2] && pt[1] >= l.rect[1] && pt[1] <= l.rect[3])
                .cloned()
            {
                match l.target {
                    ops::LinkTarget::Page(n) => self.go_to(n),
                    ops::LinkTarget::Uri(u) => crate::platform::open_url(&u),
                }
                return;
            }
        }
        // tap on a comment selects it (with any non-drawing tool); empty space deselects
        if matches!(self.tool, Tool::Hand | Tool::Select) {
            self.selected = self.annot_at(p);
            if let Some(id) = self.selected.clone()
                && let Some(a) = self.find(&id)
                && matches!(a.kind, Kind::Note { .. })
            {
                self.start_editing(&id);
            }
        } else {
            self.selected = None;
        }
    }

    /// Zoom relative to "page width".
    pub fn zoom_relative(&self) -> f32 {
        let w = self.layout.get(self.current_page).map(|r| r.width()).unwrap_or(600.0);
        self.zoom * w / (self.canvas.width() - 2.0 * PAD).max(100.0)
    }

    fn place(&mut self, ctx: &egui::Context, page: usize, p: Pos2) {
        let g = self.geom(page);
        let r = self.page_rect(page);
        let v = (p - r.min) / self.zoom;
        let (vx, vy) = (v.x as f64, v.y as f64);
        let o = self.opts.clone();
        match self.tool {
            Tool::Text => {
                let preset = o.preset_text.clone();
                let size = if preset.is_some() { o.font_size * 1.2 } else { o.font_size };
                let w = match &preset {
                    Some(t) => size * (t.chars().count() as f64 * 0.62).max(1.2) + 4.0,
                    None => (size * 14.0).max(120.0),
                };
                let h = size * 1.45;
                let rect = g.rect_to_pdf([vx, vy - h / 2.0, vx + w, vy + h / 2.0]);
                let bold = preset.as_ref().is_some_and(|t| t.chars().count() == 1);
                let a = self.new_annot(
                    page,
                    o.color(Tool::Text),
                    o.opacity,
                    Kind::Text { rect, text: preset.clone().unwrap_or_default(), font_size: size, bold },
                );
                let id = a.id.clone();
                self.add(a);
                if preset.is_none() {
                    self.start_editing(&id);
                } else {
                    self.selected = Some(id);
                }
            }
            Tool::Note => {
                let at = g.to_pdf([vx - 4.0, vy - 4.0]);
                let a = self.new_annot(page, o.color(Tool::Note), 1.0, Kind::Note { at, text: String::new() });
                let id = a.id.clone();
                self.add(a);
                self.start_editing(&id);
            }
            Tool::Signature | Tool::Image | Tool::Stamp => {
                let Some(img) = o.pending_image.clone() else {
                    // nothing chosen yet → open the chooser
                    match self.tool {
                        Tool::Signature => self.dialog = Some(Dialog::Signature(crate::signature::SignaturePad::new(false))),
                        Tool::Stamp => self.dialog = Some(Dialog::StampPicker(String::new())),
                        _ => self.want_image = true,
                    }
                    return;
                };
                let w = match img.kind {
                    ImageKind::Signature => 170.0,
                    ImageKind::Stamp => 150.0,
                    ImageKind::Image => 220.0,
                };
                let h = w / img.aspect.max(0.05);
                let rect = g.rect_to_pdf([vx - w / 2.0, vy - h / 2.0, vx + w / 2.0, vy + h / 2.0]);
                let a = self.new_annot(page, "#000000".into(), 1.0, Kind::Image { rect, src: img.src.clone(), kind: img.kind });
                self.selected = Some(a.id.clone());
                self.add(a);
                // place once, then back to selecting (like v2)
                self.tool = Tool::Select;
            }
            _ => {}
        }
        let _ = ctx;
    }

    fn erase_at(&mut self, p: Pos2) {
        if let Some(id) = self.annot_at(p) {
            self.annots.retain(|a| a.id != id);
            if let Some(Draw::Erase { erased, .. }) = &mut self.draw {
                *erased = true;
            }
            self.dirty = true;
        }
    }

    fn resized(&self, a: &Annotation, handle: Handle, p: Pos2, keep_ratio: bool) -> Annotation {
        let page = a.page as usize - 1;
        let mut a = a.clone();
        match &mut a.kind {
            Kind::Line { from, to, .. } | Kind::Arrow { from, to, .. } => {
                let pt = self.screen_to_pdf(page, p);
                if handle == Handle::From {
                    *from = pt;
                } else {
                    *to = pt;
                }
                return a;
            }
            _ => {}
        }
        let Some(rect) = a.rect() else { return a };
        let g = self.geom(page);
        let v = g.rect_to_view(rect);
        let r = self.page_rect(page);
        let pv = (p - r.min) / self.zoom;
        let (px, py) = (pv.x as f64, pv.y as f64);
        let [mut x1, mut y1, mut x2, mut y2] = v;
        let min = 4.0;
        if handle.has('w') {
            x1 = px.min(x2 - min);
        }
        if handle.has('e') {
            x2 = px.max(x1 + min);
        }
        if handle.has('n') {
            y1 = py.min(y2 - min);
        }
        if handle.has('s') {
            y2 = py.max(y1 + min);
        }
        let image = matches!(a.kind, Kind::Image { .. });
        if (keep_ratio || image) && handle.corner() {
            let ratio = (v[2] - v[0]) / (v[3] - v[1]).max(1e-6);
            let h = (x2 - x1) / ratio;
            if handle.has('n') {
                y1 = y2 - h;
            } else {
                y2 = y1 + h;
            }
        }
        let new = g.rect_to_pdf([x1, y1, x2, y2]);
        if let Kind::Text { font_size, .. } = &mut a.kind
            && keep_ratio
            && handle.corner()
        {
            *font_size *= (x2 - x1) / (v[2] - v[0]).max(1e-6);
        }
        if let Some(r) = a.rect_mut() {
            *r = new;
        }
        a
    }

    // ------------------------------------------------------------------ text editing

    pub fn start_editing(&mut self, id: &str) {
        if let Some(a) = self.find(id) {
            self.edit_buf = match &a.kind {
                Kind::Text { text, .. } | Kind::Note { text, .. } => text.clone(),
                _ => return,
            };
            self.editing = Some(id.to_string());
            self.selected = Some(id.to_string());
        }
    }

    pub fn commit_editing(&mut self) {
        let Some(id) = self.editing.take() else { return };
        let Some(a) = self.find(&id).cloned() else { return };
        let text = std::mem::take(&mut self.edit_buf);
        match &a.kind {
            Kind::Text { text: old, rect, font_size, bold } => {
                if text.trim().is_empty() {
                    self.delete(&id);
                    return;
                }
                if *old == text {
                    return;
                }
                // grow the box to fit the wrapped text
                let page = a.page as usize - 1;
                let g = self.geom(page);
                let v = g.rect_to_view(*rect);
                let lines = lightpdf_core::text::wrap_text(
                    &lightpdf_core::text::safe_text(&text),
                    *font_size,
                    *bold,
                    (v[2] - v[0] - 4.0).max(10.0),
                );
                let h = (lines.len() as f64 * font_size * 1.22 + font_size * 0.5).max(font_size * 1.3);
                let mut b = a.clone();
                if let Kind::Text { text: t, rect: r, .. } = &mut b.kind {
                    *t = text;
                    *r = g.rect_to_pdf([v[0], v[1], v[2], v[1] + h]);
                }
                self.update(b);
            }
            Kind::Note { text: old, .. } if *old != text => {
                let mut b = a.clone();
                if let Kind::Note { text: t, .. } = &mut b.kind {
                    *t = text;
                }
                self.update(b);
            }
            _ => {}
        }
    }
}

pub fn dist_to_segment(p: Pos2, a: Pos2, b: Pos2) -> f32 {
    let ab = b - a;
    let t = ((p - a).dot(ab) / ab.length_sq().max(1e-6)).clamp(0.0, 1.0);
    p.distance(a + ab * t)
}
