//! Low-level helpers around `lopdf`: loading (incl. passwords), saving, page tree access,
//! inherited page attributes and copying pages between documents.

use lopdf::{Dictionary, Document, Object, ObjectId, Stream, dictionary};
use std::collections::{BTreeMap, BTreeSet};

pub type Bytes = Vec<u8>;

#[derive(Debug, Clone, PartialEq)]
pub enum Error {
    /// The document is encrypted and needs a password (`wrong` = a password was given but rejected).
    PasswordNeeded {
        wrong: bool,
    },
    Invalid(String),
    Other(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::PasswordNeeded { wrong: false } => write!(f, "password required"),
            Error::PasswordNeeded { wrong: true } => write!(f, "wrong password"),
            Error::Invalid(s) => write!(f, "invalid PDF: {s}"),
            Error::Other(s) => write!(f, "{s}"),
        }
    }
}
impl std::error::Error for Error {}

impl From<lopdf::Error> for Error {
    fn from(e: lopdf::Error) -> Self {
        match e {
            lopdf::Error::InvalidPassword => Error::PasswordNeeded { wrong: true },
            lopdf::Error::Decryption(_) => Error::PasswordNeeded { wrong: true },
            e => Error::Other(e.to_string()),
        }
    }
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::Other(e.to_string())
    }
}

pub type Result<T> = std::result::Result<T, Error>;

/// Load a document. Encrypted documents are decrypted with `password` (empty user passwords
/// work without one); the result is always unencrypted in memory.
pub fn load(bytes: &[u8], password: Option<&str>) -> Result<Document> {
    let doc = match password {
        Some(pw) => Document::load_mem_with_options(bytes, lopdf::LoadOptions::with_password(pw)).map_err(|e| match e {
            lopdf::Error::InvalidPassword | lopdf::Error::Decryption(_) => Error::PasswordNeeded { wrong: true },
            e => Error::Invalid(e.to_string()),
        })?,
        None => Document::load_mem(bytes).map_err(|e| match e {
            lopdf::Error::InvalidPassword | lopdf::Error::Decryption(_) => Error::PasswordNeeded { wrong: false },
            e => Error::Invalid(e.to_string()),
        })?,
    };
    if doc.is_encrypted() {
        return Err(Error::PasswordNeeded { wrong: password.is_some() });
    }
    Ok(doc)
}

/// Is the file encrypted (with a non-empty user password)?
pub fn needs_password(bytes: &[u8]) -> bool {
    matches!(load(bytes, None), Err(Error::PasswordNeeded { .. }))
}

pub fn save(doc: &mut Document) -> Result<Bytes> {
    let mut out = Vec::new();
    // PDF/A-1 forbids object streams, PDF < 1.5 doesn't know them: keep the classic layout there.
    let modern = doc.version.as_str() >= "1.5" && pdfa_part(doc).is_none() && !doc.is_encrypted();
    if modern {
        doc.save_with_options(&mut out, lopdf::SaveOptions::builder().use_object_streams(true).use_xref_streams(true).build())?;
    } else {
        doc.save_to(&mut out)?;
    }
    Ok(out)
}

// ------------------------------------------------------------------ object helpers

pub fn deref<'a>(doc: &'a Document, o: &'a Object) -> &'a Object {
    doc.dereference(o).map(|(_, o)| o).unwrap_or(o)
}

pub fn as_f64(o: &Object) -> Option<f64> {
    match o {
        Object::Integer(i) => Some(*i as f64),
        Object::Real(r) => Some(*r as f64),
        _ => None,
    }
}

pub fn num_array(doc: &Document, o: &Object) -> Option<Vec<f64>> {
    match deref(doc, o) {
        Object::Array(a) => a.iter().map(|x| as_f64(deref(doc, x))).collect(),
        _ => None,
    }
}

pub fn rect_obj(r: [f64; 4]) -> Object {
    Object::Array(r.iter().map(|v| Object::Real(*v as f32)).collect())
}

pub fn reals(v: &[f64]) -> Object {
    Object::Array(v.iter().map(|v| Object::Real(*v as f32)).collect())
}

pub fn name(s: &str) -> Object {
    Object::Name(s.as_bytes().to_vec())
}

/// PDF text string (UTF-16BE with BOM, hex) like pdf-lib's `PDFHexString.fromText`.
pub fn text_hex(s: &str) -> Object {
    let mut b = vec![0xFE, 0xFF];
    for u in s.encode_utf16() {
        b.extend_from_slice(&u.to_be_bytes());
    }
    Object::String(b, lopdf::StringFormat::Hexadecimal)
}

