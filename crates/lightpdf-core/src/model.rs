//! Data model of LightPDF Studio.
//!
//! The JSON form of [`Annotation`] is identical to the one written by LightPDF Studio 2.x
//! (TypeScript), so `/LPData` entries in PDFs saved with the old app stay editable.
//! All coordinates are PDF points in the (unrotated) user space of the page.

use serde::{Deserialize, Serialize};
use std::sync::Arc;

pub type Pt = [f64; 2];
/// x1, y1, x2, y2 with x1 < x2 and y1 < y2.
pub type Rect = [f64; 4];

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Annotation {
    pub id: String,
    /// 1-based page number.
    pub page: u32,
    pub color: String,
    pub opacity: f64,
    pub created_at: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    /// Comment attached to the object.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    #[serde(flatten)]
    pub kind: Kind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ImageKind {
    Signature,
    Image,
    Stamp,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum Kind {
    Highlight {
        rects: Vec<Rect>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        text: Option<String>,
    },
    Underline {
        rects: Vec<Rect>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        text: Option<String>,
    },
    Strike {
        rects: Vec<Rect>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        text: Option<String>,
    },
    Ink {
        paths: Vec<Vec<Pt>>,
        width: f64,
        #[serde(default, skip_serializing_if = "is_false")]
        highlighter: bool,
    },
    Rect {
        rect: Rect,
        width: f64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        fill: Option<String>,
    },
    Ellipse {
        rect: Rect,
        width: f64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        fill: Option<String>,
    },
    Line {
        from: Pt,
        to: Pt,
        width: f64,
    },
    Arrow {
        from: Pt,
        to: Pt,
        width: f64,
    },
    Text {
        rect: Rect,
        text: String,
        #[serde(rename = "fontSize")]
        font_size: f64,
        #[serde(default, skip_serializing_if = "is_false")]
        bold: bool,
    },
    Note {
        at: Pt,
        text: String,
    },
    Image {
        rect: Rect,
        /// `data:image/png;base64,…` (shared, so undo snapshots stay cheap)
        src: Arc<str>,
        kind: ImageKind,
    },
    Redact {
        rect: Rect,
    },
}

fn is_false(b: &bool) -> bool {
    !*b
}

impl Annotation {
    pub fn type_name(&self) -> &'static str {
        match &self.kind {
            Kind::Highlight { .. } => "highlight",
            Kind::Underline { .. } => "underline",
            Kind::Strike { .. } => "strike",
            Kind::Ink { .. } => "ink",
            Kind::Rect { .. } => "rect",
            Kind::Ellipse { .. } => "ellipse",
            Kind::Line { .. } => "line",
            Kind::Arrow { .. } => "arrow",
            Kind::Text { .. } => "text",
            Kind::Note { .. } => "note",
            Kind::Image { .. } => "image",
            Kind::Redact { .. } => "redact",
        }
    }

    pub fn is_markup(&self) -> bool {
        matches!(self.kind, Kind::Highlight { .. } | Kind::Underline { .. } | Kind::Strike { .. })
    }

    /// Text shown as PDF `/Contents`.
    pub fn contents(&self) -> Option<&str> {
        match &self.kind {
            Kind::Note { text, .. } | Kind::Text { text, .. } => Some(text.as_str()),
            Kind::Highlight { text, .. } | Kind::Underline { text, .. } | Kind::Strike { text, .. } => {
                self.note.as_deref().or(text.as_deref())
            }
            _ => self.note.as_deref(),
        }
        .filter(|s| !s.is_empty())
    }

    /// Bounding box in PDF space (including stroke width), like `annBounds` of v2.
    pub fn bounds(&self) -> Rect {
        match &self.kind {
            Kind::Highlight { rects, .. } | Kind::Underline { rects, .. } | Kind::Strike { rects, .. } => {
                union_rects(rects.iter().copied())
            }
            Kind::Ink { paths, width, .. } => {
                let w = width / 2.0 + 1.0;
                inflate(points_bounds(paths.iter().flatten().copied()), w)
            }
            Kind::Line { from, to, width } | Kind::Arrow { from, to, width } => inflate(points_bounds([*from, *to]), width * 4.0 + 2.0),
            Kind::Note { at, .. } => [at[0], at[1] - 20.0, at[0] + 20.0, at[1]],
            Kind::Rect { rect, width, .. } | Kind::Ellipse { rect, width, .. } => inflate(*rect, width / 2.0),
            Kind::Text { rect, .. } | Kind::Image { rect, .. } | Kind::Redact { rect } => *rect,
        }
    }

    /// Move by a vector in PDF space.
    pub fn translate(&mut self, dx: f64, dy: f64) {
        let mp = |p: &mut Pt| {
            p[0] += dx;
            p[1] += dy;
        };
        let mr = |r: &mut Rect| {
            r[0] += dx;
            r[2] += dx;
            r[1] += dy;
            r[3] += dy;
        };
        match &mut self.kind {
            Kind::Highlight { rects, .. } | Kind::Underline { rects, .. } | Kind::Strike { rects, .. } => rects.iter_mut().for_each(mr),
            Kind::Ink { paths, .. } => paths.iter_mut().flatten().for_each(mp),
            Kind::Line { from, to, .. } | Kind::Arrow { from, to, .. } => {
                mp(from);
                mp(to);
            }
            Kind::Note { at, .. } => mp(at),
            Kind::Rect { rect, .. }
            | Kind::Ellipse { rect, .. }
            | Kind::Text { rect, .. }
            | Kind::Image { rect, .. }
            | Kind::Redact { rect } => mr(rect),
        }
    }

    pub fn rect_mut(&mut self) -> Option<&mut Rect> {
        match &mut self.kind {
            Kind::Rect { rect, .. }
            | Kind::Ellipse { rect, .. }
            | Kind::Text { rect, .. }
            | Kind::Image { rect, .. }
            | Kind::Redact { rect } => Some(rect),
            _ => None,
        }
    }

    pub fn rect(&self) -> Option<Rect> {
        match &self.kind {
            Kind::Rect { rect, .. }
            | Kind::Ellipse { rect, .. }
            | Kind::Text { rect, .. }
            | Kind::Image { rect, .. }
            | Kind::Redact { rect } => Some(*rect),
            _ => None,
        }
    }
}

pub fn norm_rect(a: Pt, b: Pt) -> Rect {
    [a[0].min(b[0]), a[1].min(b[1]), a[0].max(b[0]), a[1].max(b[1])]
}

pub fn inflate(r: Rect, d: f64) -> Rect {
    [r[0] - d, r[1] - d, r[2] + d, r[3] + d]
}

pub fn union_rects(rs: impl IntoIterator<Item = Rect>) -> Rect {
    let mut out = [f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY];
    for r in rs {
        out[0] = out[0].min(r[0]);
        out[1] = out[1].min(r[1]);
        out[2] = out[2].max(r[2]);
        out[3] = out[3].max(r[3]);
    }
    if !out[0].is_finite() {
        return [0.0; 4];
    }
    out
}

pub fn points_bounds(ps: impl IntoIterator<Item = Pt>) -> Rect {
    union_rects(ps.into_iter().map(|p| [p[0], p[1], p[0], p[1]]))
}

pub fn now_ms() -> f64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as f64).unwrap_or(0.0)
}

