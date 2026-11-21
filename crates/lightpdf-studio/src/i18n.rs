//! German is the base language: the German text is the key (same scheme as v2).
//! English comes from v2's dictionary plus the entries below for new texts.

use std::collections::HashMap;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};

static EN: OnceLock<HashMap<String, String>> = OnceLock::new();
static ENGLISH: AtomicBool = AtomicBool::new(false);

const EXTRA: &[(&str, &str)] = &[
    ("Bibliothek", "Library"),
    ("Sortieren", "Sort"),
    ("Lesemodus", "Reading mode"),
    ("S.", "p."),
    ("Haken", "Check mark"),
    ("Kreuz", "Cross"),
    ("Punkt", "Dot"),
    ("Öffnen …", "Open …"),
    ("Speichern unter …", "Save as …"),
    ("Neu laden", "Reload"),
    ("Werkzeuge", "Tools"),
    ("Kommentieren", "Comment"),
    ("Lesen", "Read"),
    ("Seiten", "Pages"),
    ("Markieren", "Highlight"),
    ("Unterstreichen", "Underline"),
    ("Durchstreichen", "Strikethrough"),
    ("Stift", "Pen"),
    ("Textmarker", "Highlighter"),
    ("Rechteck", "Rectangle"),
    ("Ellipse", "Ellipse"),
    ("Linie", "Line"),
    ("Pfeil", "Arrow"),
    ("Textfeld", "Text box"),
    ("Notiz", "Note"),
    ("Radierer", "Eraser"),
    ("Schwärzen", "Redact"),
    ("Unterschrift", "Signature"),
    ("Stempel", "Stamp"),
    ("Hand", "Hand"),
    ("Nur Stift zeichnet (Finger blättern)", "Stylus only draws (fingers scroll)"),
    ("Stiftmodus", "Stylus mode"),
    ("Mit zwei Fingern zoomen – es entsteht keine Markierung.", "Pinch with two fingers to zoom – no markup is created."),
    ("Kein Text an dieser Stelle", "No text here"),
    ("Schwärzungen anwenden", "Apply redactions"),
    (
        "Schwärzungen werden beim Anwenden endgültig in die Seiten gebrannt.",
        "Redactions are burned permanently into the pages when applied.",
    ),
    ("Ungespeicherte Änderungen", "Unsaved changes"),
    ("Änderungen speichern?", "Save changes?"),
    ("Nicht speichern", "Don't save"),
    ("Gespeichert", "Saved"),
    ("Dateien", "Files"),
    ("Ordner wählen", "Choose folder"),
    ("Hierher", "Here"),
    ("Nach oben", "Up"),
    ("Keine PDF-Dateien in diesem Ordner", "No PDF files in this folder"),
    ("In Downloads exportiert", "Exported to Downloads"),
    ("Zeichne deine Unterschrift", "Draw your signature"),
    ("Löschen", "Delete"),
    ("Übernehmen", "Apply"),
    ("Zeichnen", "Draw"),
    ("Tippen", "Type"),
    ("Gespeicherte", "Saved"),
    ("Initialen", "Initials"),
    ("Fertig", "Done"),
    ("Formular ausfüllen", "Fill form"),
    ("Formular fixieren", "Flatten form"),
    ("Seite {0} von {1}", "Page {0} of {1}"),
    ("{0} Treffer", "{0} matches"),
    ("Keine Treffer", "No matches"),
    ("Suchen …", "Search …"),
    ("Zoom", "Zoom"),
    ("Seitenbreite", "Page width"),
    ("Ganze Seite", "Whole page"),
    ("Kommentare", "Comments"),
    ("Miniaturen", "Thumbnails"),
    ("Lesezeichen", "Bookmarks"),
    ("Sprache", "Language"),
    ("Hell", "Light"),
    ("Dunkel", "Dark"),
    ("System", "System"),
    ("Einstellungen", "Settings"),
    ("Über", "About"),
    (
        "Freier, quelloffener PDF-Reader und -Editor. Komplett in Rust.",
        "Free, open-source PDF reader and editor. Written entirely in Rust.",
    ),
    ("Noch keine Dokumente", "No documents yet"),
    ("Öffne ein PDF, um loszulegen.", "Open a PDF to get started."),
    ("Kennwort", "Password"),
    ("Kennwort erforderlich", "Password required"),
    ("Falsches Kennwort", "Wrong password"),
    ("Entsperren", "Unlock"),
    ("Kopieren", "Copy"),
    ("Text kopiert", "Text copied"),
    ("Seiten organisieren", "Organize pages"),
    ("Seite einfügen", "Insert page"),
    ("Leere Seite", "Blank page"),
    ("PDF einfügen …", "Insert PDF …"),
    ("Duplizieren", "Duplicate"),
    ("Drehen", "Rotate"),
    ("Extrahieren", "Extract"),
    ("Zusammenführen", "Merge"),
    ("Teilen", "Split"),
    ("Komprimieren", "Compress"),
    ("Schützen", "Protect"),
    ("Kennwort entfernen", "Remove password"),
    ("Wasserzeichen", "Watermark"),
    ("Seitenzahlen", "Page numbers"),
    ("Eigenschaften", "Properties"),
    ("Exportieren", "Export"),
    ("Leeres PDF", "Blank PDF"),
    ("Rückgängig", "Undo"),
    ("Wiederholen", "Redo"),
    ("Speichern", "Save"),
    ("Schließen", "Close"),
    ("Abbrechen", "Cancel"),
    ("OK", "OK"),
    ("Farbe", "Color"),
    ("Stärke", "Width"),
    ("Füllung", "Fill"),
    ("Schriftgröße", "Font size"),
    ("Fett", "Bold"),
    ("Kommentar", "Comment"),
    ("Text eingeben …", "Enter text …"),
    ("Notiz schreiben …", "Write a note …"),
    ("Genehmigt", "Approved"),
    ("Entwurf", "Draft"),
    ("Vertraulich", "Confidential"),
    ("Abgelehnt", "Rejected"),
    ("Erledigt", "Done"),
    ("Eigener Stempel …", "Custom stamp …"),
    ("Mit Kommentaren", "With comments"),
    ("Kommentare einbrennen", "Burn in comments"),
    ("Nachtmodus", "Night mode"),
    ("Sepia", "Sepia"),
    ("Normal", "Normal"),
    ("Vollbild", "Full screen"),
    ("Ansicht drehen", "Rotate view"),
    ("Doppelseite", "Two pages"),
    ("Fortlaufend", "Continuous"),
    ("Einzelseite", "Single page"),
    ("Horizontal", "Horizontal"),
];

