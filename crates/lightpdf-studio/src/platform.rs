//! Platform integration: files in and out, URLs, keyboard.
//!
//! * Windows / macOS / Linux: native dialogs (`rfd`), "Open with" via command-line arguments,
//!   drag & drop, saving writes back to the original file.
//! * Android: "Open with" / "Share" intents (content URIs read through the ContentResolver
//!   via JNI), saving writes back to the URI when allowed, exports go to Downloads.
//! * iOS: files arrive in the app's Documents folder (Files app, "Open in…"), the in-app
//!   file browser shows that folder.

use std::path::PathBuf;

pub struct IncomingFile {
    pub name: String,
    pub bytes: Vec<u8>,
    /// Where it came from (path or content URI) – "Save" writes back there.
    pub origin: Option<String>,
}

pub const MOBILE: bool = cfg!(any(target_os = "android", target_os = "ios"));

/// Whether the platform has native open/save dialogs.
pub fn native_dialogs() -> bool {
    cfg!(any(target_os = "windows", target_os = "macos", target_os = "linux"))
}

// ------------------------------------------------------------------ desktop

#[cfg(any(target_os = "windows", target_os = "macos", target_os = "linux"))]
mod imp {
    use super::*;

    pub fn data_dir() -> PathBuf {
        directories::ProjectDirs::from("org", "LightPDF", "LightPDF Studio")
            .map(|d| d.data_dir().to_path_buf())
            .unwrap_or_else(|| std::env::temp_dir().join("lightpdf-studio"))
    }

    fn read(paths: Vec<PathBuf>) -> Vec<IncomingFile> {
        paths
            .into_iter()
            .filter_map(|p| {
                let bytes = std::fs::read(&p).ok()?;
                Some(IncomingFile {
                    name: crate::storage::file_name(&p),
                    bytes,
                    origin: Some(p.canonicalize().unwrap_or(p).to_string_lossy().into_owned()),
                })
            })
            .collect()
    }

    pub fn pick_files(title: &str, ext: &[&str], multi: bool) -> Vec<IncomingFile> {
        let d = rfd::FileDialog::new().set_title(title).add_filter(ext.join(", "), ext);
        let paths = if multi { d.pick_files().unwrap_or_default() } else { d.pick_file().into_iter().collect() };
        read(paths)
    }

    pub fn save_as(name: &str, bytes: &[u8], ext: &str) -> Result<Option<String>, String> {
        let Some(p) = rfd::FileDialog::new().set_file_name(name).add_filter(ext, &[ext]).save_file() else {
            return Ok(None);
        };
        std::fs::write(&p, bytes).map_err(|e| e.to_string())?;
        Ok(Some(p.to_string_lossy().into_owned()))
    }

    pub fn write_back(origin: &str, bytes: &[u8]) -> Result<(), String> {
        let p = PathBuf::from(origin);
        let tmp = p.with_extension("lightpdf-tmp");
        std::fs::write(&tmp, bytes).map_err(|e| e.to_string())?;
        std::fs::rename(&tmp, &p).or_else(|_| std::fs::write(&p, bytes).map(|_| ())).map_err(|e| e.to_string())?;
        let _ = std::fs::remove_file(tmp);
        Ok(())
    }

    pub fn open_url(url: &str) {
        let _ = open::that_detached(url);
    }

    /// Opens a file with the system's default app (used for printing).
    pub fn open_externally(name: &str, bytes: &[u8]) -> Result<(), String> {
        let p = std::env::temp_dir().join(name);
        std::fs::write(&p, bytes).map_err(|e| e.to_string())?;
        open::that_detached(&p).map_err(|e| e.to_string())
    }

    pub fn incoming() -> Vec<IncomingFile> {
        use std::sync::atomic::{AtomicBool, Ordering};
        static DONE: AtomicBool = AtomicBool::new(false);
        if DONE.swap(true, Ordering::SeqCst) {
            return vec![];
        }
        read(std::env::args().skip(1).filter(|a| !a.starts_with('-')).map(PathBuf::from).collect())
    }

    pub fn browse_roots() -> Vec<(String, PathBuf)> {
        let mut v = Vec::new();
        if let Some(u) = directories::UserDirs::new() {
            if let Some(d) = u.document_dir() {
                v.push((crate::i18n::tr("Dokumente"), d.to_path_buf()));
            }
            if let Some(d) = u.download_dir() {
                v.push(("Downloads".into(), d.to_path_buf()));
            }
            v.push(("Home".into(), u.home_dir().to_path_buf()));
        }
        v
    }

