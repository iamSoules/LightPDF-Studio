//! Library on disk: `library.json` (documents, folders, settings, signatures) and the files.
//!
//! Layout inside the data directory:
//! ```text
//! library.json
//! docs/<id>.pdf        working copy of every document
//! thumbs/<id>.png      cover thumbnail
//! signatures/<id>.png  saved signatures / initials
//! ```

use lightpdf_core::model::{now_ms, uid};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ThemeMode {
    Light,
    Dark,
    System,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Reading {
    Normal,
    Night,
    Sepia,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ScrollMode {
    Vertical,
    Horizontal,
    Page,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SortBy {
    Recent,
    Added,
    Name,
    Size,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub english: bool,
    pub theme: ThemeMode,
    pub author: String,
    pub reading: Reading,
    pub scroll_mode: ScrollMode,
    pub spread: bool,
    pub sort: SortBy,
    pub grid: bool,
    /// Only pen/mouse draws; fingers always scroll and zoom.
    pub pen_only: bool,
    pub ui_scale: f32,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            english: !crate::i18n::system_is_german(),
            theme: ThemeMode::System,
            author: String::new(),
            reading: Reading::Normal,
            scroll_mode: ScrollMode::Vertical,
            spread: false,
            sort: SortBy::Recent,
            grid: true,
            pen_only: false,
            ui_scale: 1.0,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DocMeta {
    pub id: String,
    pub name: String,
    pub size: u64,
    pub pages: usize,
    pub added_at: f64,
    pub opened_at: f64,
    pub last_page: usize,
    pub folder_id: Option<String>,
    pub starred: bool,
    pub encrypted: bool,
    /// Original location (desktop path or Android content URI): "Save" writes back there.
    pub path: Option<String>,
}

impl Default for DocMeta {
    fn default() -> Self {
        Self {
            id: uid(),
            name: String::new(),
            size: 0,
            pages: 0,
            added_at: now_ms(),
            opened_at: 0.0,
            last_page: 0,
            folder_id: None,
            starred: false,
            encrypted: false,
            path: None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Folder {
    pub id: String,
    pub name: String,
    pub color: String,
    pub created_at: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SavedSignature {
    pub id: String,
    pub initials: bool,
    pub created_at: f64,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct LibraryFile {
    pub settings: Settings,
    pub docs: Vec<DocMeta>,
    pub folders: Vec<Folder>,
    pub signatures: Vec<SavedSignature>,
    pub stamps: Vec<String>,
}

pub struct Storage {
    pub dir: PathBuf,
    pub lib: LibraryFile,
}

impl Storage {
    pub fn open(dir: PathBuf) -> Self {
        for sub in ["docs", "thumbs", "signatures"] {
            let _ = std::fs::create_dir_all(dir.join(sub));
        }
        let lib = std::fs::read(dir.join("library.json")).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
        Self { dir, lib }
    }

    pub fn save(&self) {
        if let Ok(b) = serde_json::to_vec_pretty(&self.lib) {
            let tmp = self.dir.join("library.json.tmp");
            if std::fs::write(&tmp, b).is_ok() {
                let _ = std::fs::rename(tmp, self.dir.join("library.json"));
            }
        }
    }

    pub fn doc_path(&self, id: &str) -> PathBuf {
        self.dir.join("docs").join(format!("{id}.pdf"))
    }
    pub fn thumb_path(&self, id: &str) -> PathBuf {
        self.dir.join("thumbs").join(format!("{id}.png"))
    }
    pub fn signature_path(&self, id: &str) -> PathBuf {
        self.dir.join("signatures").join(format!("{id}.png"))
    }

    pub fn read_doc(&self, id: &str) -> std::io::Result<Vec<u8>> {
        std::fs::read(self.doc_path(id))
    }

    pub fn write_doc(&mut self, id: &str, bytes: &[u8]) -> std::io::Result<()> {
        let p = self.doc_path(id);
        let tmp = p.with_extension("tmp");
        std::fs::write(&tmp, bytes)?;
        std::fs::rename(tmp, &p)?;
        if let Some(m) = self.lib.docs.iter_mut().find(|d| d.id == id) {
            m.size = bytes.len() as u64;
        }
        let _ = std::fs::remove_file(self.thumb_path(id)); // regenerate cover
        Ok(())
    }

    /// Adds a document (a file that was opened or created). Re-opening the same path
    /// refreshes the existing entry instead of creating a duplicate.
    pub fn add(&mut self, name: &str, bytes: &[u8], path: Option<String>) -> std::io::Result<DocMeta> {
        if let Some(p) = &path
            && let Some(i) = self.lib.docs.iter().position(|d| d.path.as_deref() == Some(p))
        {
            let id = self.lib.docs[i].id.clone();
            let same = self.read_doc(&id).map(|b| b == bytes).unwrap_or(false);
            if !same {
                self.write_doc(&id, bytes)?;
            }
            let m = &mut self.lib.docs[i];
            m.pages = page_count(bytes).unwrap_or(m.pages);
            m.encrypted = lightpdf_core::pdfdoc::needs_password(bytes);
            self.save();
            return Ok(self.lib.docs[i].clone());
        }
        let name = name.trim_end_matches(".pdf").trim_end_matches(".PDF").to_string();
        let meta = DocMeta {
            name: if name.is_empty() { "Dokument".into() } else { name },
            size: bytes.len() as u64,
            pages: page_count(bytes).unwrap_or(0),
            encrypted: lightpdf_core::pdfdoc::needs_password(bytes),
            path,
            ..DocMeta::default()
        };
        std::fs::write(self.doc_path(&meta.id), bytes)?;
        self.lib.docs.push(meta.clone());
        self.save();
        Ok(meta)
    }

    pub fn delete(&mut self, id: &str) {
        let _ = std::fs::remove_file(self.doc_path(id));
        let _ = std::fs::remove_file(self.thumb_path(id));
        self.lib.docs.retain(|d| d.id != id);
        self.save();
    }

    pub fn update(&mut self, meta: &DocMeta) {
        if let Some(m) = self.lib.docs.iter_mut().find(|d| d.id == meta.id) {
            *m = meta.clone();
        }
        self.save();
    }

    pub fn get(&self, id: &str) -> Option<DocMeta> {
        self.lib.docs.iter().find(|d| d.id == id).cloned()
    }

    pub fn add_signature(&mut self, png: &[u8], initials: bool) -> std::io::Result<SavedSignature> {
        let s = SavedSignature { id: uid(), initials, created_at: now_ms() };
        std::fs::write(self.signature_path(&s.id), png)?;
        self.lib.signatures.push(s.clone());
        self.save();
        Ok(s)
    }

    pub fn delete_signature(&mut self, id: &str) {
        let _ = std::fs::remove_file(self.signature_path(id));
        self.lib.signatures.retain(|s| s.id != id);
        self.save();
    }
}

pub fn page_count(bytes: &[u8]) -> Option<usize> {
    lightpdf_core::pdfdoc::load(bytes, None).ok().map(|d| lightpdf_core::pdfdoc::pages(&d).len())
}

pub fn file_name(path: &Path) -> String {
    path.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "Dokument.pdf".into())
}

pub fn human_size(n: u64) -> String {
    let n = n as f64;
    if n < 1024.0 {
        format!("{n} B")
    } else if n < 1024.0 * 1024.0 {
        format!("{:.0} KB", n / 1024.0)
    } else {
        format!("{:.1} MB", n / 1024.0 / 1024.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn library_roundtrip() {
        let dir = std::env::temp_dir().join(format!("lp-test-{}", uid()));
        let mut s = Storage::open(dir.clone());
        let pdf = lightpdf_core::ops::blank_pdf(2, lightpdf_core::ops::A4).unwrap();
        let m = s.add("Test.pdf", &pdf, Some("/x/Test.pdf".into())).unwrap();
        assert_eq!((m.name.as_str(), m.pages), ("Test", 2));
        // same path again → same entry
        let m2 = s.add("Test.pdf", &pdf, Some("/x/Test.pdf".into())).unwrap();
        assert_eq!(m.id, m2.id);
        assert_eq!(s.lib.docs.len(), 1);
        let s2 = Storage::open(dir.clone());
        assert_eq!(s2.lib.docs.len(), 1);
        assert_eq!(s2.read_doc(&m.id).unwrap(), pdf);
        let _ = std::fs::remove_dir_all(dir);
    }
}
