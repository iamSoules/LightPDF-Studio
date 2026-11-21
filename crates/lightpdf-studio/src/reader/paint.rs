//! Drawing pages, comments, drafts, search hits and the selection.

use super::*;
use crate::theme::{Palette, color_of};
use eframe::egui::{Color32, CornerRadius, FontId, Painter, Pos2, Shape, Stroke, StrokeKind, pos2, vec2};
use lightpdf_core::annotio::arrow_head;

const UV: Rect = Rect { min: Pos2 { x: 0.0, y: 0.0 }, max: Pos2 { x: 1.0, y: 1.0 } };

fn with_alpha(c: Color32, a: f64) -> Color32 {
    Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), (a.clamp(0.0, 1.0) * 255.0) as u8)
}

impl Reader {
    /// Requests renders for visible pages and drops textures far away.
    pub(super) fn manage_textures(&mut self, ctx: &egui::Context, now: f64) {
        let ppp = ctx.pixels_per_point() as f64;
        let max_side = ctx.input(|i| i.max_texture_side).clamp(2048, 8192) as f64;
        let max_side = if crate::platform::MOBILE { max_side.min(4096.0) } else { max_side };
        let settled = now - self.last_zoom_change > 0.14 && self.arbiter.active_pointers() < 2;
        let visible = self.visible_pages(self.canvas.height() * 0.6);
        let want = self.zoom as f64 * ppp;
        let Some(pool) = &self.pool else { return };
        let mut new_tag = false;
        for &i in &visible {
            let g = self.geom(i);
            let big = g.width().max(g.height());
            let scale = want.min(max_side / big);
            let have = self.textures.get(&i);
            let need = match have {
                None => true,
                Some(t) => t.rotation != self.view_rot || (settled && ((t.scale / scale) - 1.0).abs() > 0.12),
            };
            let pending = self.requested.get(&i).is_some_and(|r| ((r.0 / scale) - 1.0).abs() < 0.05 && r.1 == self.view_rot);
            if need && !pending {
                if !new_tag && settled {
                    self.tag += 1;
                    new_tag = true;
                }
                // while zooming, a quick low-res version first if there's nothing at all
                let s = if have.is_none() && !settled { scale.min(1.0 * ppp) } else { scale };
                pool.submit(Job::Render { page: i, scale: s, rotation: self.view_rot, region: None, tag: self.tag });
                self.requested.insert(i, (s, self.view_rot));
            }
        }
        if new_tag {
            pool.discard_below(self.tag);
        }
        // Detail tile: the page is larger than a texture may be → render only the visible part sharply
        self.detail_wanted(ctx, max_side, want, settled);
        // forget textures of far away pages
        let keep: HashSet<usize> = self.visible_pages(self.canvas.height() * 2.0).into_iter().collect();
        self.textures.retain(|k, _| keep.contains(k));
        self.requested.retain(|k, _| keep.contains(k));
        if self.detail.as_ref().is_some_and(|d| !keep.contains(&d.page) || d.rotation != self.view_rot) {
            self.detail = None;
        }
    }

    fn detail_wanted(&mut self, _ctx: &egui::Context, max_side: f64, want: f64, settled: bool) {
        if !settled {
            return;
        }
        let page = self.current_page;
        if page >= self.pages.len() {
            return;
        }
        let g = self.geom(page);
        if g.width().max(g.height()) * want <= max_side {
            self.detail = None;
            self.detail_req = None;
            return;
        }
        let pr = self.page_rect(page);
        let vis = pr.intersect(self.canvas.expand(64.0));
        if !vis.is_positive() {
            return;
        }
        let z = self.zoom as f64;
        let region = [
            ((vis.min.x - pr.min.x) as f64 / z).max(0.0),
            ((vis.min.y - pr.min.y) as f64 / z).max(0.0),
            ((vis.max.x - pr.min.x) as f64 / z).min(g.width()),
            ((vis.max.y - pr.min.y) as f64 / z).min(g.height()),
        ];
        let covered = |r: &lightpdf_core::model::Rect, s: f64| {
            r[0] <= region[0] + 0.5
                && r[1] <= region[1] + 0.5
                && r[2] >= region[2] - 0.5
                && r[3] >= region[3] - 0.5
                && ((s / want) - 1.0).abs() < 0.05
        };
        if self.detail.as_ref().is_some_and(|d| d.page == page && covered(&d.region, d.scale)) {
            return;
        }
        if self.detail_req.as_ref().is_some_and(|d| d.0 == page && covered(&d.1, d.2) && d.3 == self.view_rot) {
            return;
        }
        // a bit of margin so small pans don't need a new tile
        let m = 0.15;
        let (w, h) = (region[2] - region[0], region[3] - region[1]);
        let region = [
            (region[0] - w * m).max(0.0),
            (region[1] - h * m).max(0.0),
            (region[2] + w * m).min(g.width()),
            (region[3] + h * m).min(g.height()),
        ];
        if let Some(pool) = &self.pool {
            pool.submit(Job::Render { page, scale: want, rotation: self.view_rot, region: Some(region), tag: self.tag });
        }
        self.detail_req = Some((page, region, want, self.view_rot));
    }