    pub fn show_keyboard(_show: bool) {}
}

// ------------------------------------------------------------------ Android

#[cfg(target_os = "android")]
pub mod imp {
    use super::*;
    use jni::objects::{JByteArray, JObject, JValue};
    use jni::{JNIEnv, JavaVM};
    use std::sync::OnceLock;
    use winit::platform::android::activity::AndroidApp;

    pub static APP: OnceLock<AndroidApp> = OnceLock::new();

    pub fn data_dir() -> PathBuf {
        APP.get().and_then(|a| a.internal_data_path()).unwrap_or_else(|| PathBuf::from("/data/local/tmp/lightpdf"))
    }

    fn with_env<R>(f: impl FnOnce(&mut JNIEnv, &JObject) -> jni::errors::Result<R>) -> Result<R, String> {
        let app = APP.get().ok_or("no activity")?;
        let vm = unsafe { JavaVM::from_raw(app.vm_as_ptr() as *mut _) }.map_err(|e| e.to_string())?;
        let mut env = vm.attach_current_thread().map_err(|e| e.to_string())?;
        let activity = unsafe { JObject::from_raw(app.activity_as_ptr() as jni::sys::jobject) };
        let r = f(&mut env, &activity);
        if env.exception_check().unwrap_or(false) {
            let _ = env.exception_describe();
            let _ = env.exception_clear();
        }
        r.map_err(|e| e.to_string())
    }

    fn jstring(env: &mut JNIEnv, o: JObject) -> jni::errors::Result<Option<String>> {
        if o.is_null() {
            return Ok(None);
        }
        Ok(Some(env.get_string(&o.into())?.into()))
    }