pub fn literal(s: &str) -> Object {
    Object::String(s.as_bytes().to_vec(), lopdf::StringFormat::Literal)
}

pub fn decode_text(doc: &Document, o: &Object) -> Option<String> {
    match deref(doc, o) {
        Object::String(b, _) => Some(decode_text_bytes(b)),
        _ => None,
    }
}

pub fn decode_text_bytes(b: &[u8]) -> String {
    if b.len() >= 2 && b[0] == 0xFE && b[1] == 0xFF {
        let u: Vec<u16> = b[2..].chunks(2).map(|c| u16::from_be_bytes([c[0], *c.get(1).unwrap_or(&0)])).collect();
        String::from_utf16_lossy(&u)
    } else if b.len() >= 3 && b[..3] == [0xEF, 0xBB, 0xBF] {
        String::from_utf8_lossy(&b[3..]).into_owned()
    } else {
        // PDFDocEncoding ≈ Latin-1 for the printable range
        b.iter().map(|&c| crate::text::pdfdoc_char(c)).collect()
    }
}

// ------------------------------------------------------------------ pages

/// Page object ids in order.
pub fn pages(doc: &Document) -> Vec<ObjectId> {
    doc.get_pages().into_values().collect()
}

/// Look up an attribute on the page or inherit it from its ancestors.
pub fn inherited<'a>(doc: &'a Document, page: ObjectId, key: &[u8]) -> Option<&'a Object> {
    let mut cur = doc.get_dictionary(page).ok()?;
    for _ in 0..64 {
        if let Ok(v) = cur.get(key) {
            return Some(v);
        }
        let parent = cur.get(b"Parent").ok()?.as_reference().ok()?;
        cur = doc.get_dictionary(parent).ok()?;
    }
    None
}

pub fn rotation(doc: &Document, page: ObjectId) -> i32 {
    let r = inherited(doc, page, b"Rotate").and_then(|o| as_f64(deref(doc, o))).unwrap_or(0.0) as i32;
    ((r % 360) + 360) % 360
}

pub fn set_rotation(doc: &mut Document, page: ObjectId, deg: i32) {
    if let Ok(d) = doc.get_dictionary_mut(page) {
        d.set("Rotate", Object::Integer(deg.rem_euclid(360) as i64));
    }
}

pub fn media_box(doc: &Document, page: ObjectId) -> [f64; 4] {
    let mb = inherited(doc, page, b"MediaBox").and_then(|o| num_array(doc, o)).filter(|v| v.len() == 4);
    match mb {
        Some(v) => [v[0].min(v[2]), v[1].min(v[3]), v[0].max(v[2]), v[1].max(v[3])],
        None => [0.0, 0.0, 595.28, 841.89],
    }
}

/// Visible box (crop box ∩ media box).
pub fn view_box(doc: &Document, page: ObjectId) -> [f64; 4] {
    let mb = media_box(doc, page);
    let cb = inherited(doc, page, b"CropBox").and_then(|o| num_array(doc, o)).filter(|v| v.len() == 4);
    match cb {
        Some(v) => {
            let c = [v[0].min(v[2]), v[1].min(v[3]), v[0].max(v[2]), v[1].max(v[3])];
            let r = [c[0].max(mb[0]), c[1].max(mb[1]), c[2].min(mb[2]), c[3].min(mb[3])];
            if r[2] - r[0] < 1.0 || r[3] - r[1] < 1.0 { mb } else { r }
        }
        None => mb,
    }
}

fn root_pages_id(doc: &Document) -> Result<ObjectId> {
    doc.catalog()?.get(b"Pages").and_then(Object::as_reference).map_err(|_| Error::Invalid("no page tree".into()))
}

/// Copies inherited attributes onto every page so pages can be moved freely.
pub fn materialize_inherited(doc: &mut Document) {
    let ids = pages(doc);
    for id in ids {
        let mut add = Vec::new();
        for key in [&b"Resources"[..], b"MediaBox", b"CropBox", b"Rotate"] {
            let own = doc.get_dictionary(id).map(|d| d.has(key)).unwrap_or(true);
            if !own && let Some(v) = inherited(doc, id, key) {
                add.push((key.to_vec(), v.clone()));
            }
        }
        if let Ok(d) = doc.get_dictionary_mut(id) {
            for (k, v) in add {
                d.set(k, v);
            }
        }
    }
}

