//! Reader UI around the canvas: toolbars, side panel, search, selection bar, editors.

use super::canvas::Fit;
use super::*;
use crate::theme::{self, Palette, SWATCHES, color_of};
use eframe::egui::{Align, Align2, Color32, Key, Layout, Pos2, RichText, Sense, Stroke, pos2, vec2};

/// Stamps offered by default (like v2).
pub const STAMPS: [(&str, &str); 5] =
    [("Genehmigt", "#2e8b57"), ("Entwurf", "#2f6bd6"), ("Vertraulich", "#c0473e"), ("Abgelehnt", "#c0473e"), ("Erledigt", "#2e8b57")];

fn tool_button(ui: &mut egui::Ui, r: &mut Reader, t: Tool, pal: &Palette, touch: bool) {
    let sel = r.tool == t;
    let size = if touch { vec2(44.0, 40.0) } else { vec2(34.0, 30.0) };
    let mut text = RichText::new(t.icon()).size(if touch { 19.0 } else { 16.0 });
    if sel {
        text = text.color(pal.accent);
    }
    let resp = ui.add_sized(size, egui::Button::selectable(sel, text)).on_hover_text(t.label());
    if resp.clicked() {
        r.set_tool(t);
    }
}

impl Reader {
    pub fn set_tool(&mut self, t: Tool) {
        self.commit_editing();
        if self.tool == t && t != Tool::Hand {
            self.tool = Tool::Hand;
            return;
        }
        self.tool = t;
        if !matches!(t, Tool::Select | Tool::Hand) {
            self.selected = None;
        }
        match t {
            Tool::Text => {}
            _ => self.opts.preset_text = None,
        }
        if !matches!(t, Tool::Signature | Tool::Image | Tool::Stamp) {
            self.opts.pending_image = None;
        }
        match t {
            Tool::Signature if self.opts.pending_image.is_none() => {
                self.dialog = Some(Dialog::Signature(crate::signature::SignaturePad::new(false)))
            }
            Tool::Stamp if self.opts.pending_image.is_none() => self.dialog = Some(Dialog::StampPicker(String::new())),
            Tool::Image if self.opts.pending_image.is_none() => self.want_image = true,
            _ => {}
        }
    }

    fn tools_of_tab(&self) -> Vec<Tool> {
        match self.tab {
            Tab::Read => vec![Tool::Hand, Tool::Select],
            Tab::Comment => vec![
                Tool::Select,
                Tool::Highlight,
                Tool::Underline,
                Tool::Strike,
                Tool::Ink,
                Tool::Highlighter,
                Tool::Rect,
                Tool::Ellipse,
                Tool::Line,
                Tool::Arrow,
                Tool::Text,
                Tool::Note,
                Tool::Stamp,
                Tool::Image,
                Tool::Eraser,
            ],
            Tab::Fill => vec![Tool::Select, Tool::Text, Tool::Signature, Tool::Redact],
            Tab::Pages => vec![],
        }
    }

    // ------------------------------------------------------------------ main frame

