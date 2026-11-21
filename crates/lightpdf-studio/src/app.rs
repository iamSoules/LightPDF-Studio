//! The application shell: screens, toasts, background tasks, incoming files.

use crate::browser::FileBrowser;
use crate::i18n::{tr, trf};
use crate::library::Library;
use crate::platform::{self, IncomingFile};
use crate::reader::Reader;
use crate::storage::{DocMeta, Storage, ThemeMode};
use crate::tasks::Tasks;
use crate::theme::{self, Palette};
use eframe::egui::{self, Color32, RichText};
use lightpdf_core::ops;
use lightpdf_core::pdfdoc::Error as PdfError;

pub enum Screen {
    Library,
    Reader(Box<Reader>),
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ToastKind {
    Ok,
    Err,
}

pub struct Toast {
    pub text: String,
    pub kind: ToastKind,
    pub until: f64,
}

/// A document waiting for its password.
pub struct PasswordPrompt {
    pub meta: DocMeta,
    pub bytes: Vec<u8>,
    pub input: String,
    pub wrong: bool,
}

/// What the in-app file browser was opened for.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum BrowsePurpose {
    Open,
    Merge,
    InsertPdf,
    Images,
    ImagesToPdf,
}

pub struct Studio {
    pub storage: Storage,
    pub pal: Palette,
    pub touch: bool,
    pub screen: Screen,
    pub library: Library,
    pub toasts: Vec<Toast>,
    pub tasks: Tasks,
    pub browser: Option<(FileBrowser, BrowsePurpose)>,
    pub password: Option<PasswordPrompt>,
    pub ctx: egui::Context,
    applied_theme: Option<(bool, bool)>,
    last_focus: bool,
    pub show_settings: bool,
    pub show_about: bool,
}