/// Replaces the page tree by a flat list of `order` (page object ids, duplicates allowed:
/// duplicates are cloned). Pages not in the list are removed.
pub fn set_pages(doc: &mut Document, order: &[ObjectId]) -> Result<Vec<ObjectId>> {
    materialize_inherited(doc);
    let root = root_pages_id(doc)?;
    let mut seen = BTreeSet::new();
    let mut kids = Vec::with_capacity(order.len());
    for &id in order {
        let id = if seen.insert(id) { id } else { clone_page(doc, id)? };
        kids.push(id);
    }
    for &id in &kids {
        doc.get_dictionary_mut(id)?.set("Parent", Object::Reference(root));
    }
    let rd = doc.get_dictionary_mut(root)?;
    rd.set("Kids", Object::Array(kids.iter().map(|&k| Object::Reference(k)).collect()));
    rd.set("Count", Object::Integer(kids.len() as i64));
    rd.remove(b"Resources");
    rd.remove(b"MediaBox");
    rd.remove(b"CropBox");
    rd.remove(b"Rotate");
    // Remove orphaned intermediate /Pages nodes and removed pages.
    doc.prune_objects();
    Ok(kids)
}

/// Shallow copy of a page (content and resources shared) with its own annotations.
fn clone_page(doc: &mut Document, id: ObjectId) -> Result<ObjectId> {
    let mut d = doc.get_dictionary(id)?.clone();
    let new_id = doc.new_object_id();
    if let Ok(annots) = d.get(b"Annots") {
        let list: Vec<Object> = match deref(doc, annots) {
            Object::Array(a) => a.clone(),
            _ => vec![],
        };
        let mut out = Vec::new();
        for a in list {
            let Ok(ad) = deref(doc, &a).as_dict() else { continue };
            // Form fields belong to exactly one page; drop them on duplicates.
            if ad.get(b"Subtype").and_then(Object::as_name).ok() == Some(b"Widget") {
                continue;
            }
            let mut ad = ad.clone();
            ad.set("P", Object::Reference(new_id));
            ad.remove(b"Popup");
            out.push(Object::Reference(doc.add_object(Object::Dictionary(ad))));
        }
        d.set("Annots", Object::Array(out));
    }
    doc.objects.insert(new_id, Object::Dictionary(d));
    Ok(new_id)
}

/// Copies pages (0-based indices) of `src` into `dst`, returns the new page ids (not yet in
/// the page tree of `dst`).
pub fn import_pages(dst: &mut Document, src: &Document, indices: &[usize]) -> Result<Vec<ObjectId>> {
    let mut src = src.clone();
    materialize_inherited(&mut src);
    let src_pages = pages(&src);
    let src_root = root_pages_id(&src)?;
    // Keep only the wanted pages in the copy (duplicates cloned).
    let wanted: Vec<ObjectId> = indices.iter().filter_map(|&i| src_pages.get(i).copied()).collect();
    let kept = set_pages(&mut src, &wanted)?;
    // Detach from the source tree: references to the root / catalog must not drag them along.
    let catalog_id = src.trailer.get(b"Root").and_then(Object::as_reference).ok();
    src.renumber_objects_with(dst.max_id + 1);
    let new_pages = pages(&src);
    debug_assert_eq!(new_pages.len(), kept.len());
    let new_root = root_pages_id(&src)?;
    let new_catalog = src.trailer.get(b"Root").and_then(Object::as_reference).ok();
    let _ = (src_root, catalog_id);
    for (id, obj) in std::mem::take(&mut src.objects) {
        if Some(id) == new_catalog || id == new_root {
            continue;
        }
        dst.objects.insert(id, obj);
    }
    dst.max_id = dst.max_id.max(src.max_id);
    Ok(new_pages)
}

/// Inserts already imported page objects at a position (0-based) of the page order.
pub fn insert_page_ids(doc: &mut Document, at: usize, ids: &[ObjectId]) -> Result<()> {
    let mut order = pages(doc);
    let at = at.min(order.len());
    for (i, id) in ids.iter().enumerate() {
        order.insert(at + i, *id);
    }
    set_pages(doc, &order)?;
    Ok(())
}

pub fn new_document() -> Document {
    let mut doc = Document::with_version("1.7");
    let pages_id = doc.new_object_id();
    doc.objects.insert(pages_id, Object::Dictionary(dictionary! { "Type" => "Pages", "Kids" => Vec::<Object>::new(), "Count" => 0 }));
    let catalog = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
    doc.trailer.set("Root", catalog);
    doc
}