    pub fn ui(&mut self, s: &mut Studio, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        let pal = s.pal;
        let touch = s.touch;
        let now = ctx.input(|i| i.time);
        let reading = s.storage.lib.settings.reading;
        let mode = s.storage.lib.settings.scroll_mode;
        let spread = s.storage.lib.settings.spread;
        let narrow = ctx.content_rect().width() < 720.0;

        if reading != self.reading_applied {
            self.reading_applied = reading;
            self.textures.clear();
            self.requested.clear();
            self.detail = None;
            self.detail_req = None;
        }
        self.poll_renders(&ctx, reading);
        if !self.search.active_query.is_empty() {
            self.run_search();
        }
        if let Some(h) = self.toast_hint.take() {
            s.toast(h);
        }
        if std::mem::take(&mut self.want_image) {
            if let Some(f) = s.pick(BrowsePurpose::Images) {
                self.files_picked(BrowsePurpose::Images, f);
            }
            if self.opts.pending_image.is_none() && self.tool == Tool::Image && crate::platform::native_dialogs() {
                self.tool = Tool::Select;
            }
        }
        if let Some(bytes) = self.take_pending_insert() {
            let at = self.current_page + 1;
            self.page_op(&mut s.tasks, &ctx, tr("Bearbeite Seiten …"), move |b| ops::insert_pdf(b, &bytes, at));
        }

        // ---------------------------------------------------------------- top bar
        if !self.fullscreen {
            egui::Panel::top("rd-top")
                .frame(egui::Frame::new().fill(pal.panel).inner_margin(egui::Margin::symmetric(8, 6)).stroke(Stroke::new(1.0, pal.line)))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        if ui
                            .add_sized([40.0, 34.0], egui::Button::new(RichText::new("⬅").size(18.0)))
                            .on_hover_text(tr("Bibliothek"))
                            .clicked()
                        {
                            self.request_close();
                        }
                        let title = format!("{}{}", self.meta.name, if self.dirty { " •" } else { "" });
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            self.top_right(s, ui, &ctx, &pal, narrow);
                            ui.add(egui::Label::new(RichText::new(title).strong()).truncate());
                        });
                    });
                    if !narrow {
                        ui.add_space(2.0);
                        self.toolbar(s, ui, &ctx, &pal, touch, now, mode, spread);
                    }
                });
        }
        if narrow && !self.fullscreen {
            egui::Panel::bottom("rd-bottom")
                .frame(egui::Frame::new().fill(pal.panel).inner_margin(egui::Margin::symmetric(6, 6)).stroke(Stroke::new(1.0, pal.line)))
                .show(ui, |ui| {
                    self.toolbar(s, ui, &ctx, &pal, touch, now, mode, spread);
                });
        }

        // ---------------------------------------------------------------- search bar
        if self.search.open && !self.fullscreen {
            egui::Panel::top("rd-search").frame(egui::Frame::new().fill(pal.bg2).inner_margin(egui::Margin::symmetric(10, 6))).show(
                ui,
                |ui| {
                    ui.horizontal(|ui| {
                        let r =
                            ui.add(egui::TextEdit::singleline(&mut self.search.query).hint_text(tr("Suchen …")).desired_width(if narrow {
                                150.0
                            } else {
                                260.0
                            }));
                        if std::mem::take(&mut self.search.focus) {
                            r.request_focus();
                        }
                        if r.changed() {
                            self.run_search();
                        }
                        if r.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                            let back = ui.input(|i| i.modifiers.shift);
                            self.search_step(if back { -1 } else { 1 });
                            r.request_focus();
                        }
                        if ui.button("⏶").clicked() {
                            self.search_step(-1);
                        }
                        if ui.button("⏷").clicked() {
                            self.search_step(1);
                        }
                        let n = self.search.hits.len();
                        let scanning = self.search.scanned.len() < self.pages.len() && !self.search.active_query.is_empty();
                        let label = if self.search.active_query.is_empty() {
                            String::new()
                        } else if n == 0 && !scanning {
                            tr("Keine Treffer")
                        } else {
                            format!("{} / {}{}", if n > 0 { self.search.current + 1 } else { 0 }, n, if scanning { " …" } else { "" })
                        };
                        ui.label(RichText::new(label).color(pal.muted));
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if ui.button("✖").clicked() {
                                self.search = Search::default();
                            }
                        });
                    });
                },
            );
        }

        // ---------------------------------------------------------------- side panel
        if let Some(side) = self.side.filter(|_| !self.fullscreen) {
            let w = if narrow { ctx.content_rect().width() * 0.62 } else { 230.0 };
            egui::Panel::left("rd-side").exact_size(w).resizable(false).frame(egui::Frame::new().fill(pal.bg2).inner_margin(8)).show(
                ui,
                |ui| {
                    self.side_panel(ui, &ctx, &pal, side, narrow);
                },
            );
        }

        // ---------------------------------------------------------------- canvas
        egui::CentralPanel::no_frame().show(ui, |ui| {
            let rect = ui.max_rect();
            let first = self.canvas == Rect::NOTHING;
            if rect != self.canvas {
                let old = self.canvas;
                self.canvas = rect;
                if !first && old.width() > 0.0 {
                    // panels appeared / disappeared: keep the document where it is on screen
                    self.offset -= rect.min - old.min;
                    self.clamp_offset();
                    // window resized (rotation, split screen, foldable): re-fit unless zoomed by hand
                    if (rect.width() - old.width()).abs() > 40.0 && !self.user_zoomed {
                        let page = self.current_page;
                        self.pending_fit = Some(Fit::Auto);
                        self.pending_goto = Some(page);
                    }
                }
            }
            self.ensure_layout(mode, spread);
            if let Some(f) = self.pending_fit.take() {
                self.fit(f, now);
            }
            if let Some(p) = self.pending_goto.take() {
                self.go_to(p);
            }
            let resp = ui.allocate_rect(rect, Sense::hover());
            let layer = ui.layer_id();
            let blocked = |p: Pos2| ctx.layer_id_at(p).is_some_and(|l| l != layer);
            if self.organizer.is_none() && self.dialog.is_none() && !s.picker_open() && s.password.is_none() {
                self.handle_input(&ctx, &blocked);
            }
            self.clamp_offset();
            self.manage_textures(&ctx, now);
            let painter = ui.painter_at(rect);
            self.paint(&ctx, &painter, &pal, reading);

            // cursor
            if resp.hovered() {
                let icon = match self.tool {
                    Tool::Hand => {
                        if self.arbiter.is_navigating() {
                            egui::CursorIcon::Grabbing
                        } else {
                            egui::CursorIcon::Grab
                        }
                    }
                    Tool::Select => egui::CursorIcon::Default,
                    Tool::Text | Tool::Highlight | Tool::Underline | Tool::Strike => egui::CursorIcon::Text,
                    _ => egui::CursorIcon::Crosshair,
                };
                ctx.set_cursor_icon(icon);
            }

            // page indicator & zoom
            self.status_chip(ui, &ctx, &pal, now, touch);
        });

        self.selection_bar(&ctx, &pal);
        self.text_editor(&ctx, &pal);
        self.note_editor(&ctx, &pal);
        if let Some(org) = &mut self.organizer {
            let sizes: Vec<(f64, f64)> = self
                .pages
                .iter()
                .map(|p| {
                    let g = p.geom(0);
                    (g.width(), g.height())
                })
                .collect();
            let done = org.show(&ctx, &pal, &self.thumbs, self.pages.len(), &sizes);
            let wanted: Vec<usize> = org.wanted_thumbs.drain(..).collect();
            for p in wanted {
                self.request_thumb(p, 150.0);
            }
            if let Some(action) = done {
                let org = self.organizer.take().unwrap();
                org.apply(action, self, s, &ctx);
            }
        }
        if self.dialog.is_some() {
            crate::dialogs::reader_dialog(self, s, &ctx);
        }
        self.keyboard(s, &ctx, now);
        if self.close_requested {
            self.close_requested = false;
            self.commit_editing();
            if self.dirty {
                self.dialog = Some(Dialog::ConfirmClose);
            } else {
                self.closed = true;
                s.screen = Screen::Library;
            }
        }
    }

    fn top_right(&mut self, s: &mut Studio, ui: &mut egui::Ui, ctx: &egui::Context, pal: &Palette, narrow: bool) {
        ui.menu_button(RichText::new("⋮").size(18.0), |ui| {
            if ui.button(format!("💾  {}", tr("Speichern unter …"))).clicked() {
                self.save_as(s, false);
                ui.close();
            }
            if ui.button(format!("🔥  {}", tr("Kommentare einbrennen"))).on_hover_text(tr("Speichern unter …")).clicked() {
                self.save_as(s, true);
                ui.close();
            }
            if crate::platform::native_dialogs() && ui.button(format!("🖶  {}", tr("Drucken"))).clicked() {
                match self.export_bytes(false) {
                    Ok(b) => {
                        if let Err(e) = crate::platform::open_externally(&format!("{}.pdf", self.meta.name), &b) {
                            s.toast_err(e);
                        }
                    }
                    Err(e) => s.toast_err(e),
                }
                ui.close();
            }
            ui.separator();
            if ui.button(format!("ℹ  {}", tr("Eigenschaften"))).clicked() {
                self.dialog = crate::dialogs::properties(self);
                ui.close();
            }
            if ui.button(format!("⚙  {}", tr("Einstellungen"))).clicked() {
                s.show_settings = true;
                ui.close();
            }
        });
        let save = theme::primary(ui, pal, if narrow { "💾".to_string() } else { format!("💾 {}", tr("Speichern")) });
        if save.clicked() {
            self.commit_editing();
            self.save(s, false);
        }
        if ui.selectable_label(self.side.is_some(), "◧").on_hover_text(tr("Miniaturen")).clicked() {
            self.side = if self.side.is_some() { None } else { Some(Side::Thumbs) };
        }
        if ui.selectable_label(self.search.open, "🔍").on_hover_text(tr("Suchen …")).clicked() {
            self.search.open = !self.search.open;
            self.search.focus = self.search.open;
        }
        if ui.add_enabled(self.can_redo(), egui::Button::new("↷")).on_hover_text(tr("Wiederholen")).clicked() {
            self.redo(ctx);
        }
        if ui.add_enabled(self.can_undo(), egui::Button::new("↶")).on_hover_text(tr("Rückgängig")).clicked() {
            self.undo(ctx);
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn toolbar(
        &mut self,
        s: &mut Studio,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        pal: &Palette,
        touch: bool,
        now: f64,
        mode: ScrollMode,
        spread: bool,
    ) {
        // tabs
        ui.horizontal(|ui| {
            for (t, label) in [
                (Tab::Read, tr("Lesen")),
                (Tab::Comment, tr("Kommentieren")),
                (Tab::Fill, tr("Ausfüllen & Signieren")),
                (Tab::Pages, tr("Seiten")),
            ] {
                if ui.selectable_label(self.tab == t, RichText::new(label).strong()).clicked() {
                    self.tab = t;
                    if t == Tab::Read {
                        self.set_tool(Tool::Hand);
                    } else if t == Tab::Comment && self.tool == Tool::Hand {
                        self.tool = Tool::Select;
                    }
                }
            }
        });
        egui::ScrollArea::horizontal().id_salt("tools").scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden).show(
            ui,
            |ui| {
                ui.horizontal(|ui| {
                    for t in self.tools_of_tab() {
                        tool_button(ui, self, t, pal, touch);
                    }
                    match self.tab {
                        Tab::Read => self.read_controls(s, ui, now, pal, mode, spread),
                        Tab::Fill => {
                            ui.separator();
                            for (p, tip) in [("✓", "Haken"), ("✗", "Kreuz"), ("•", "Punkt")] {
                                if ui
                                    .add_sized(
                                        [if touch { 44.0 } else { 32.0 }, if touch { 40.0 } else { 30.0 }],
                                        egui::Button::selectable(self.tool == Tool::Text && self.opts.preset_text.as_deref() == Some(p), p),
                                    )
                                    .on_hover_text(tr(tip))
                                    .clicked()
                                {
                                    self.tool = Tool::Text;
                                    self.opts.preset_text = Some(p.into());
                                }
                            }
                            if ui.button(format!("📅 {}", tr("Datum"))).clicked() {
                                self.tool = Tool::Text;
                                self.opts.preset_text = Some(today());
                            }
                            if ui.button(format!("✍ {}", tr("Initialen"))).clicked() {
                                self.tool = Tool::Signature;
                                self.dialog = Some(Dialog::Signature(crate::signature::SignaturePad::new(true)));
                            }
                            ui.separator();
                            if ui.add_enabled(self.has_form, egui::Button::new(format!("📝 {}", tr("Formular ausfüllen")))).clicked() {
                                self.dialog = crate::dialogs::form_dialog(self);
                            }
                            let n_redact = self.annots.iter().filter(|a| matches!(a.kind, Kind::Redact { .. })).count();
                            if n_redact > 0
                                && ui
                                    .button(RichText::new(format!("⬛ {} ({n_redact})", tr("Schwärzungen anwenden"))).color(pal.danger))
                                    .clicked()
                            {
                                self.dialog = Some(Dialog::ConfirmRedact);
                            }
                        }
                        Tab::Pages => self.pages_controls(s, ui, ctx),
                        Tab::Comment => {}
                    }
                });
            },
        );
        // options of the current tool (the row is always there, so the page doesn't jump)
        if matches!(self.tab, Tab::Comment | Tab::Fill) {
            let h = if touch { 30.0 } else { 24.0 };
            ui.allocate_ui(vec2(ui.available_width(), h), |ui| {
                ui.set_min_height(h);
                if matches!(
                    self.tool,
                    Tool::Hand | Tool::Select | Tool::Eraser | Tool::Redact | Tool::Signature | Tool::Image | Tool::Stamp
                ) {
                    let hint = if touch { tr("Mit zwei Fingern zoomen – es entsteht keine Markierung.") } else { self.tool.label() };
                    ui.label(RichText::new(hint).small().color(pal.muted));
                } else {
                    self.tool_options(ui, pal, touch);
                }
            });
        }
    }

    fn read_controls(&mut self, s: &mut Studio, ui: &mut egui::Ui, now: f64, pal: &Palette, mode: ScrollMode, spread: bool) {
        ui.separator();
        if ui.button("−").on_hover_text(tr("Verkleinern")).clicked() {
            let c = self.canvas.center();
            self.zoom_at(c, 1.0 / 1.25, now);
        }
        ui.label(format!("{:.0} %", self.zoom * 100.0));
        if ui.button("+").on_hover_text(tr("Vergrößern")).clicked() {
            let c = self.canvas.center();
            self.zoom_at(c, 1.25, now);
        }
        if ui.button("↔").on_hover_text(tr("Seitenbreite")).clicked() {
            self.fit(Fit::Width, now);
        }
        if ui.button("⛶").on_hover_text(tr("Ganze Seite")).clicked() {
            self.fit(Fit::Page, now);
        }
        if ui.button("⟳").on_hover_text(tr("Ansicht drehen")).clicked() {
            self.view_rot = (self.view_rot + 90) % 360;
            self.layout_key = None;
            let p = self.current_page;
            self.pending_goto = Some(p);
            self.textures.clear();
            self.requested.clear();
            self.detail = None;
        }
        ui.separator();
        let st = &mut s.storage.lib.settings;
        let before = (st.reading, st.scroll_mode, st.spread);
        egui::ComboBox::new("reading", tr("Lesemodus"))
            .selected_text(match st.reading {
                Reading::Normal => tr("Normal"),
                Reading::Night => tr("Nachtmodus"),
                Reading::Sepia => tr("Sepia"),
            })
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut st.reading, Reading::Normal, tr("Normal"));
                ui.selectable_value(&mut st.reading, Reading::Night, tr("Nachtmodus"));
                ui.selectable_value(&mut st.reading, Reading::Sepia, tr("Sepia"));
            });
        egui::ComboBox::new("scrollmode", tr("Ansicht"))
            .selected_text(match (mode, spread) {
                (ScrollMode::Horizontal, _) => tr("Horizontal"),
                (_, true) => tr("Doppelseite"),
                (ScrollMode::Page, _) => tr("Einzelseite"),
                _ => tr("Fortlaufend"),
            })
            .show_ui(ui, |ui| {
                if ui.selectable_label(st.scroll_mode == ScrollMode::Vertical && !st.spread, tr("Fortlaufend")).clicked() {
                    st.scroll_mode = ScrollMode::Vertical;
                    st.spread = false;
                }
                if ui.selectable_label(st.scroll_mode == ScrollMode::Horizontal, tr("Horizontal")).clicked() {
                    st.scroll_mode = ScrollMode::Horizontal;
                    st.spread = false;
                }
                if ui.selectable_label(st.spread, tr("Doppelseite")).clicked() {
                    st.scroll_mode = ScrollMode::Vertical;
                    st.spread = true;
                }
            });
        if (st.reading, st.scroll_mode, st.spread) != before {
            s.storage.save();
            let p = self.current_page;
            self.pending_goto = Some(p);
        }
        if ui.button("⛶").on_hover_text(tr("Vollbild")).clicked() {
            self.fullscreen = true;
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Fullscreen(true));
        }
        let _ = pal;
    }

    fn pages_controls(&mut self, s: &mut Studio, ui: &mut egui::Ui, ctx: &egui::Context) {
        if ui.button(format!("▦ {}", tr("Seiten organisieren"))).clicked() {
            self.organizer = Some(crate::organizer::Organizer::new(self.pages.len(), self.current_page));
        }
        let cur = self.current_page;
        if ui.button(format!("⟳ {}", tr("Drehen"))).clicked() {
            self.page_op(&mut s.tasks, ctx, tr("Bearbeite Seiten …"), move |b| ops::rotate_pages(b, &[cur], 90));
        }
        if ui.button(format!("➕ {}", tr("Leere Seite"))).clicked() {
            self.page_op(&mut s.tasks, ctx, tr("Bearbeite Seiten …"), move |b| ops::insert_blank(b, cur + 1));
        }
        if ui.button(format!("📄 {}", tr("PDF einfügen …"))).clicked()
            && let Some(f) = s.pick(BrowsePurpose::InsertPdf)
        {
            self.files_picked(BrowsePurpose::InsertPdf, f);
        }
        if ui.add_enabled(self.pages.len() > 1, egui::Button::new(format!("🗑 {}", tr("Seite löschen")))).clicked() {
            self.page_op(&mut s.tasks, ctx, tr("Bearbeite Seiten …"), move |b| ops::delete_pages(b, &[cur]));
        }
        ui.separator();
        ui.menu_button(format!("🧰 {}", tr("Werkzeuge")), |ui| crate::dialogs::tools_menu(self, s, ui));
    }

    fn tool_options(&mut self, ui: &mut egui::Ui, pal: &Palette, touch: bool) {
        let t = self.tool;
        ui.horizontal_wrapped(|ui| {
            let current = self.opts.color(t);
            let sw = if touch { 26.0 } else { 20.0 };
            for c in SWATCHES {
                let (rect, r) = ui.allocate_exact_size(vec2(sw, sw), Sense::click());
                let selected = c.eq_ignore_ascii_case(&current);
                ui.painter().circle_filled(rect.center(), sw / 2.0 - 1.0, color_of(c));
                ui.painter().circle_stroke(
                    rect.center(),
                    sw / 2.0 - 1.0,
                    Stroke::new(if selected { 2.5 } else { 1.0 }, if selected { pal.accent } else { pal.line }),
                );
                if r.clicked() {
                    self.opts.colors.insert(t, c.to_string());
                }
            }
            if matches!(t, Tool::Ink | Tool::Rect | Tool::Ellipse | Tool::Line | Tool::Arrow | Tool::Highlighter) {
                ui.add(egui::Slider::new(&mut self.opts.width, 0.5..=12.0).text(tr("Stärke")).max_decimals(1));
            }
            if matches!(t, Tool::Rect | Tool::Ellipse) {
                let mut fill = self.opts.fill.is_some();
                if ui.checkbox(&mut fill, tr("Füllung")).changed() {
                    self.opts.fill = fill.then(|| self.opts.color(t));
                }
            }
            if matches!(t, Tool::Text) {
                ui.add(egui::Slider::new(&mut self.opts.font_size, 6.0..=48.0).text(tr("Schriftgröße")).max_decimals(0));
            }
            if !matches!(t, Tool::Highlighter | Tool::Note) {
                ui.add(egui::Slider::new(&mut self.opts.opacity, 0.1..=1.0).text(tr("Deckkraft")).max_decimals(2));
            }
        });
    }

    fn status_chip(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, pal: &Palette, now: f64, touch: bool) {
        let n = self.pages.len();
        if n == 0 {
            return;
        }
        let text = format!("{} / {}", self.current_page + 1, n);
        let pos = self.canvas.left_bottom() + vec2(12.0, -12.0);
        egui::Area::new(egui::Id::unique("page-chip")).fixed_pos(pos).pivot(Align2::LEFT_BOTTOM).order(egui::Order::Middle).show(
            ctx,
            |ui| {
                egui::Frame::new()
                    .fill(pal.panel.gamma_multiply(0.94))
                    .corner_radius(10)
                    .stroke(Stroke::new(1.0, pal.line))
                    .inner_margin(egui::Margin::symmetric(8, 4))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            if ui.small_button("⏴").clicked() && self.current_page > 0 {
                                let p = self.current_page - 1;
                                self.go_to(p);
                            }
                            ui.label(RichText::new(text).monospace());
                            if ui.small_button("⏵").clicked() && self.current_page + 1 < n {
                                let p = self.current_page + 1;
                                self.go_to(p);
                            }
                            if !touch {
                                ui.separator();
                                ui.label(RichText::new(format!("{:.0} %", self.zoom * 100.0)).small().color(pal.muted));
                            }
                            if self.fullscreen && ui.small_button("✖").clicked() {
                                self.fullscreen = false;
                                ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(false));
                            }
                        });
                    });
            },
        );
        let _ = (ui, now);
    }

    fn side_panel(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, pal: &Palette, side: Side, narrow: bool) {
        ui.horizontal(|ui| {
            for (sd, label) in [(Side::Thumbs, "▦"), (Side::Outline, "☰"), (Side::Comments, "🗨")] {
                let tip = match sd {
                    Side::Thumbs => tr("Miniaturen"),
                    Side::Outline => tr("Lesezeichen"),
                    Side::Comments => tr("Kommentare"),
                };
                if ui.selectable_label(side == sd, label).on_hover_text(tip).clicked() {
                    self.side = Some(sd);
                }
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui.button("✖").clicked() {
                    self.side = None;
                }
            });
        });
        ui.separator();
        let mut goto = None;
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| match side {
            Side::Thumbs => {
                let w = ui.available_width() - 16.0;
                let ppp = ctx.pixels_per_point();
                for i in 0..self.pages.len() {
                    let g = self.pages[i].geom(0);
                    let h = w * (g.height() / g.width()) as f32;
                    let (rect, r) = ui.allocate_exact_size(vec2(ui.available_width(), h + 22.0), Sense::click());
                    if ui.is_rect_visible(rect) {
                        self.request_thumb(i, w * ppp);
                        let img = Rect::from_min_size(pos2(rect.center().x - w / 2.0, rect.min.y), vec2(w, h));
                        ui.painter().rect_filled(img, 2.0, Color32::WHITE);
                        if let Some(t) = self.thumbs.get(&i) {
                            ui.painter().image(t.id(), img, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
                        }
                        let cur = i == self.current_page;
                        ui.painter().rect_stroke(
                            img,
                            2.0,
                            Stroke::new(if cur { 2.5 } else { 1.0 }, if cur { pal.accent } else { pal.line }),
                            egui::StrokeKind::Outside,
                        );
                        ui.painter().text(
                            pos2(rect.center().x, img.max.y + 4.0),
                            Align2::CENTER_TOP,
                            (i + 1).to_string(),
                            egui::FontId::proportional(12.0),
                            pal.muted,
                        );
                    }
                    if r.clicked() {
                        goto = Some(i);
                    }
                }
            }
            Side::Outline => {
                if self.outline.is_empty() {
                    ui.label(RichText::new(tr("Dieses Dokument enthält keine Lesezeichen.")).color(pal.muted));
                }
                fn walk(ui: &mut egui::Ui, items: &[OutlineItem], goto: &mut Option<usize>, depth: usize) {
                    for it in items {
                        ui.horizontal(|ui| {
                            ui.add_space(depth as f32 * 12.0);
                            let r = ui.add(egui::Button::new(&it.title).frame(false).wrap_mode(egui::TextWrapMode::Wrap));
                            if r.clicked() {
                                *goto = it.page;
                            }
                        });
                        walk(ui, &it.children, goto, depth + 1);
                    }
                }
                walk(ui, &self.outline, &mut goto, 0);
            }
            Side::Comments => {
                if self.annots.is_empty() {
                    ui.label(RichText::new(tr("Noch keine Kommentare")).color(pal.muted));
                }
                let mut list = self.annots.clone();
                list.sort_by(|a, b| a.page.cmp(&b.page).then(a.created_at.total_cmp(&b.created_at)));
                let mut select = None;
                for a in &list {
                    let text = a.contents().map(|s| s.to_string()).unwrap_or_else(|| a.type_name().to_string());
                    egui::Frame::new()
                        .fill(pal.panel)
                        .corner_radius(8)
                        .inner_margin(8)
                        .stroke(Stroke::new(1.0, if self.selected.as_deref() == Some(&a.id) { pal.accent } else { pal.line }))
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            let r = ui.horizontal(|ui| {
                                ui.painter().circle_filled(ui.cursor().min + vec2(6.0, 9.0), 5.0, color_of(&a.color));
                                ui.add_space(14.0);
                                ui.label(RichText::new(format!("{} {}", tr("S."), a.page)).small().color(pal.muted));
                                if let Some(au) = &a.author {
                                    ui.label(RichText::new(au).small().color(pal.muted));
                                }
                            });
                            let l = ui.add(egui::Label::new(text).wrap().sense(Sense::click()));
                            if l.clicked() || r.response.interact(Sense::click()).clicked() {
                                select = Some(a.clone());
                            }
                        });
                    ui.add_space(4.0);
                }
                if let Some(a) = select {
                    let page = a.page as usize - 1;
                    self.selected = Some(a.id.clone());
                    if self.tool != Tool::Select {
                        self.tool = Tool::Select;
                    }
                    self.scroll_to_pdf_rect(page, a.bounds());
                }
            }
        });
        if let Some(p) = goto {
            self.go_to(p);
            if narrow {
                self.side = None;
            }
        }
    }

    // ------------------------------------------------------------------ overlays

    fn selection_bar(&mut self, ctx: &egui::Context, pal: &Palette) {
        let Some(id) = self.selected.clone() else { return };
        if self.editing.is_some() || self.draw.is_some() || !self.arbiter.is_idle() {
            return;
        }
        let Some(a) = self.find(&id).cloned() else {
            self.selected = None;
            return;
        };
        let page = a.page as usize - 1;
        if page >= self.pages.len() {
            return;
        }
        let b = self.annot_screen_bounds(&a);
        if !b.intersects(self.canvas) {
            return;
        }
        let pos =
            pos2(b.center().x.clamp(self.canvas.min.x + 120.0, self.canvas.max.x - 120.0), (b.min.y - 10.0).max(self.canvas.min.y + 50.0));
        let mut act = None;
        egui::Area::new(egui::Id::unique("selbar")).fixed_pos(pos).pivot(Align2::CENTER_BOTTOM).order(egui::Order::Foreground).show(
            ctx,
            |ui| {
                egui::Frame::new()
                    .fill(pal.panel)
                    .corner_radius(12)
                    .stroke(Stroke::new(1.0, pal.line))
                    .inner_margin(6)
                    .shadow(egui::Shadow { offset: [0, 4], blur: 16, spread: 0, color: Color32::from_black_alpha(40) })
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            if !matches!(a.kind, Kind::Image { .. } | Kind::Redact { .. }) {
                                let (rect, r) = ui.allocate_exact_size(vec2(28.0, 28.0), Sense::click());
                                ui.painter().circle_filled(rect.center(), 11.0, color_of(&a.color));
                                ui.painter().circle_stroke(rect.center(), 11.0, Stroke::new(1.0, pal.line));
                                if r.clicked() {
                                    self.show_colors = !self.show_colors;
                                }
                            }
                            if ui.button("🗨").on_hover_text(tr("Kommentar")).clicked() {
                                act = Some("comment");
                            }
                            if matches!(a.kind, Kind::Text { .. } | Kind::Note { .. })
                                && ui.button("✏").on_hover_text(tr("Bearbeiten")).clicked()
                            {
                                act = Some("edit");
                            }
                            if a.is_markup() && ui.button("⧉").on_hover_text(tr("Kopieren")).clicked() {
                                act = Some("copy");
                            }
                            if !a.is_markup() && ui.button("⎘").on_hover_text(tr("Duplizieren")).clicked() {
                                act = Some("dup");
                            }
                            if ui.button(RichText::new("🗑").color(pal.danger)).on_hover_text(tr("Löschen")).clicked() {
                                act = Some("del");
                            }
                        });
                        if self.show_colors {
                            ui.horizontal(|ui| {
                                for c in &SWATCHES[..10] {
                                    let (rect, r) = ui.allocate_exact_size(vec2(24.0, 24.0), Sense::click());
                                    ui.painter().circle_filled(rect.center(), 10.0, color_of(c));
                                    if r.clicked() {
                                        let mut b = a.clone();
                                        b.color = c.to_string();
                                        self.update(b);
                                        self.show_colors = false;
                                    }
                                }
                            });
                        }
                    });
            },
        );
        match act {
            Some("comment") => {
                let text = a.note.clone().unwrap_or_default();
                self.dialog = Some(Dialog::Comment { id: a.id.clone(), text });
            }
            Some("edit") => self.start_editing(&a.id),
            Some("copy") => {
                let t = match &a.kind {
                    Kind::Highlight { text, .. } | Kind::Underline { text, .. } | Kind::Strike { text, .. } => {
                        text.clone().unwrap_or_default()
                    }
                    _ => String::new(),
                };
                self.copy_text(ctx, &t);
                self.toast_hint = Some(tr("Text kopiert"));
            }
            Some("dup") => self.duplicate(&a.id),
            Some("del") => self.delete(&a.id),
            _ => {}
        }
    }

    /// In-place editor for text boxes.
    fn text_editor(&mut self, ctx: &egui::Context, pal: &Palette) {
        let Some(id) = self.editing.clone() else { return };
        let Some(a) = self.find(&id).cloned() else {
            self.editing = None;
            return;
        };
        let Kind::Text { rect, font_size, .. } = &a.kind else { return };
        let page = a.page as usize - 1;
        let size = (*font_size as f32 * self.zoom).max(6.0);
        // When editing starts or the view shrinks (soft keyboard opens), scroll the box into view
        if self.edit_view.as_ref() != Some(&(id.clone(), self.canvas)) {
            let s = self.pdf_rect_to_screen(page, *rect);
            let h = s.height().max(size * 1.4 + 10.0);
            self.keep_on_screen(egui::Rect::from_min_size(s.min, egui::vec2(s.width().max(60.0), h)));
            self.edit_view = Some((id.clone(), self.canvas));
            ctx.request_repaint();
        }
        let s = self.pdf_rect_to_screen(page, *rect);
        let mut done = false;
        egui::Area::new(egui::Id::unique(("text-edit", id.clone()))).fixed_pos(s.min).order(egui::Order::Foreground).show(ctx, |ui| {
            egui::Frame::new().fill(Color32::from_white_alpha(230)).stroke(Stroke::new(1.5, pal.accent)).corner_radius(3).show(ui, |ui| {
                let r = ui.add(
                    egui::TextEdit::multiline(&mut self.edit_buf)
                        .font(egui::FontId::proportional(size))
                        .text_color(color_of(&a.color))
                        .desired_width(s.width().max(60.0))
                        .desired_rows(1)
                        .frame(egui::Frame::NONE)
                        .hint_text(tr("Text eingeben …")),
                );
                if !r.has_focus() && !r.lost_focus() {
                    r.request_focus();
                }
                if ui.input(|i| i.key_pressed(Key::Escape) || (i.key_pressed(Key::Enter) && (i.modifiers.ctrl || i.modifiers.command)))
                    || r.lost_focus()
                {
                    done = true;
                }
            });
        });
        if done {
            self.commit_editing();
        }
    }

    fn note_editor(&mut self, ctx: &egui::Context, pal: &Palette) {
        let Some(id) = self.editing.clone() else { return };
        let Some(a) = self.find(&id).cloned() else { return };
        let Kind::Note { at, .. } = &a.kind else { return };
        let page = a.page as usize - 1;
        let p = self.pdf_to_screen(page, *at);
        let pos = pos2(p.x.min(self.canvas.max.x - 270.0).max(self.canvas.min.x + 4.0), (p.y + 26.0).min(self.canvas.max.y - 200.0));
        let mut act = None;
        egui::Area::new(egui::Id::unique(("note", id.clone()))).fixed_pos(pos).order(egui::Order::Foreground).show(ctx, |ui| {
            theme::card(pal).shadow(egui::Shadow { offset: [0, 6], blur: 20, spread: 0, color: Color32::from_black_alpha(50) }).show(
                ui,
                |ui| {
                    ui.set_width(250.0);
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(a.author.clone().unwrap_or_else(|| tr("Notiz"))).strong());
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if ui.button(RichText::new("🗑").color(pal.danger)).clicked() {
                                act = Some(false);
                            }
                        });
                    });
                    let r = ui.add(
                        egui::TextEdit::multiline(&mut self.edit_buf)
                            .hint_text(tr("Notiz schreiben …"))
                            .desired_rows(4)
                            .desired_width(f32::INFINITY),
                    );
                    if !r.has_focus() && self.edit_buf.is_empty() {
                        r.request_focus();
                    }
                    if ui.input(|i| i.key_pressed(Key::Escape)) {
                        act = Some(true);
                    }
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if theme::primary(ui, pal, tr("Fertig")).clicked() {
                            act = Some(true);
                        }
                    });
                },
            );
        });
        match act {
            Some(true) => self.commit_editing(),
            Some(false) => {
                self.editing = None;
                self.delete(&id);
            }
            None => {}
        }
    }

    fn keyboard(&mut self, s: &mut Studio, ctx: &egui::Context, now: f64) {
        if ctx.egui_wants_keyboard_input() {
            return;
        }
        let (cmd, shift) = ctx.input(|i| (i.modifiers.command || i.modifiers.ctrl, i.modifiers.shift));
        let pressed = |k: Key| ctx.input(|i| i.key_pressed(k));
        if cmd && pressed(Key::Z) {
            if shift { self.redo(ctx) } else { self.undo(ctx) }
        } else if cmd && pressed(Key::Y) {
            self.redo(ctx);
        } else if cmd && pressed(Key::S) {
            if shift {
                self.save_as(s, false);
            } else {
                self.save(s, false);
            }
        } else if cmd && pressed(Key::F) {
            self.search.open = true;
            self.search.focus = true;
        } else if cmd && (pressed(Key::Plus) || pressed(Key::Equals)) {
            let c = self.canvas.center();
            self.zoom_at(c, 1.25, now);
        } else if cmd && pressed(Key::Minus) {
            let c = self.canvas.center();
            self.zoom_at(c, 0.8, now);
        } else if cmd && pressed(Key::Num0) {
            self.fit(Fit::Width, now);
        } else if pressed(Key::Delete) || pressed(Key::Backspace) {
            if let Some(id) = self.selected.clone() {
                self.delete(&id);
            }
        } else if pressed(Key::Escape) {
            if self.fullscreen {
                self.fullscreen = false;
                ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(false));
            } else if self.selected.is_some() {
                self.selected = None;
            } else if self.search.open {
                self.search = Search::default();
            } else {
                self.tool = Tool::Hand;
            }
        } else if pressed(Key::PageDown) || (pressed(Key::Space) && !shift) {
            self.offset.y -= self.canvas.height() * 0.9;
            self.clamp_offset();
        } else if pressed(Key::PageUp) || (pressed(Key::Space) && shift) {
            self.offset.y += self.canvas.height() * 0.9;
            self.clamp_offset();
        } else if pressed(Key::ArrowDown) {
            self.offset.y -= 60.0;
            self.clamp_offset();
        } else if pressed(Key::ArrowUp) {
            self.offset.y += 60.0;
            self.clamp_offset();
        } else if pressed(Key::ArrowRight) {
            if self.zoom_relative() <= 1.01 {
                let p = (self.current_page + 1).min(self.pages.len().saturating_sub(1));
                self.go_to(p);
            } else {
                self.offset.x -= 60.0;
                self.clamp_offset();
            }
        } else if pressed(Key::ArrowLeft) {
            if self.zoom_relative() <= 1.01 {
                let p = self.current_page.saturating_sub(1);
                self.go_to(p);
            } else {
                self.offset.x += 60.0;
                self.clamp_offset();
            }
        } else if pressed(Key::Home) {
            self.go_to(0);
        } else if pressed(Key::End) {
            let p = self.pages.len().saturating_sub(1);
            self.go_to(p);
        } else if !cmd {
            let map = [
                (Key::V, Tool::Select),
                (Key::H, Tool::Hand),
                (Key::P, Tool::Ink),
                (Key::T, Tool::Text),
                (Key::N, Tool::Note),
                (Key::E, Tool::Eraser),
                (Key::R, Tool::Rect),
                (Key::L, Tool::Line),
                (Key::A, Tool::Arrow),
                (Key::O, Tool::Ellipse),
            ];
            for (k, t) in map {
                if pressed(k) {
                    self.tab = if matches!(t, Tool::Hand) { Tab::Read } else { Tab::Comment };
                    self.set_tool(t);
                }
            }
        }
    }
}

pub fn today() -> String {
    let days = (now_ms() / 86_400_000.0).floor() as i64;
    let (y, m, d) = lightpdf_core::pdfdoc::civil_from_days(days);
    if crate::i18n::is_english() { format!("{m:02}/{d:02}/{y}") } else { format!("{d:02}.{m:02}.{y}") }
}