impl Studio {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        Self::with_dir(cc, platform::data_dir(), true)
    }

    /// `incoming`: import files handed over by the OS (command line, intents).
    pub fn with_dir(cc: &eframe::CreationContext<'_>, dir: std::path::PathBuf, incoming: bool) -> Self {
        let storage = Storage::open(dir);
        crate::i18n::set_english(storage.lib.settings.english);
        crate::install_fonts(&cc.egui_ctx);
        let mut s = Self {
            storage,
            pal: theme::LIGHT,
            touch: platform::MOBILE,
            screen: Screen::Library,
            library: Library::default(),
            toasts: Vec::new(),
            tasks: Tasks::default(),
            browser: None,
            password: None,
            ctx: cc.egui_ctx.clone(),
            applied_theme: None,
            last_focus: true,
            show_settings: false,
            show_about: false,
        };
        let scale = s.storage.lib.settings.ui_scale;
        if (scale - 1.0).abs() > 0.01 {
            cc.egui_ctx.set_zoom_factor(scale);
        }
        if incoming {
            s.take_incoming();
        }
        s
    }

    // ------------------------------------------------------------------ toasts
    pub fn toast(&mut self, text: impl Into<String>) {
        let now = self.ctx.input(|i| i.time);
        self.toasts.push(Toast { text: text.into(), kind: ToastKind::Ok, until: now + 2.6 });
    }
    pub fn toast_err(&mut self, text: impl Into<String>) {
        let now = self.ctx.input(|i| i.time);
        self.toasts.push(Toast { text: text.into(), kind: ToastKind::Err, until: now + 4.5 });
    }

    // ------------------------------------------------------------------ opening documents
    fn take_incoming(&mut self) {
        let files = platform::incoming();
        self.import_files(files, true);
    }

    /// Adds files to the library; opens the first one if `open`.
    pub fn import_files(&mut self, files: Vec<IncomingFile>, open: bool) {
        let mut first = None;
        for f in files {
            if !f.bytes.starts_with(b"%PDF") && !f.bytes.windows(5).take(1024).any(|w| w == b"%PDF-") {
                self.toast_err(trf("Konnte die Datei nicht öffnen: {0}", &[&f.name]));
                continue;
            }
            match self.storage.add(&f.name, &f.bytes, f.origin) {
                Ok(m) => {
                    first.get_or_insert(m);
                }
                Err(e) => self.toast_err(trf("Konnte die Datei nicht öffnen: {0}", &[&e])),
            }
        }
        if let Some(m) = first.filter(|_| open) {
            self.open_doc(m);
        }
    }

    pub fn open_doc_id(&mut self, id: &str) {
        if let Some(m) = self.storage.get(id) {
            self.open_doc(m);
        }
    }

    pub fn open_doc(&mut self, meta: DocMeta) {
        let bytes = match self.storage.read_doc(&meta.id) {
            Ok(b) => b,
            Err(_) => {
                self.toast_err(tr("Datei nicht gefunden"));
                return;
            }
        };
        if lightpdf_core::pdfdoc::needs_password(&bytes) {
            self.password = Some(PasswordPrompt { meta, bytes, input: String::new(), wrong: false });
            return;
        }
        self.start_reader(meta, bytes, None);
    }

    pub fn start_reader(&mut self, meta: DocMeta, bytes: Vec<u8>, password: Option<String>) {
        if let Screen::Reader(r) = &mut self.screen {
            r.persist_position(&mut self.storage);
        }
        let ctx = self.ctx.clone();
        self.tasks.spawn(&ctx, tr("Öffne …"), move |_| {
            let r = ops::import_pdf(&bytes, password.as_deref());
            Box::new(move |s: &mut Studio| match r {
                Ok(imp) => match Reader::new(&s.ctx, meta.clone(), imp, password, &s.storage) {
                    Ok(reader) => {
                        let mut m = meta;
                        m.opened_at = lightpdf_core::model::now_ms();
                        m.pages = reader.page_count();
                        s.storage.update(&m);
                        s.screen = Screen::Reader(Box::new(reader));
                    }
                    Err(e) => s.toast_err(trf("Konnte die Datei nicht öffnen: {0}", &[&e])),
                },
                Err(PdfError::PasswordNeeded { wrong }) => {
                    s.password = Some(PasswordPrompt { meta, bytes, input: String::new(), wrong });
                }
                Err(e) => s.toast_err(trf("Konnte die Datei nicht öffnen: {0}", &[&e])),
            })
        });
    }

    pub fn close_reader(&mut self) {
        if let Screen::Reader(r) = &mut self.screen {
            r.persist_position(&mut self.storage);
        }
        self.screen = Screen::Library;
    }

    /// Native dialog on desktop, in-app browser on mobile.
    pub fn pick(&mut self, purpose: BrowsePurpose) -> Option<Vec<IncomingFile>> {
        let (ext, multi): (&[&str], bool) = match purpose {
            BrowsePurpose::Images | BrowsePurpose::ImagesToPdf => (&["png", "jpg", "jpeg", "webp", "gif", "bmp"], true),
            BrowsePurpose::Merge => (&["pdf"], true),
            BrowsePurpose::Open => (&["pdf"], true),
            BrowsePurpose::InsertPdf => (&["pdf"], false),
        };
        if platform::native_dialogs() {
            let files = platform::pick_files(&tr("Datei öffnen"), ext, multi);
            return Some(files);
        }
        self.browser = Some((FileBrowser::new(ext, multi), purpose));
        None
    }

    pub fn handle_picked(&mut self, purpose: BrowsePurpose, files: Vec<IncomingFile>) {
        if files.is_empty() {
            return;
        }
        match purpose {
            BrowsePurpose::Open => self.import_files(files, true),
            BrowsePurpose::Merge | BrowsePurpose::ImagesToPdf => crate::dialogs::create_from_files(self, purpose, files),
            BrowsePurpose::InsertPdf | BrowsePurpose::Images => {
                if let Screen::Reader(r) = &mut self.screen {
                    r.files_picked(purpose, files);
                }
            }
        }
    }

    // ------------------------------------------------------------------ settings
    pub fn dark(&self, ctx: &egui::Context) -> bool {
        match self.storage.lib.settings.theme {
            ThemeMode::Light => false,
            ThemeMode::Dark => true,
            ThemeMode::System => ctx.system_theme().map(|t| t == egui::Theme::Dark).unwrap_or(false),
        }
    }

    fn apply_theme(&mut self, ctx: &egui::Context) {
        let dark = self.dark(ctx);
        if self.applied_theme != Some((dark, self.touch)) {
            self.pal = if dark { theme::DARK } else { theme::LIGHT };
            theme::apply(ctx, &self.pal, self.touch);
            self.applied_theme = Some((dark, self.touch));
        }
    }

    fn settings_window(&mut self, ctx: &egui::Context) {
        let mut open = self.show_settings;
        let pal = self.pal;
        egui::Window::new(tr("Einstellungen"))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .default_width(360.0)
            .show(ctx, |ui| {
                let s = &mut self.storage.lib.settings;
                let before = s.clone();
                egui::Grid::new("settings").num_columns(2).spacing([16.0, 12.0]).show(ui, |ui| {
                    ui.label(tr("Sprache"));
                    ui.horizontal(|ui| {
                        ui.selectable_value(&mut s.english, false, "Deutsch");
                        ui.selectable_value(&mut s.english, true, "English");
                    });
                    ui.end_row();
                    ui.label(tr("Design"));
                    ui.horizontal(|ui| {
                        ui.selectable_value(&mut s.theme, ThemeMode::Light, tr("Hell"));
                        ui.selectable_value(&mut s.theme, ThemeMode::Dark, tr("Dunkel"));
                        ui.selectable_value(&mut s.theme, ThemeMode::System, tr("System"));
                    });
                    ui.end_row();
                    ui.label(tr("Autor"));
                    ui.add(egui::TextEdit::singleline(&mut s.author).hint_text(tr("Dein Name für Kommentare")).desired_width(200.0));
                    ui.end_row();
                    ui.label(tr("Stiftmodus"));
                    ui.checkbox(&mut s.pen_only, tr("Nur Stift zeichnet (Finger blättern)"));
                    ui.end_row();
                    ui.label(tr("Zoom"));
                    ui.add(egui::Slider::new(&mut s.ui_scale, 0.75..=1.75).step_by(0.05));
                    ui.end_row();
                });
                ui.add_space(6.0);
                ui.label(RichText::new(tr("Mit zwei Fingern zoomen – es entsteht keine Markierung.")).small().color(pal.muted));
                if *s != before {
                    crate::i18n::set_english(s.english);
                    if (s.ui_scale - before.ui_scale).abs() > 0.001 {
                        ctx.set_zoom_factor(s.ui_scale);
                    }
                    let pen_only = s.pen_only;
                    self.storage.save();
                    self.applied_theme = None;
                    if let Screen::Reader(r) = &mut self.screen {
                        r.set_pen_only(pen_only);
                    }
                }
            });
        self.show_settings = open;
    }

    fn about_window(&mut self, ctx: &egui::Context) {
        let mut open = self.show_about;
        egui::Window::new(crate::APP_NAME)
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.label(RichText::new(format!("{} {}", crate::APP_NAME, env!("CARGO_PKG_VERSION"))).heading());
                ui.label(tr("Freier, quelloffener PDF-Reader und -Editor. Komplett in Rust."));
                ui.add_space(6.0);
                ui.label(RichText::new("MIT · egui · hayro · lopdf").small().color(self.pal.muted));
            });
        self.show_about = open;
    }

    fn password_window(&mut self, ctx: &egui::Context) {
        let Some(p) = &mut self.password else { return };
        let pal = self.pal;
        let mut action = None;
        egui::Modal::new(egui::Id::unique("pw")).show(ctx, |ui| {
            ui.set_width(320.0_f32.min(ctx.content_rect().width() - 48.0));
            ui.heading(tr("Kennwort erforderlich"));
            ui.label(RichText::new(&p.meta.name).color(pal.muted));
            ui.add_space(8.0);
            let r = ui.add(egui::TextEdit::singleline(&mut p.input).password(true).hint_text(tr("Kennwort")).desired_width(f32::INFINITY));
            if !r.has_focus() && p.input.is_empty() {
                r.request_focus();
            }
            if r.has_focus() {
                platform::show_keyboard(true);
            }
            if p.wrong {
                ui.colored_label(pal.danger, tr("Falsches Kennwort"));
            }
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if theme::primary(ui, &pal, tr("Entsperren")).clicked() || ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    action = Some(true);
                }
                if ui.button(tr("Abbrechen")).clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                    action = Some(false);
                }
            });
        });
        match action {
            Some(true) => {
                let p = self.password.take().unwrap();
                platform::show_keyboard(false);
                if lightpdf_core::pdfdoc::load(&p.bytes, Some(&p.input)).is_ok() {
                    self.start_reader(p.meta, p.bytes, Some(p.input));
                } else {
                    self.password = Some(PasswordPrompt { wrong: true, input: String::new(), ..p });
                }
            }
            Some(false) => {
                self.password = None;
                platform::show_keyboard(false);
            }
            None => {}
        }
    }

    fn toasts_ui(&mut self, ctx: &egui::Context) {
        let now = ctx.input(|i| i.time);
        self.toasts.retain(|t| t.until > now);
        let busy = self.tasks.busy().map(|t| (t.label.clone(), t.progress));
        if self.toasts.is_empty() && busy.is_none() {
            return;
        }
        let pal = self.pal;
        egui::Area::new(egui::Id::unique("toasts"))
            .anchor(egui::Align2::CENTER_BOTTOM, [0.0, -28.0])
            .order(egui::Order::Tooltip)
            .interactable(false)
            .show(ctx, |ui| {
                ui.vertical_centered(|ui| {
                    let frame = |c: Color32| {
                        egui::Frame::new().fill(c).corner_radius(12).inner_margin(egui::Margin::symmetric(16, 10)).shadow(egui::Shadow {
                            offset: [0, 4],
                            blur: 18,
                            spread: 0,
                            color: Color32::from_black_alpha(50),
                        })
                    };
                    if let Some((label, progress)) = &busy {
                        frame(if pal.dark { Color32::from_gray(235) } else { Color32::from_rgb(44, 37, 32) }).show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.add(egui::Spinner::new().color(if pal.dark { Color32::BLACK } else { Color32::WHITE }));
                                let mut t = label.clone();
                                if let Some(p) = progress {
                                    t += &format!("  {:.0} %", p * 100.0);
                                }
                                ui.label(RichText::new(t).color(if pal.dark { Color32::BLACK } else { Color32::WHITE }));
                            });
                        });
                        ui.add_space(6.0);
                    }
                    for t in &self.toasts {
                        let bg = match t.kind {
                            ToastKind::Ok => {
                                if pal.dark {
                                    Color32::from_gray(235)
                                } else {
                                    Color32::from_rgb(44, 37, 32)
                                }
                            }
                            ToastKind::Err => pal.danger,
                        };
                        let fg = if pal.dark && t.kind == ToastKind::Ok { Color32::BLACK } else { Color32::WHITE };
                        frame(bg).show(ui, |ui| {
                            ui.set_max_width(420.0);
                            ui.label(RichText::new(&t.text).color(fg));
                        });
                        ui.add_space(6.0);
                    }
                });
            });
        ctx.request_repaint_after(std::time::Duration::from_millis(250));
    }

    fn persist(&mut self) {
        if let Screen::Reader(r) = &mut self.screen {
            r.persist_position(&mut self.storage);
        }
        self.storage.save();
    }

    fn handle_drops(&mut self, ctx: &egui::Context) {
        let dropped = ctx.input(|i| i.raw.dropped_files.clone());
        if dropped.is_empty() {
            return;
        }
        let files: Vec<IncomingFile> = dropped
            .into_iter()
            .filter_map(|f| {
                let path = f.path().to_path_buf();
                let bytes = f.bytes().ok().or_else(|| std::fs::read(&path).ok())?;
                let origin = (!path.as_os_str().is_empty()).then(|| path.to_string_lossy().into_owned());
                Some(IncomingFile { name: crate::storage::file_name(&path), bytes, origin })
            })
            .collect();
        let (images, pdfs): (Vec<_>, Vec<_>) = files.into_iter().partition(|f| !f.bytes.starts_with(b"%PDF"));
        if !images.is_empty() {
            if let Screen::Reader(r) = &mut self.screen {
                r.files_picked(BrowsePurpose::Images, images);
            } else {
                crate::dialogs::create_from_files(self, BrowsePurpose::ImagesToPdf, images);
            }
        }
        if !pdfs.is_empty() {
            self.import_files(pdfs, true);
        }
    }
}

