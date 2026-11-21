//! Page geometry: PDF user space ⇄ view space (points, y down, rotation applied).
//!
//! Same convention as pdf.js viewports at scale 1, so the maths match v2.

use crate::model::{Pt, Rect, norm_rect};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PageGeom {
    /// Visible box (crop box ∩ media box) in user space: x0, y0, x1, y1.
    pub view_box: Rect,
    /// Total clockwise rotation (page /Rotate + view rotation), 0/90/180/270.
    pub rotation: u32,
}

impl PageGeom {
    pub fn new(view_box: Rect, rotation: i32) -> Self {
        Self { view_box, rotation: rotation.rem_euclid(360) as u32 / 90 * 90 }
    }

    pub fn with_extra_rotation(&self, delta: i32) -> Self {
        Self::new(self.view_box, self.rotation as i32 + delta)
    }

    pub fn box_w(&self) -> f64 {
        self.view_box[2] - self.view_box[0]
    }
    pub fn box_h(&self) -> f64 {
        self.view_box[3] - self.view_box[1]
    }

    /// Size in view points (rotated).
    pub fn width(&self) -> f64 {
        if self.rotation.is_multiple_of(180) { self.box_w() } else { self.box_h() }
    }
    pub fn height(&self) -> f64 {
        if self.rotation.is_multiple_of(180) { self.box_h() } else { self.box_w() }
    }

    /// Affine `[a, b, c, d, e, f]` mapping user space → view space:
    /// `vx = a·x + c·y + e`, `vy = b·x + d·y + f`.
    pub fn affine(&self) -> [f64; 6] {
        let [x0, y0, x1, y1] = self.view_box;
        match self.rotation {
            90 => [0.0, 1.0, 1.0, 0.0, -y0, -x0],
            180 => [-1.0, 0.0, 0.0, 1.0, x1, -y0],
            270 => [0.0, -1.0, -1.0, 0.0, y1, x1],
            _ => [1.0, 0.0, 0.0, -1.0, -x0, y1],
        }
    }

    pub fn to_view(&self, p: Pt) -> Pt {
        let [a, b, c, d, e, f] = self.affine();
        [a * p[0] + c * p[1] + e, b * p[0] + d * p[1] + f]
    }

    pub fn to_pdf(&self, v: Pt) -> Pt {
        let [x0, y0, x1, y1] = self.view_box;
        let [vx, vy] = v;
        match self.rotation {
            90 => [vy + x0, vx + y0],
            180 => [x1 - vx, vy + y0],
            270 => [x1 - vy, y1 - vx],
            _ => [vx + x0, y1 - vy],
        }
    }

    pub fn rect_to_view(&self, r: Rect) -> Rect {
        norm_rect(self.to_view([r[0], r[1]]), self.to_view([r[2], r[3]]))
    }

    pub fn rect_to_pdf(&self, r: Rect) -> Rect {
        norm_rect(self.to_pdf([r[0], r[1]]), self.to_pdf([r[2], r[3]]))
    }

    /// Converts a vector (not a point) from view space to PDF space.
    pub fn vec_to_pdf(&self, d: Pt) -> Pt {
        let o = self.to_pdf([0.0, 0.0]);
        let p = self.to_pdf(d);
        [p[0] - o[0], p[1] - o[1]]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_all_rotations() {
        for r in [0, 90, 180, 270, -90, 450] {
            let g = PageGeom::new([10.0, 20.0, 610.0, 820.0], r);
            for p in [[10.0, 20.0], [300.0, 400.0], [610.0, 820.0], [11.5, 799.25]] {
                let v = g.to_view(p);
                let back = g.to_pdf(v);
                assert!((back[0] - p[0]).abs() < 1e-9 && (back[1] - p[1]).abs() < 1e-9, "rot {r}");
                assert!(v[0] >= -1e-9 && v[0] <= g.width() + 1e-9);
                assert!(v[1] >= -1e-9 && v[1] <= g.height() + 1e-9);
            }
        }
    }

    #[test]
    fn top_left_corner() {
        // Top-left of the unrotated page (x0, y1)
        let tl = [0.0, 800.0];
        assert_eq!(PageGeom::new([0.0, 0.0, 600.0, 800.0], 0).to_view(tl), [0.0, 0.0]);
        // rotated 90° clockwise it ends up top-right
        assert_eq!(PageGeom::new([0.0, 0.0, 600.0, 800.0], 90).to_view(tl), [800.0, 0.0]);
        assert_eq!(PageGeom::new([0.0, 0.0, 600.0, 800.0], 180).to_view(tl), [600.0, 800.0]);
        assert_eq!(PageGeom::new([0.0, 0.0, 600.0, 800.0], 270).to_view(tl), [0.0, 600.0]);
    }
}