    pub(super) fn paint(&mut self, ctx: &egui::Context, painter: &Painter, pal: &Palette, reading: Reading) {
        painter.rect_filled(self.canvas, 0.0, pal.viewer);
        let tint = match reading {
            Reading::Sepia => Color32::from_rgb(255, 238, 208),
            _ => Color32::WHITE,
        };
        let paper = match reading {
            Reading::Night => Color32::from_rgb(28, 30, 27),
            Reading::Sepia => Color32::from_rgb(246, 236, 214),
            Reading::Normal => Color32::WHITE,
        };
        let visible = self.visible_pages(0.0);
        for &i in &visible {
            let r = self.page_rect(i);
            painter.add(Shape::rect_filled(
                r.translate(vec2(0.0, 2.0)).expand(1.0),
                2.0,
                Color32::from_black_alpha(if pal.dark { 90 } else { 28 }),
            ));
            painter.rect_filled(r, 0.0, paper);
            if let Some(t) = self.textures.get(&i) {
                painter.image(t.tex.id(), r, UV, tint);
            }
            if let Some(d) = self.detail.as_ref().filter(|d| d.page == i && d.rotation == self.view_rot) {
                let z = self.zoom;
                let dr = Rect::from_min_max(
                    r.min + vec2(d.region[0] as f32, d.region[1] as f32) * z,
                    r.min + vec2(d.region[2] as f32, d.region[3] as f32) * z,
                );
                painter.image(d.tex.id(), dr, UV, tint);
            }
        }
        let clip = painter.clone().with_clip_rect(self.canvas);
        // search hits
        for (k, (page, rects)) in self.search.hits.iter().enumerate() {
            if !visible.contains(page) {
                continue;
            }
            let cur = k == self.search.current;
            for r in rects {
                let s = self.pdf_rect_to_screen(*page, *r).expand(1.0);
                clip.rect_filled(
                    s,
                    2.0,
                    if cur { Color32::from_rgba_unmultiplied(255, 140, 0, 110) } else { Color32::from_rgba_unmultiplied(255, 220, 0, 80) },
                );
            }
        }
        // comments
        let annots = self.annots.clone();
        for a in annots.iter().filter(|a| visible.contains(&(a.page as usize - 1))) {
            if self.editing.as_deref() == Some(a.id.as_str()) && matches!(a.kind, Kind::Text { .. }) {
                continue; // drawn by the text editor
            }
            self.paint_annot(ctx, &clip, a, pal);
        }
        // drafts
        match &self.draw {
            Some(Draw::Draft { ann, .. }) | Some(Draw::Markup { ann, .. }) => {
                let a = ann.clone();
                self.paint_annot(ctx, &clip, &a, pal);
            }
            _ => {}
        }
        // selection
        if let Some(a) = self.selected.as_deref().and_then(|id| self.find(id)).cloned()
            && visible.contains(&(a.page as usize - 1))
            && self.editing.as_deref() != Some(a.id.as_str())
        {
            let b = self.annot_screen_bounds(&a).expand(3.0);
            if !matches!(a.kind, Kind::Line { .. } | Kind::Arrow { .. }) {
                clip.add(Shape::dashed_line(
                    &[b.left_top(), b.right_top(), b.right_bottom(), b.left_bottom(), b.left_top()],
                    Stroke::new(1.5, pal.accent),
                    5.0,
                    3.0,
                ));
            }
            if self.tool == Tool::Select || self.tool == Tool::Hand {
                let hs = self.handle_size();
                for (_, p) in self.handles(&a) {
                    clip.circle(p, hs / 2.0, Color32::WHITE, Stroke::new(2.0, pal.accent));
                }
            }
        }
    }