impl eframe::App for Studio {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        // Touch-sized UI as soon as a finger is used
        if !self.touch && ctx.input(|i| i.events.iter().any(|e| matches!(e, egui::Event::Touch { .. }))) {
            self.touch = true;
        }
        self.apply_theme(&ctx);

        // Files handed over while running (Android: new intent, iOS: Inbox) when we regain focus
        let focused = ctx.input(|i| i.focused);
        if focused && !self.last_focus {
            self.take_incoming();
        }
        self.last_focus = focused;

        for apply in self.tasks.poll() {
            apply(self);
        }
        self.handle_drops(&ctx);

        // Android back button / Escape
        if ctx.input(|i| i.key_pressed(egui::Key::BrowserBack)) {
            if self.browser.is_some() {
                self.browser = None;
            } else if let Screen::Reader(r) = &mut self.screen {
                r.request_close();
            }
        }

        match &mut self.screen {
            Screen::Library => crate::library::show(self, ui),
            Screen::Reader(_) => crate::reader::show(self, ui),
        }

        if let Some((b, purpose)) = &mut self.browser {
            let purpose = *purpose;
            match b.show(&ctx, &self.pal) {
                Some(Some(files)) => {
                    self.browser = None;
                    self.handle_picked(purpose, files);
                }
                Some(None) => self.browser = None,
                None => {}
            }
        }
        self.password_window(&ctx);
        if self.show_settings {
            self.settings_window(&ctx);
        }
        if self.show_about {
            self.about_window(&ctx);
        }
        self.toasts_ui(&ctx);

        // Soft keyboard follows text focus (Android)
        if platform::MOBILE {
            platform::show_keyboard(ctx.egui_wants_keyboard_input());
        }
    }

    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        self.pal.bg.to_normalized_gamma_f32()
    }

    #[cfg(not(target_os = "ios"))]
    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.persist();
    }

    #[cfg(target_os = "ios")]
    fn on_exit(&mut self) {
        self.persist();
    }
}