/// Random id, same shape as v2 (`uid()`): base36 time + random suffix.
pub fn uid() -> String {
    let t = now_ms() as u64;
    let r: u64 = rand::random::<u64>() % 36u64.pow(6);
    format!("{}{}", base36(t), base36(r))
}

fn base36(mut n: u64) -> String {
    const D: &[u8] = b"0123456789abcdefghijklmnopqrstuvwxyz";
    if n == 0 {
        return "0".into();
    }
    let mut s = Vec::new();
    while n > 0 {
        s.push(D[(n % 36) as usize]);
        n /= 36;
    }
    s.reverse();
    String::from_utf8(s).unwrap()
}

pub fn hex_to_rgb(hex: &str) -> [f64; 3] {
    let h = hex.trim_start_matches('#');
    let h: String = if h.len() == 3 { h.chars().flat_map(|c| [c, c]).collect() } else { h.chars().take(6).collect() };
    let n = u32::from_str_radix(&h, 16).unwrap_or(0);
    [((n >> 16) & 255) as f64 / 255.0, ((n >> 8) & 255) as f64 / 255.0, (n & 255) as f64 / 255.0]
}

pub fn rgb_to_hex(c: [u8; 3]) -> String {
    format!("#{:02x}{:02x}{:02x}", c[0], c[1], c[2])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_compatible_with_v2() {
        // Exactly what LightPDF Studio 2.x (TypeScript) wrote into /LPData
        let js = r##"{"id":"lx1abc","page":2,"color":"#e5484d","opacity":1,"createdAt":1731000000000,"type":"ink","paths":[[[10,20],[30,40.5]]],"width":2,"highlighter":true}"##;
        let a: Annotation = serde_json::from_str(js).unwrap();
        assert_eq!(a.page, 2);
        assert!(matches!(a.kind, Kind::Ink { highlighter: true, .. }));
        let back = serde_json::to_value(&a).unwrap();
        assert_eq!(back["type"], "ink");
        assert_eq!(back["createdAt"], 1731000000000.0);

        let t = r##"{"id":"t","page":1,"color":"#000","opacity":1,"createdAt":0,"type":"text","rect":[1,2,3,4],"text":"Hi","fontSize":12,"bold":true,"author":"Me"}"##;
        let a: Annotation = serde_json::from_str(t).unwrap();
        assert_eq!(a.author.as_deref(), Some("Me"));
        let v = serde_json::to_value(&a).unwrap();
        assert_eq!(v["fontSize"], 12.0);

        let img = r##"{"id":"i","page":1,"color":"#000","opacity":1,"createdAt":0,"type":"image","rect":[1,2,3,4],"src":"data:image/png;base64,AA==","kind":"signature"}"##;
        let a: Annotation = serde_json::from_str(img).unwrap();
        assert!(matches!(a.kind, Kind::Image { kind: ImageKind::Signature, .. }));
    }

    #[test]
    fn colors() {
        assert_eq!(hex_to_rgb("#ff0000"), [1.0, 0.0, 0.0]);
        assert_eq!(hex_to_rgb("#0f0"), [0.0, 1.0, 0.0]);
        assert_eq!(rgb_to_hex([1, 2, 255]), "#0102ff");
    }
}