/// Appends a page with the given content stream and resources.
pub fn add_page(doc: &mut Document, size: [f64; 2], content: Vec<u8>, resources: Dictionary) -> Result<ObjectId> {
    let root = root_pages_id(doc)?;
    let content_id = doc.add_object(Stream::new(Dictionary::new(), content));
    let page = doc.add_object(dictionary! {
        "Type" => "Page",
        "Parent" => root,
        "MediaBox" => rect_obj([0.0, 0.0, size[0], size[1]]),
        "Contents" => content_id,
        "Resources" => resources,
    });
    let mut order = pages(doc);
    order.push(page);
    set_pages(doc, &order)?;
    Ok(page)
}

// ------------------------------------------------------------------ resources & content

/// Gets the page's own /Resources dictionary (materialising inherited/indirect ones), mutable.
pub fn page_resources_mut(doc: &mut Document, page: ObjectId) -> Result<&mut Dictionary> {
    let res = inherited(doc, page, b"Resources").cloned();
    let res = match res {
        Some(Object::Reference(r)) => doc.get_dictionary(r).cloned().unwrap_or_default(),
        Some(Object::Dictionary(d)) => d,
        _ => Dictionary::new(),
    };
    // Resolve indirect sub-dictionaries we write into
    let mut res = res;
    for key in [&b"XObject"[..], b"ExtGState", b"Font"] {
        if let Ok(Object::Reference(r)) = res.get(key)
            && let Ok(d) = doc.get_dictionary(*r)
        {
            res.set(key.to_vec(), Object::Dictionary(d.clone()));
        }
    }
    doc.get_dictionary_mut(page)?.set("Resources", Object::Dictionary(res));
    Ok(doc.get_dictionary_mut(page)?.get_mut(b"Resources")?.as_dict_mut()?)
}

/// Adds an entry to a resource category under a fresh name and returns the name.
pub fn add_resource(doc: &mut Document, page: ObjectId, category: &str, prefix: &str, obj: Object) -> Result<String> {
    let res = page_resources_mut(doc, page)?;
    if !res.has(category.as_bytes()) {
        res.set(category, Dictionary::new());
    }
    let cat = res.get_mut(category.as_bytes())?.as_dict_mut()?;
    let mut i = 0;
    let name = loop {
        let n = format!("{prefix}{i}");
        if !cat.has(n.as_bytes()) {
            break n;
        }
        i += 1;
    };
    cat.set(name.clone(), obj);
    Ok(name)
}

/// Appends content to a page, wrapping the existing content in q/Q so unbalanced graphics
/// state in the original can't affect what we draw.
pub fn append_content(doc: &mut Document, page: ObjectId, content: Vec<u8>) -> Result<()> {
    let existing: Vec<Object> = match doc.get_dictionary(page)?.get(b"Contents") {
        Ok(Object::Reference(id)) => match doc.get_object(*id) {
            Ok(Object::Array(a)) => a.clone(),
            _ => vec![Object::Reference(*id)],
        },
        Ok(Object::Array(a)) => a.clone(),
        _ => vec![],
    };
    let mut list = Vec::new();
    if !existing.is_empty() {
        list.push(Object::Reference(doc.add_object(Stream::new(Dictionary::new(), b"q\n".to_vec()))));
        list.extend(existing);
        list.push(Object::Reference(doc.add_object(Stream::new(Dictionary::new(), b"\nQ\n".to_vec()))));
    }
    let mut s = Stream::new(Dictionary::new(), content);
    let _ = s.compress();
    list.push(Object::Reference(doc.add_object(s)));
    doc.get_dictionary_mut(page)?.set("Contents", Object::Array(list));
    Ok(())
}

pub fn page_annots(doc: &Document, page: ObjectId) -> Vec<Object> {
    match doc.get_dictionary(page).ok().and_then(|d| d.get(b"Annots").ok()) {
        Some(o) => match deref(doc, o) {
            Object::Array(a) => a.clone(),
            _ => vec![],
        },
        None => vec![],
    }
}

pub fn set_page_annots(doc: &mut Document, page: ObjectId, annots: Vec<Object>) {
    if let Ok(d) = doc.get_dictionary_mut(page) {
        if annots.is_empty() {
            d.remove(b"Annots");
        } else {
            d.set("Annots", Object::Array(annots));
        }
    }
}

// ------------------------------------------------------------------ metadata / PDF/A

pub fn read_xmp(doc: &Document) -> Option<String> {
    let id = doc.catalog().ok()?.get(b"Metadata").ok()?.as_reference().ok()?;
    let s = doc.get_object(id).ok()?.as_stream().ok()?;
    let data = s.decompressed_content().unwrap_or_else(|_| s.content.clone());
    Some(String::from_utf8_lossy(&data).into_owned())
}