    fn paint_annot(&mut self, ctx: &egui::Context, p: &Painter, a: &Annotation, _pal: &Palette) {
        let page = a.page as usize - 1;
        let z = self.zoom;
        let col = color_of(&a.color);
        match &a.kind {
            Kind::Highlight { rects, .. } => {
                for r in rects {
                    p.rect_filled(self.pdf_rect_to_screen(page, *r), CornerRadius::same(1), with_alpha(col, a.opacity * 0.42));
                }
            }
            Kind::Underline { rects, .. } | Kind::Strike { rects, .. } => {
                let under = matches!(a.kind, Kind::Underline { .. });
                let vertical = !self.geom(page).rotation.is_multiple_of(180);
                for r in rects {
                    let s = self.pdf_rect_to_screen(page, *r);
                    let c = with_alpha(col, a.opacity);
                    if vertical {
                        let w = s.width();
                        let rot = self.geom(page).rotation;
                        let x = if under { if rot == 90 { s.min.x + w * 0.08 } else { s.max.x - w * 0.08 } } else { s.center().x };
                        p.line_segment([pos2(x, s.min.y), pos2(x, s.max.y)], Stroke::new((w * 0.07).max(1.0), c));
                    } else {
                        let h = s.height();
                        let y = if under { s.max.y - h * 0.08 } else { s.min.y + h * 0.55 };
                        p.line_segment([pos2(s.min.x, y), pos2(s.max.x, y)], Stroke::new((h * 0.07).max(1.0), c));
                    }
                }
            }
            Kind::Ink { paths, width, highlighter } => {
                let alpha = if *highlighter { a.opacity.min(0.5) } else { a.opacity };
                let stroke = Stroke::new((*width as f32 * z).max(0.8), with_alpha(col, alpha));
                for path in paths {
                    let pts: Vec<Pos2> = path.iter().map(|q| self.pdf_to_screen(page, *q)).collect();
                    if pts.len() == 1 {
                        p.circle_filled(pts[0], stroke.width / 2.0, stroke.color);
                    } else {
                        p.add(Shape::line(pts.clone(), stroke));
                        // round caps
                        p.circle_filled(pts[0], stroke.width / 2.0, stroke.color);
                        p.circle_filled(*pts.last().unwrap(), stroke.width / 2.0, stroke.color);
                    }
                }
            }
            Kind::Rect { rect, width, fill } => {
                let s = self.pdf_rect_to_screen(page, *rect);
                if let Some(f) = fill {
                    p.rect_filled(s, 0.0, with_alpha(color_of(f), a.opacity));
                }
                p.rect_stroke(s, 0.0, Stroke::new(*width as f32 * z, with_alpha(col, a.opacity)), StrokeKind::Middle);
            }
            Kind::Ellipse { rect, width, fill } => {
                let s = self.pdf_rect_to_screen(page, *rect);
                let radius = s.size() / 2.0;
                if let Some(f) = fill {
                    p.add(Shape::ellipse_filled(s.center(), radius, with_alpha(color_of(f), a.opacity)));
                }
                p.add(Shape::ellipse_stroke(s.center(), radius, Stroke::new(*width as f32 * z, with_alpha(col, a.opacity))));
            }
            Kind::Line { from, to, width } | Kind::Arrow { from, to, width } => {
                let stroke = Stroke::new((*width as f32 * z).max(0.8), with_alpha(col, a.opacity));
                let (f, t) = (self.pdf_to_screen(page, *from), self.pdf_to_screen(page, *to));
                p.line_segment([f, t], stroke);
                if matches!(a.kind, Kind::Arrow { .. }) {
                    let [h1, h2] = arrow_head(*from, *to, *width);
                    p.add(Shape::line(vec![self.pdf_to_screen(page, h1), t, self.pdf_to_screen(page, h2)], stroke));
                }
            }
            Kind::Text { rect, text, font_size, bold } => {
                let s = self.pdf_rect_to_screen(page, *rect);
                let size = (*font_size as f32 * z).max(1.0);
                let mut job = egui::text::LayoutJob::default();
                job.wrap.max_width = (s.width() - 4.0 * z).max(4.0);
                job.append(
                    text,
                    0.0,
                    egui::TextFormat {
                        font_id: FontId::proportional(size),
                        color: with_alpha(col, a.opacity),
                        line_height: Some(size * 1.22),
                        ..Default::default()
                    },
                );
                let galley = ctx.fonts_mut(|f| f.layout_job(job));
                let pos = s.min + vec2(2.0 * z, 2.0 * z);
                p.galley(pos, galley.clone(), col);
                if *bold {
                    p.galley(pos + vec2(0.45 * z.min(2.0), 0.0), galley, col);
                }
            }
            Kind::Note { at, .. } => {
                let o = self.pdf_to_screen(page, *at);
                let r = Rect::from_min_size(o, vec2(22.0, 20.0));
                p.rect_filled(r, CornerRadius::same(4), col);
                p.rect_stroke(r, CornerRadius::same(4), Stroke::new(1.0, Color32::from_black_alpha(70)), StrokeKind::Inside);
                for i in 0..3 {
                    let y = r.min.y + 6.0 + i as f32 * 4.0;
                    p.line_segment(
                        [pos2(r.min.x + 5.0, y), pos2(r.max.x - if i == 2 { 9.0 } else { 5.0 }, y)],
                        Stroke::new(1.2, Color32::from_black_alpha(140)),
                    );
                }
            }
            Kind::Image { rect, .. } => {
                let s = self.pdf_rect_to_screen(page, *rect);
                if let Some(t) = self.image_texture(ctx, a) {
                    p.image(t.id(), s, UV, with_alpha(Color32::WHITE, a.opacity));
                } else {
                    p.rect_stroke(s, 0.0, Stroke::new(1.0, Color32::GRAY), StrokeKind::Inside);
                }
            }
            Kind::Redact { rect } => {
                let s = self.pdf_rect_to_screen(page, *rect);
                p.rect_filled(s, 0.0, Color32::from_black_alpha(140));
                p.add(Shape::dashed_line(
                    &[s.left_top(), s.right_top(), s.right_bottom(), s.left_bottom(), s.left_top()],
                    Stroke::new(1.2, Color32::from_rgb(220, 50, 50)),
                    4.0,
                    3.0,
                ));
            }
        }
    }
}