fn table() -> &'static HashMap<String, String> {
    EN.get_or_init(|| {
        let mut m: HashMap<String, String> = serde_json::from_str(include_str!("i18n_en.json")).unwrap_or_default();
        for (k, v) in EXTRA {
            m.entry((*k).to_string()).or_insert_with(|| (*v).to_string());
        }
        m
    })
}

pub fn set_english(en: bool) {
    ENGLISH.store(en, Ordering::Relaxed);
}

pub fn is_english() -> bool {
    ENGLISH.load(Ordering::Relaxed)
}

/// Translate a German text (the key).
pub fn tr(s: &str) -> String {
    if is_english()
        && let Some(v) = table().get(s)
    {
        return v.clone();
    }
    s.to_string()
}

/// Translate with `{0}`, `{1}` … placeholders.
pub fn trf(s: &str, args: &[&dyn std::fmt::Display]) -> String {
    let mut out = tr(s);
    for (i, a) in args.iter().enumerate() {
        out = out.replace(&format!("{{{i}}}"), &a.to_string());
    }
    out
}

/// System language guess (for the first start).
pub fn system_is_german() -> bool {
    ["LC_ALL", "LC_MESSAGES", "LANG", "LANGUAGE"].iter().filter_map(|k| std::env::var(k).ok()).any(|v| v.starts_with("de"))
}
