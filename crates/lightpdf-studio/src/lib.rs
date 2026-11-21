//! LightPDF Studio – free, open-source PDF reader and editor, written in pure Rust.
//!
//! One code base for Windows, macOS, Linux, Android and iOS (egui + eframe).

pub mod app;
pub mod browser;
pub mod dialogs;
pub mod i18n;
pub mod icons;
pub mod library;
pub mod organizer;
pub mod platform;
pub mod reader;
pub mod signature;
pub mod storage;
pub mod tasks;
pub mod theme;

pub const APP_NAME: &str = "LightPDF Studio";

/// Text font (egui's default) + Lucide icons (v2's icon set) as fallback for symbols.
pub fn install_fonts(ctx: &eframe::egui::Context) {
    use eframe::egui::{FontData, FontDefinitions, FontFamily};
    let mut fonts = FontDefinitions::default();
    fonts.font_data.insert("lucide".into(), std::sync::Arc::new(FontData::from_static(include_bytes!("../assets/lucide.ttf"))));
    for fam in [FontFamily::Proportional, FontFamily::Monospace] {
        if let Some(list) = fonts.families.get_mut(&fam) {
            list.insert(1.min(list.len()), "lucide".into());
        }
    }
    ctx.set_fonts(fonts);
}

fn icon() -> eframe::egui::IconData {
    let img = image::load_from_memory(include_bytes!("../assets/icon.png")).map(|i| i.to_rgba8());
    match img {
        Ok(i) => eframe::egui::IconData { width: i.width(), height: i.height(), rgba: i.into_raw() },
        Err(_) => eframe::egui::IconData::default(),
    }
}

/// Desktop entry point (Windows, macOS, Linux; also used for iOS builds).
pub fn run_desktop() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title(APP_NAME)
            .with_app_id("org.lightpdf.studio")
            .with_inner_size([1180.0, 820.0])
            .with_min_inner_size([360.0, 480.0])
            .with_icon(icon())
            .with_drag_and_drop(true),
        ..Default::default()
    };
    eframe::run_native(APP_NAME, options, Box::new(|cc| Ok(Box::new(app::Studio::new(cc)))))
}

/// Android entry point (NativeActivity).
#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
fn android_main(app: winit::platform::android::activity::AndroidApp) {
    android_logger::init_once(android_logger::Config::default().with_max_level(log::LevelFilter::Info));
    let _ = platform::imp::APP.set(app.clone());
    let options = eframe::NativeOptions { android_app: Some(app), ..Default::default() };
    if let Err(e) = eframe::run_native(APP_NAME, options, Box::new(|cc| Ok(Box::new(app::Studio::new(cc))))) {
        log::error!("{e}");
    }
}