    fn content_resolver<'a>(env: &mut JNIEnv<'a>, activity: &JObject) -> jni::errors::Result<JObject<'a>> {
        env.call_method(activity, "getContentResolver", "()Landroid/content/ContentResolver;", &[])?.l()
    }

    fn parse_uri<'a>(env: &mut JNIEnv<'a>, s: &str) -> jni::errors::Result<JObject<'a>> {
        let js = env.new_string(s)?;
        env.call_static_method("android/net/Uri", "parse", "(Ljava/lang/String;)Landroid/net/Uri;", &[JValue::Object(&js)])?.l()
    }

    fn read_uri(env: &mut JNIEnv, activity: &JObject, uri: &JObject) -> jni::errors::Result<Vec<u8>> {
        let cr = content_resolver(env, activity)?;
        let is = env.call_method(&cr, "openInputStream", "(Landroid/net/Uri;)Ljava/io/InputStream;", &[JValue::Object(uri)])?.l()?;
        let buf: JByteArray = env.new_byte_array(1 << 16)?;
        let mut out = Vec::new();
        let mut chunk = vec![0i8; 1 << 16];
        loop {
            let n = env.call_method(&is, "read", "([B)I", &[JValue::Object(&buf)])?.i()?;
            if n < 0 {
                break;
            }
            env.get_byte_array_region(&buf, 0, &mut chunk[..n as usize])?;
            out.extend(chunk[..n as usize].iter().map(|&b| b as u8));
        }
        env.call_method(&is, "close", "()V", &[])?;
        Ok(out)
    }

    fn write_uri(env: &mut JNIEnv, activity: &JObject, uri: &JObject, bytes: &[u8]) -> jni::errors::Result<()> {
        let cr = content_resolver(env, activity)?;
        let mode = env.new_string("wt")?;
        let os = env
            .call_method(
                &cr,
                "openOutputStream",
                "(Landroid/net/Uri;Ljava/lang/String;)Ljava/io/OutputStream;",
                &[JValue::Object(uri), JValue::Object(&mode)],
            )?
            .l()?;
        let arr = env.byte_array_from_slice(bytes)?;
        env.call_method(&os, "write", "([B)V", &[JValue::Object(&arr)])?;
        env.call_method(&os, "close", "()V", &[])?;
        Ok(())
    }

    fn display_name(env: &mut JNIEnv, activity: &JObject, uri: &JObject) -> Option<String> {
        (|| -> jni::errors::Result<Option<String>> {
            let cr = content_resolver(env, activity)?;
            let null = JObject::null();
            let cursor = env
                .call_method(
                    &cr,
                    "query",
                    "(Landroid/net/Uri;[Ljava/lang/String;Ljava/lang/String;[Ljava/lang/String;Ljava/lang/String;)Landroid/database/Cursor;",
                    &[JValue::Object(uri), JValue::Object(&null), JValue::Object(&null), JValue::Object(&null), JValue::Object(&null)],
                )?
                .l()?;
            if cursor.is_null() {
                return Ok(None);
            }
            let mut name = None;
            if env.call_method(&cursor, "moveToFirst", "()Z", &[])?.z()? {
                let col = env.new_string("_display_name")?;
                let idx = env.call_method(&cursor, "getColumnIndex", "(Ljava/lang/String;)I", &[JValue::Object(&col)])?.i()?;
                if idx >= 0 {
                    let s = env.call_method(&cursor, "getString", "(I)Ljava/lang/String;", &[JValue::Int(idx)])?.l()?;
                    name = jstring(env, s)?;
                }
            }
            env.call_method(&cursor, "close", "()V", &[])?;
            Ok(name)
        })()
        .ok()
        .flatten()
    }

    /// The file the app was started with ("Open with" / "Share"), consumed once.
    pub fn incoming() -> Vec<IncomingFile> {
        with_env(|env, activity| {
            let intent = env.call_method(activity, "getIntent", "()Landroid/content/Intent;", &[])?.l()?;
            if intent.is_null() {
                return Ok(vec![]);
            }
            let action = env.call_method(&intent, "getAction", "()Ljava/lang/String;", &[])?.l()?;
            let action = jstring(env, action)?.unwrap_or_default();
            let uri = match action.as_str() {
                "android.intent.action.VIEW" | "android.intent.action.EDIT" => {
                    env.call_method(&intent, "getData", "()Landroid/net/Uri;", &[])?.l()?
                }
                "android.intent.action.SEND" => {
                    let key = env.new_string("android.intent.extra.STREAM")?;
                    env.call_method(&intent, "getParcelableExtra", "(Ljava/lang/String;)Landroid/os/Parcelable;", &[JValue::Object(&key)])?
                        .l()?
                }
                _ => JObject::null(),
            };
            if uri.is_null() {
                return Ok(vec![]);
            }
            let s = env.call_method(&uri, "toString", "()Ljava/lang/String;", &[])?.l()?;
            let origin = jstring(env, s)?;
            let bytes = read_uri(env, activity, &uri)?;
            let name = display_name(env, activity, &uri).unwrap_or_else(|| "Dokument.pdf".into());
            // consume the intent so it isn't imported again
            let empty = env.new_object("android/content/Intent", "()V", &[])?;
            env.call_method(activity, "setIntent", "(Landroid/content/Intent;)V", &[JValue::Object(&empty)])?;
            // Only write back to the source for real edit permissions (VIEW of a file manager usually grants it)
            let origin = if action == "android.intent.action.SEND" { None } else { origin };
            Ok(vec![IncomingFile { name, bytes, origin }])
        })
        .unwrap_or_default()
    }

    pub fn write_back(origin: &str, bytes: &[u8]) -> Result<(), String> {
        if origin.starts_with('/') {
            return std::fs::write(origin, bytes).map_err(|e| e.to_string());
        }
        with_env(|env, activity| {
            let uri = parse_uri(env, origin)?;
            write_uri(env, activity, &uri, bytes)
        })
    }

    /// Saves into Downloads/LightPDF (MediaStore, Android 10+).
    pub fn save_as(name: &str, bytes: &[u8], ext: &str) -> Result<Option<String>, String> {
        let mime = match ext {
            "pdf" => "application/pdf",
            "zip" => "application/zip",
            "txt" => "text/plain",
            "png" => "image/png",
            _ => "application/octet-stream",
        };
        with_env(|env, activity| {
            let values = env.new_object("android/content/ContentValues", "()V", &[])?;
            for (k, v) in [("_display_name", name), ("mime_type", mime), ("relative_path", "Download/LightPDF")] {
                let (k, v) = (env.new_string(k)?, env.new_string(v)?);
                env.call_method(&values, "put", "(Ljava/lang/String;Ljava/lang/String;)V", &[JValue::Object(&k), JValue::Object(&v)])?;
            }
            let base = env.get_static_field("android/provider/MediaStore$Downloads", "EXTERNAL_CONTENT_URI", "Landroid/net/Uri;")?.l()?;
            let cr = content_resolver(env, activity)?;
            let uri = env
                .call_method(
                    &cr,
                    "insert",
                    "(Landroid/net/Uri;Landroid/content/ContentValues;)Landroid/net/Uri;",
                    &[JValue::Object(&base), JValue::Object(&values)],
                )?
                .l()?;
            if uri.is_null() {
                return Ok(None);
            }
            write_uri(env, activity, &uri, bytes)?;
            let s = env.call_method(&uri, "toString", "()Ljava/lang/String;", &[])?.l()?;
            jstring(env, s)
        })
    }

    pub fn pick_files(_title: &str, _ext: &[&str], _multi: bool) -> Vec<IncomingFile> {
        vec![] // in-app browser is used instead
    }

    pub fn open_url(url: &str) {
        let _ = with_env(|env, activity| {
            let uri = parse_uri(env, url)?;
            let action = env.new_string("android.intent.action.VIEW")?;
            let intent = env.new_object(
                "android/content/Intent",
                "(Ljava/lang/String;Landroid/net/Uri;)V",
                &[JValue::Object(&action), JValue::Object(&uri)],
            )?;
            env.call_method(activity, "startActivity", "(Landroid/content/Intent;)V", &[JValue::Object(&intent)])?;
            Ok(())
        });
    }

    pub fn open_externally(name: &str, bytes: &[u8]) -> Result<(), String> {
        save_as(name, bytes, "pdf").map(|_| ())
    }

    pub fn browse_roots() -> Vec<(String, PathBuf)> {
        vec![
            ("Downloads".into(), PathBuf::from("/storage/emulated/0/Download")),
            (crate::i18n::tr("Dokumente"), PathBuf::from("/storage/emulated/0/Documents")),
            ("Speicher".into(), PathBuf::from("/storage/emulated/0")),
        ]
    }

    pub fn show_keyboard(show: bool) {
        if let Some(a) = APP.get() {
            if show { a.show_soft_input(true) } else { a.hide_soft_input(true) }
        }
    }
}