/// `Some("1")` etc. if the document claims PDF/A conformance.
pub fn pdfa_part(doc: &Document) -> Option<String> {
    let xml = read_xmp(doc)?;
    for pat in ["pdfaid:part=\"", "pdfaid:part='", "<pdfaid:part>"] {
        if let Some(i) = xml.find(pat) {
            let rest = xml[i + pat.len()..].trim_start();
            return rest.chars().next().filter(|c| c.is_ascii_digit()).map(|c| c.to_string());
        }
    }
    None
}

/// Removes the PDF/A claim (needed before encrypting, which PDF/A forbids).
pub fn strip_pdfa(doc: &mut Document) {
    let Some(xml) = read_xmp(doc) else { return };
    let mut out = String::with_capacity(xml.len());
    let mut rest = xml.as_str();
    // attribute form: pdfaid:part="1"
    'outer: while !rest.is_empty() {
        for key in ["pdfaid:part", "pdfaid:conformance", "pdfaid:amd", "pdfaid:rev"] {
            if rest.starts_with(key) {
                let after = &rest[key.len()..];
                let t = after.trim_start();
                if let Some(t) = t.strip_prefix('=') {
                    let t = t.trim_start();
                    if let Some(q) = t.chars().next().filter(|c| *c == '"' || *c == '\'')
                        && let Some(end) = t[1..].find(q)
                    {
                        rest = &t[end + 2..];
                        continue 'outer;
                    }
                }
                if let Some(t) = after.strip_prefix('>') {
                    let close = format!("</{key}>");
                    if let Some(end) = t.find(&close) {
                        rest = &t[end + close.len()..];
                        if out.ends_with('<') {
                            out.pop();
                        }
                        continue 'outer;
                    }
                }
            }
        }
        let c = rest.chars().next().unwrap();
        out.push(c);
        rest = &rest[c.len_utf8()..];
    }
    let stream = Stream::new(dictionary! { "Type" => "Metadata", "Subtype" => "XML" }, out.into_bytes());
    let id = doc.add_object(stream);
    if let Ok(c) = doc.catalog_mut() {
        c.set("Metadata", id);
    }
}

/// Info dictionary (created if missing).
pub fn info_mut(doc: &mut Document) -> &mut Dictionary {
    let id = match doc.trailer.get(b"Info").and_then(Object::as_reference) {
        Ok(id) if doc.get_dictionary(id).is_ok() => id,
        _ => {
            let id = doc.add_object(Dictionary::new());
            doc.trailer.set("Info", id);
            id
        }
    };
    doc.get_dictionary_mut(id).expect("info dict")
}

pub fn info_get(doc: &Document, key: &str) -> String {
    doc.trailer
        .get(b"Info")
        .ok()
        .map(|o| deref(doc, o))
        .and_then(|o| o.as_dict().ok())
        .and_then(|d| d.get(key.as_bytes()).ok())
        .and_then(|o| decode_text(doc, o))
        .unwrap_or_default()
}

pub fn pdf_date(ms: f64) -> String {
    let secs = (ms / 1000.0) as i64;
    let days = secs.div_euclid(86400);
    let tod = secs.rem_euclid(86400);
    let (y, m, d) = civil_from_days(days);
    format!("D:{y:04}{m:02}{d:02}{:02}{:02}{:02}Z", tod / 3600, (tod % 3600) / 60, tod % 60)
}

/// Days since 1970-01-01 → (year, month, day). (H. Hinnant's algorithm)
pub fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719468;
    let era = z.div_euclid(146097);
    let doe = z.rem_euclid(146097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// Parses `D:YYYYMMDDHHmmSS…` into "YYYY-MM-DD HH:mm".
pub fn format_pdf_date(s: &str) -> String {
    let s = s.trim_start_matches("D:");
    let g = |a: usize, b: usize| s.get(a..b).unwrap_or("");
    if s.len() < 8 {
        return s.to_string();
    }
    let mut out = format!("{}-{}-{}", g(0, 4), g(4, 6), g(6, 8));
    if s.len() >= 12 {
        out += &format!(" {}:{}", g(8, 10), g(10, 12));
    }
    out
}

/// Removes everything unreachable; small helper used after destructive edits.
pub fn tidy(doc: &mut Document) {
    doc.prune_objects();
}

pub fn object_ids_of_pages(doc: &Document) -> BTreeMap<ObjectId, usize> {
    pages(doc).into_iter().enumerate().map(|(i, id)| (id, i)).collect()
}