// ------------------------------------------------------------------ iOS (and other targets)

#[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux", target_os = "android")))]
mod imp {
    use super::*;

    fn documents() -> PathBuf {
        std::env::var_os("HOME").map(|h| PathBuf::from(h).join("Documents")).unwrap_or_else(std::env::temp_dir)
    }

    pub fn data_dir() -> PathBuf {
        std::env::var_os("HOME")
            .map(|h| PathBuf::from(h).join("Library/Application Support/LightPDF"))
            .unwrap_or_else(|| std::env::temp_dir().join("lightpdf"))
    }

    /// PDFs handed over by other apps land in Documents/Inbox.
    pub fn incoming() -> Vec<IncomingFile> {
        let inbox = documents().join("Inbox");
        let Ok(rd) = std::fs::read_dir(&inbox) else { return vec![] };
        let mut out = Vec::new();
        for e in rd.flatten() {
            let p = e.path();
            if p.extension().is_some_and(|x| x.eq_ignore_ascii_case("pdf")) {
                if let Ok(bytes) = std::fs::read(&p) {
                    out.push(IncomingFile { name: crate::storage::file_name(&p), bytes, origin: None });
                    let _ = std::fs::remove_file(&p);
                }
            }
        }
        out
    }

    pub fn pick_files(_title: &str, _ext: &[&str], _multi: bool) -> Vec<IncomingFile> {
        vec![]
    }

    pub fn save_as(name: &str, bytes: &[u8], _ext: &str) -> Result<Option<String>, String> {
        let p = documents().join(name);
        std::fs::write(&p, bytes).map_err(|e| e.to_string())?;
        Ok(Some(p.to_string_lossy().into_owned()))
    }

    pub fn write_back(origin: &str, bytes: &[u8]) -> Result<(), String> {
        std::fs::write(origin, bytes).map_err(|e| e.to_string())
    }

    pub fn open_url(_url: &str) {}

    pub fn open_externally(name: &str, bytes: &[u8]) -> Result<(), String> {
        save_as(name, bytes, "pdf").map(|_| ())
    }

    pub fn browse_roots() -> Vec<(String, PathBuf)> {
        vec![(crate::i18n::tr("Dokumente"), documents())]
    }

    pub fn show_keyboard(_show: bool) {}
}

pub use imp::*;
