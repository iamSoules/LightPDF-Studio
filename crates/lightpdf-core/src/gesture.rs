//! Gesture arbitration for touch, pen and mouse.
//!
//! This is the fix for the multi-touch markup problems of v2. There, every `pointerdown`
//! started a stroke, text box or note, while pinch-zoom ran separately on `touchstart`.
//! Pinching with a drawing tool therefore left stray ink, shapes or notes under the fingers,
//! and the finger that stayed down after a pinch started a new stroke.
//!
//! [`GestureArbiter`] owns *all* pointer input of the document canvas and decides, per
//! gesture, whether it is markup or navigation:
//!
//! * A single finger with a drawing tool is held **pending** (its points are buffered).
//!   It becomes markup only after it moved past the touch slop or was held for a moment.
//!   Buffered points are replayed, so the stroke starts exactly where the finger landed.
//! * A second finger turns the gesture into **navigation** (pinch zoom and two-finger pan).
//!   A pending stroke is dropped without a trace, and an already committed one is
//!   **cancelled** (the app discards it), so a pinch never leaves markup behind.
//! * Once a gesture is navigation, it stays navigation until *every* finger is lifted. The
//!   finger left over after a pinch can't start a stroke.
//! * Tap tools (text, note, signature, stamp…) only act on a clean single-finger **tap**.
//!   Dragging with such a tool scrolls the page instead of placing objects.
//! * Pen and mouse input is unambiguous and starts markup immediately. While a pen is
//!   down, touches are ignored (palm rejection).

use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PointerId(pub u64);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PointerKind {
    Touch,
    Pen,
    Mouse,
}

/// What a single pointer would do on its own with the current tool, decided by the app
/// when the first pointer goes down (it may hit-test, e.g. "select" on an object).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Intent {
    /// Drag tools: ink, shapes, eraser, text selection, moving a selected object …
    Draw,
    /// Tools that place something with a tap: text box, note, image, stamp, signature.
    Tap,
    /// Hand tool or empty space: one finger pans.
    Navigate,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    /// Start markup at a position (screen points).
    Begin {
        pos: [f32; 2],
        kind: PointerKind,
    },
    Move {
        pos: [f32; 2],
    },
    /// Markup finished; commit it.
    End {
        pos: [f32; 2],
    },
    /// Markup must be discarded (a second finger arrived): nothing may remain on the page.
    Cancel,
    /// Clean single tap (for tap tools, or tapping empty space with any tool).
    Tap {
        pos: [f32; 2],
        kind: PointerKind,
    },
    DoubleTap {
        pos: [f32; 2],
    },
    /// Navigation: move content by `pan`, then zoom by `zoom` around `center`.
    Navigate {
        pan: [f32; 2],
        zoom: f32,
        center: [f32; 2],
    },
    /// All fingers lifted after navigation; `velocity` in points/second for fling scrolling.
    NavigateEnd {
        velocity: [f32; 2],
    },
}

#[derive(Clone, Copy, Debug)]
pub struct Config {
    /// Distance (points) a finger may move before a pending touch counts as a drag.
    pub slop: f32,
    /// Hold time (s) after which a stationary finger commits to markup on its next move.
    pub hold: f64,
    /// Max time (s) between taps for a double tap.
    pub double_tap: f64,
    /// Max distance (points) between taps for a double tap.
    pub double_tap_dist: f32,
    /// Max duration (s) of a tap.
    pub tap_time: f64,
    /// Only pen/mouse draws; fingers always navigate (stylus mode).
    pub pen_only: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self { slop: 8.0, hold: 0.15, double_tap: 0.3, double_tap_dist: 30.0, tap_time: 0.35, pen_only: false }
    }
}

#[derive(Clone, Debug, PartialEq)]
enum Mode {
    Idle,
    /// Single touch, undecided. Buffered points (incl. the start).
    Pending {
        intent: Intent,
        start: [f32; 2],
        t0: f64,
        buf: Vec<[f32; 2]>,
    },
    /// Markup in progress for the given pointer.
    Drawing {
        id: PointerId,
    },
    /// Pan / pinch. Remains until all pointers are up.
    Navigating,
    /// Gesture was cancelled or rejected; ignore everything until all pointers are up.
    Ignoring,
}

#[derive(Clone, Copy, Debug)]
struct Ptr {
    kind: PointerKind,
    pos: [f32; 2],
}

pub struct GestureArbiter {
    pub config: Config,
    mode: Mode,
    ptrs: BTreeMap<PointerId, Ptr>,
    /// Baseline for navigation deltas: centroid and spread of the active touches.
    nav_base: Option<([f32; 2], f32, usize)>,
    vel: [f32; 2],
    last_nav_t: f64,
    last_tap: Option<(f64, [f32; 2])>,
}

impl Default for GestureArbiter {
    fn default() -> Self {
        Self::new(Config::default())
    }
}

fn dist(a: [f32; 2], b: [f32; 2]) -> f32 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2)).sqrt()
}

impl GestureArbiter {
    pub fn new(config: Config) -> Self {
        Self { config, mode: Mode::Idle, ptrs: BTreeMap::new(), nav_base: None, vel: [0.0; 2], last_nav_t: 0.0, last_tap: None }
    }

    /// True while markup is being drawn.
    pub fn is_drawing(&self) -> bool {
        matches!(self.mode, Mode::Drawing { .. })
    }
    /// True while the user pans/zooms with touch.
    pub fn is_navigating(&self) -> bool {
        matches!(self.mode, Mode::Navigating)
    }
    pub fn active_pointers(&self) -> usize {
        self.ptrs.len()
    }
    pub fn is_idle(&self) -> bool {
        self.ptrs.is_empty()
    }

    fn touches(&self) -> impl Iterator<Item = &Ptr> {
        self.ptrs.values().filter(|p| p.kind == PointerKind::Touch)
    }

    fn pen_down(&self) -> bool {
        self.ptrs.values().any(|p| p.kind != PointerKind::Touch)
    }

    fn touch_stats(&self) -> Option<([f32; 2], f32, usize)> {
        let pts: Vec<[f32; 2]> = self.touches().map(|p| p.pos).collect();
        if pts.is_empty() {
            return None;
        }
        let n = pts.len() as f32;
        let c = [pts.iter().map(|p| p[0]).sum::<f32>() / n, pts.iter().map(|p| p[1]).sum::<f32>() / n];
        let spread = if pts.len() >= 2 { pts.iter().map(|p| dist(*p, c)).sum::<f32>() / n } else { 0.0 };
        Some((c, spread, pts.len()))
    }

    fn start_navigation(&mut self, now: f64) {
        self.mode = Mode::Navigating;
        self.nav_base = self.touch_stats();
        self.vel = [0.0; 2];
        self.last_nav_t = now;
    }

    /// A pointer went down. `intent` is what the current tool would do with a single pointer
    /// at this position (only consulted for the first pointer of a gesture).
    pub fn down(&mut self, id: PointerId, kind: PointerKind, pos: [f32; 2], now: f64, intent: Intent) -> Vec<Action> {
        let mut out = Vec::new();
        // Palm rejection: a pen/mouse is drawing, touches don't exist.
        if kind == PointerKind::Touch && self.pen_down() {
            return out;
        }
        self.ptrs.insert(id, Ptr { kind, pos });

        if kind != PointerKind::Touch {
            // Pen / mouse: unambiguous. A pen landing cancels any finger gesture (palm).
            match &self.mode {
                Mode::Drawing { .. } => out.push(Action::Cancel),
                Mode::Navigating => out.push(Action::NavigateEnd { velocity: [0.0; 2] }),
                _ => {}
            }
            self.ptrs.retain(|_, p| p.kind != PointerKind::Touch);
            self.mode = match intent {
                Intent::Draw => {
                    out.push(Action::Begin { pos, kind });
                    Mode::Drawing { id }
                }
                Intent::Tap => Mode::Pending { intent, start: pos, t0: now, buf: vec![pos] },
                Intent::Navigate => {
                    self.nav_base = Some((pos, 0.0, 1));
                    self.vel = [0.0; 2];
                    self.last_nav_t = now;
                    Mode::Navigating
                }
            };
            return out;
        }

        let n_touch = self.touches().count();
        match std::mem::replace(&mut self.mode, Mode::Idle) {
            Mode::Idle => {
                let intent = if self.config.pen_only && intent == Intent::Draw { Intent::Navigate } else { intent };
                if intent == Intent::Navigate {
                    self.start_navigation(now);
                } else {
                    self.mode = Mode::Pending { intent, start: pos, t0: now, buf: vec![pos] };
                }
            }
            // Second finger before the first committed: navigation, nothing was drawn.
            Mode::Pending { .. } => self.start_navigation(now),
            // Second finger while drawing: discard the stroke, then navigate.
            Mode::Drawing { .. } => {
                out.push(Action::Cancel);
                self.start_navigation(now);
            }
            Mode::Navigating => {
                // Finger count changed: rebase so content doesn't jump.
                self.mode = Mode::Navigating;
                self.nav_base = self.touch_stats();
            }
            Mode::Ignoring => self.mode = Mode::Ignoring,
        }
        debug_assert!(n_touch >= 1);
        out
    }

    pub fn moved(&mut self, id: PointerId, pos: [f32; 2], now: f64) -> Vec<Action> {
        let mut out = Vec::new();
        let Some(p) = self.ptrs.get_mut(&id) else { return out };
        p.pos = pos;
        let kind = p.kind;
        match &mut self.mode {
            Mode::Pending { intent, start, t0, buf } => {
                let far = dist(*start, pos) > self.config.slop;
                let held = now - *t0 >= self.config.hold;
                match intent {
                    Intent::Draw => {
                        buf.push(pos);
                        if far || held {
                            let buf = std::mem::take(buf);
                            out.push(Action::Begin { pos: buf[0], kind });
                            out.extend(buf[1..].iter().map(|&p| Action::Move { pos: p }));
                            self.mode = Mode::Drawing { id };
                        }
                    }
                    Intent::Tap => {
                        // Dragging with a tap tool scrolls instead of placing.
                        if far {
                            if kind == PointerKind::Touch {
                                let s = *start;
                                self.start_navigation(now);
                                self.nav_base = Some((s, 0.0, 1));
                                out.extend(self.navigate(now));
                            } else {
                                self.mode = Mode::Ignoring;
                            }
                        }
                    }
                    Intent::Navigate => unreachable!(),
                }
            }
            Mode::Drawing { id: did } => {
                if *did == id {
                    out.push(Action::Move { pos });
                }
            }
            Mode::Navigating => out.extend(self.navigate(now)),
            Mode::Idle | Mode::Ignoring => {}
        }
        out
    }

    fn navigate(&mut self, now: f64) -> Vec<Action> {
        let stats = if self.pen_down() {
            // mouse/pen pan (hand tool)
            self.ptrs.values().next().map(|p| (p.pos, 0.0, 1))
        } else {
            self.touch_stats()
        };
        let (Some((c, s, n)), Some((c0, s0, n0))) = (stats, self.nav_base) else { return vec![] };
        self.nav_base = Some((c, s, n));
        if n != n0 {
            return vec![];
        }
        let pan = [c[0] - c0[0], c[1] - c0[1]];
        let zoom = if n >= 2 && s0 > 1.0 && s > 1.0 { s / s0 } else { 1.0 };
        let dt = (now - self.last_nav_t).max(1e-3) as f32;
        self.last_nav_t = now;
        let inst = [pan[0] / dt, pan[1] / dt];
        // Smoothed velocity for fling
        self.vel = [self.vel[0] * 0.6 + inst[0] * 0.4, self.vel[1] * 0.6 + inst[1] * 0.4];
        if pan == [0.0, 0.0] && zoom == 1.0 {
            return vec![];
        }
        vec![Action::Navigate { pan, zoom, center: c }]
    }

    pub fn up(&mut self, id: PointerId, pos: [f32; 2], now: f64) -> Vec<Action> {
        self.release(id, Some(pos), now)
    }

    /// The system cancelled the pointer (e.g. an OS gesture took over): nothing is committed.
    pub fn cancel(&mut self, id: PointerId, now: f64) -> Vec<Action> {
        self.release(id, None, now)
    }

    fn release(&mut self, id: PointerId, pos: Option<[f32; 2]>, _now: f64) -> Vec<Action> {
        let mut out = Vec::new();
        let Some(p) = self.ptrs.remove(&id) else { return out };
        let cancelled = pos.is_none();
        let pos = pos.unwrap_or(p.pos);
        match std::mem::replace(&mut self.mode, Mode::Idle) {
            Mode::Pending { intent, start, buf, .. } => match intent {
                _ if cancelled => {}
                Intent::Draw => {
                    // Quick stroke that never left the slop: replay it entirely (a dot / short line).
                    out.push(Action::Begin { pos: buf[0], kind: p.kind });
                    out.extend(buf[1..].iter().map(|&q| Action::Move { pos: q }));
                    out.push(Action::End { pos });
                }
                Intent::Tap => {
                    if dist(start, pos) <= self.config.slop {
                        out.push(Action::Tap { pos: start, kind: p.kind });
                    }
                }
                Intent::Navigate => {}
            },
            Mode::Drawing { id: did } if cancelled && did == id => out.push(Action::Cancel),
            Mode::Drawing { id: did } => {
                if did == id {
                    out.push(Action::End { pos });
                } else {
                    self.mode = Mode::Drawing { id: did };
                }
            }
            Mode::Navigating => {
                if self.ptrs.is_empty() {
                    out.push(Action::NavigateEnd { velocity: self.vel });
                    self.nav_base = None;
                    // A short, still single-finger touch in navigate mode is a tap.
                } else {
                    self.mode = Mode::Navigating;
                    self.nav_base = self.touch_stats();
                }
            }
            Mode::Ignoring => {
                if !self.ptrs.is_empty() {
                    self.mode = Mode::Ignoring;
                }
            }
            Mode::Idle => {}
        }
        // Navigation that ends with fingers still down → ignore the rest, never draw.
        if !self.ptrs.is_empty() && matches!(self.mode, Mode::Idle) {
            self.mode = Mode::Ignoring;
        }
        out
    }

    /// Call once per frame with the current time: commits a pending stationary *draw* finger
    /// after the hold time, so slow, careful strokes show a preview immediately.
    pub fn tick(&mut self, now: f64) -> Vec<Action> {
        let mut out = Vec::new();
        if let Mode::Pending { intent: Intent::Draw, t0, buf, .. } = &mut self.mode
            && now - *t0 >= self.config.hold * 2.0
        {
            let (&id, p) = self.ptrs.iter().next().expect("pending without pointer");
            let buf = std::mem::take(buf);
            out.push(Action::Begin { pos: buf[0], kind: p.kind });
            out.extend(buf[1..].iter().map(|&q| Action::Move { pos: q }));
            self.mode = Mode::Drawing { id };
        }
        out
    }

    /// Feed a navigation tap through double-tap detection. Returns `DoubleTap` on the second tap.
    pub fn classify_tap(&mut self, pos: [f32; 2], now: f64) -> Option<Action> {
        if let Some((t, p)) = self.last_tap
            && now - t <= self.config.double_tap
            && dist(p, pos) <= self.config.double_tap_dist
        {
            self.last_tap = None;
            return Some(Action::DoubleTap { pos });
        }
        self.last_tap = Some((now, pos));
        None
    }
}

/// Simple tap detector for navigation gestures (single finger, short, no movement).
/// Kept separate so the arbiter state machine stays small.
#[derive(Default)]
pub struct TapTracker {
    start: Option<(PointerId, [f32; 2], f64, bool)>,
}

impl TapTracker {
    pub fn down(&mut self, id: PointerId, pos: [f32; 2], now: f64, first: bool) {
        if first {
            self.start = Some((id, pos, now, true));
        } else if let Some(s) = &mut self.start {
            s.3 = false; // multi-finger → no tap
        }
    }
    pub fn moved(&mut self, id: PointerId, pos: [f32; 2], slop: f32) {
        if let Some(s) = &mut self.start
            && s.0 == id
            && dist(s.1, pos) > slop
        {
            s.3 = false;
        }
    }
    /// Returns the tap position if this release completes a tap.
    pub fn up(&mut self, id: PointerId, now: f64, max_time: f64) -> Option<[f32; 2]> {
        match self.start {
            Some((sid, p, t, ok)) if sid == id => {
                self.start = None;
                (ok && now - t <= max_time).then_some(p)
            }
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use Action::*;
    use PointerKind::*;

    const A: PointerId = PointerId(1);
    const B: PointerId = PointerId(2);
    const C: PointerId = PointerId(3);

    fn markup(actions: &[Action]) -> Vec<&Action> {
        actions.iter().filter(|a| matches!(a, Begin { .. } | Move { .. } | End { .. } | Tap { .. })).collect()
    }

    /// Simulates a log and returns "net markup": strokes that ended (not cancelled) + taps.
    fn committed(actions: &[Action]) -> (usize, usize) {
        let (mut strokes, mut taps, mut open) = (0, 0, false);
        for a in actions {
            match a {
                Begin { .. } => {
                    assert!(!open, "Begin while a stroke is open");
                    open = true
                }
                Move { .. } => assert!(open, "Move outside a stroke"),
                End { .. } => {
                    assert!(open, "End outside a stroke");
                    open = false;
                    strokes += 1
                }
                Cancel => open = false,
                Tap { .. } => taps += 1,
                _ => {}
            }
        }
        assert!(!open, "stroke left open");
        (strokes, taps)
    }

    #[test]
    fn single_finger_stroke_is_drawn_from_its_first_point() {
        let mut g = GestureArbiter::default();
        let mut log = g.down(A, Touch, [10.0, 10.0], 0.0, Intent::Draw);
        assert!(log.is_empty(), "nothing is drawn before the gesture is decided");
        log.extend(g.moved(A, [12.0, 11.0], 0.01));
        log.extend(g.moved(A, [30.0, 20.0], 0.02));
        assert_eq!(log[0], Begin { pos: [10.0, 10.0], kind: Touch });
        log.extend(g.moved(A, [40.0, 25.0], 0.03));
        log.extend(g.up(A, [40.0, 25.0], 0.04));
        assert_eq!(committed(&log), (1, 0));
        assert_eq!(log.iter().filter(|a| matches!(a, Move { .. })).count(), 3);
    }

    #[test]
    fn pinch_with_drawing_tool_leaves_no_markup() {
        let mut g = GestureArbiter::default();
        let mut log = g.down(A, Touch, [100.0, 100.0], 0.0, Intent::Draw);
        log.extend(g.moved(A, [101.0, 101.0], 0.01));
        log.extend(g.down(B, Touch, [200.0, 200.0], 0.03, Intent::Draw));
        for i in 0..20 {
            let d = i as f32 * 5.0;
            log.extend(g.moved(A, [100.0 - d, 100.0 - d], 0.04 + i as f64 * 0.016));
            log.extend(g.moved(B, [200.0 + d, 200.0 + d], 0.045 + i as f64 * 0.016));
        }
        assert!(log.iter().any(|a| matches!(a, Navigate { zoom, .. } if *zoom > 1.0)));
        log.extend(g.up(A, [5.0, 5.0], 0.5));
        // The remaining finger keeps moving: must NOT start a stroke
        log.extend(g.moved(B, [400.0, 300.0], 0.55));
        log.extend(g.moved(B, [420.0, 330.0], 0.6));
        log.extend(g.up(B, [420.0, 330.0], 0.65));
        assert!(markup(&log).is_empty(), "pinch produced markup: {log:?}");
        assert_eq!(committed(&log), (0, 0));
        assert!(matches!(log.last(), Some(NavigateEnd { .. })));
    }

    #[test]
    fn second_finger_after_commit_cancels_stroke() {
        let mut g = GestureArbiter::default();
        let mut log = g.down(A, Touch, [0.0, 0.0], 0.0, Intent::Draw);
        log.extend(g.moved(A, [50.0, 0.0], 0.05));
        assert!(g.is_drawing());
        log.extend(g.down(B, Touch, [100.0, 100.0], 0.2, Intent::Draw));
        assert_eq!(log.last(), Some(&Cancel));
        log.extend(g.moved(A, [60.0, 0.0], 0.25));
        log.extend(g.up(B, [100.0, 100.0], 0.3));
        log.extend(g.moved(A, [90.0, 0.0], 0.35));
        log.extend(g.up(A, [90.0, 0.0], 0.4));
        assert_eq!(committed(&log), (0, 0));
    }

    #[test]
    fn simultaneous_two_finger_tap_with_note_tool_places_nothing() {
        let mut g = GestureArbiter::default();
        let mut log = g.down(A, Touch, [10.0, 10.0], 0.0, Intent::Tap);
        log.extend(g.down(B, Touch, [80.0, 10.0], 0.005, Intent::Tap));
        log.extend(g.up(A, [10.0, 10.0], 0.08));
        log.extend(g.up(B, [80.0, 10.0], 0.09));
        assert_eq!(committed(&log), (0, 0));
    }

    #[test]
    fn three_finger_gestures_and_finger_juggling() {
        let mut g = GestureArbiter::default();
        let mut log = g.down(A, Touch, [0.0, 0.0], 0.0, Intent::Draw);
        log.extend(g.down(B, Touch, [50.0, 0.0], 0.01, Intent::Draw));
        log.extend(g.down(C, Touch, [25.0, 50.0], 0.02, Intent::Draw));
        log.extend(g.up(A, [0.0, 0.0], 0.1));
        log.extend(g.down(A, Touch, [0.0, 0.0], 0.15, Intent::Draw)); // re-lands
        log.extend(g.up(B, [0.0, 0.0], 0.2));
        log.extend(g.up(C, [0.0, 0.0], 0.25));
        log.extend(g.moved(A, [100.0, 100.0], 0.3));
        log.extend(g.up(A, [100.0, 100.0], 0.35));
        assert_eq!(committed(&log), (0, 0));
        // next gesture works normally again
        let mut log = g.down(A, Touch, [0.0, 0.0], 1.0, Intent::Draw);
        log.extend(g.moved(A, [20.0, 0.0], 1.05));
        log.extend(g.up(A, [20.0, 0.0], 1.1));
        assert_eq!(committed(&log), (1, 0));
    }

    #[test]
    fn tap_tool_places_on_tap_and_scrolls_on_drag() {
        let mut g = GestureArbiter::default();
        let mut log = g.down(A, Touch, [10.0, 10.0], 0.0, Intent::Tap);
        log.extend(g.up(A, [11.0, 10.0], 0.1));
        assert_eq!(log, vec![Tap { pos: [10.0, 10.0], kind: Touch }]);

        let mut log = g.down(A, Touch, [10.0, 10.0], 1.0, Intent::Tap);
        log.extend(g.moved(A, [10.0, 60.0], 1.05));
        log.extend(g.moved(A, [10.0, 90.0], 1.1));
        log.extend(g.up(A, [10.0, 90.0], 1.15));
        assert_eq!(committed(&log), (0, 0));
        let panned: f32 = log.iter().filter_map(|a| if let Navigate { pan, .. } = a { Some(pan[1]) } else { None }).sum();
        assert!((panned - 80.0).abs() < 0.01, "panned {panned}");
    }

    #[test]
    fn quick_dot_is_kept() {
        let mut g = GestureArbiter::default();
        let mut log = g.down(A, Touch, [10.0, 10.0], 0.0, Intent::Draw);
        log.extend(g.up(A, [10.0, 10.0], 0.05));
        assert_eq!(committed(&log), (1, 0));
    }

    #[test]
    fn hold_commits_without_movement() {
        let mut g = GestureArbiter::default();
        let mut log = g.down(A, Touch, [10.0, 10.0], 0.0, Intent::Draw);
        assert!(g.tick(0.1).is_empty());
        log.extend(g.tick(0.4));
        assert!(g.is_drawing());
        log.extend(g.moved(A, [11.0, 10.0], 0.45));
        log.extend(g.up(A, [11.0, 10.0], 0.5));
        assert_eq!(committed(&log), (1, 0));
    }

    #[test]
    fn palm_is_ignored_while_pen_draws() {
        let mut g = GestureArbiter::default();
        let mut log = g.down(A, Pen, [10.0, 10.0], 0.0, Intent::Draw);
        assert_eq!(log, vec![Begin { pos: [10.0, 10.0], kind: Pen }]);
        log.extend(g.down(B, Touch, [300.0, 300.0], 0.05, Intent::Draw)); // palm
        log.extend(g.moved(B, [310.0, 320.0], 0.06));
        log.extend(g.moved(A, [50.0, 10.0], 0.07));
        log.extend(g.up(B, [310.0, 320.0], 0.08));
        log.extend(g.up(A, [50.0, 10.0], 0.1));
        assert_eq!(committed(&log), (1, 0));
        assert!(!log.iter().any(|a| matches!(a, Navigate { .. } | Cancel)));
    }

    #[test]
    fn pen_landing_cancels_finger_stroke() {
        let mut g = GestureArbiter::default();
        let mut log = g.down(A, Touch, [0.0, 0.0], 0.0, Intent::Draw);
        log.extend(g.moved(A, [40.0, 0.0], 0.05));
        log.extend(g.down(B, Pen, [5.0, 5.0], 0.1, Intent::Draw));
        log.extend(g.moved(A, [80.0, 0.0], 0.12)); // palm keeps moving: ignored
        log.extend(g.moved(B, [25.0, 5.0], 0.15));
        log.extend(g.up(B, [25.0, 5.0], 0.2));
        log.extend(g.up(A, [80.0, 0.0], 0.25));
        assert_eq!(committed(&log), (1, 0));
    }

    #[test]
    fn stylus_mode_fingers_only_navigate() {
        let mut g = GestureArbiter::new(Config { pen_only: true, ..Config::default() });
        let mut log = g.down(A, Touch, [0.0, 0.0], 0.0, Intent::Draw);
        log.extend(g.moved(A, [0.0, 50.0], 0.05));
        log.extend(g.up(A, [0.0, 50.0], 0.1));
        assert_eq!(committed(&log), (0, 0));
        assert!(log.iter().any(|a| matches!(a, Navigate { .. })));
    }

    #[test]
    fn pinch_zoom_factor_and_no_jump_on_finger_change() {
        let mut g = GestureArbiter::default();
        g.down(A, Touch, [100.0, 100.0], 0.0, Intent::Navigate);
        g.down(B, Touch, [200.0, 100.0], 0.0, Intent::Navigate);
        let a = g.moved(B, [300.0, 100.0], 0.016);
        let Navigate { zoom, .. } = a[0] else { panic!() };
        assert!((zoom - 2.0).abs() < 1e-4, "{zoom}");
        // Lifting one finger must not produce a pan jump to the remaining finger
        let r = g.up(B, [300.0, 100.0], 0.03);
        assert!(r.is_empty());
        let m = g.moved(A, [101.0, 100.0], 0.05);
        assert_eq!(m, vec![Navigate { pan: [1.0, 0.0], zoom: 1.0, center: [101.0, 100.0] }]);
    }

    #[test]
    fn mouse_draws_immediately() {
        let mut g = GestureArbiter::default();
        let mut log = g.down(A, Mouse, [1.0, 1.0], 0.0, Intent::Draw);
        log.extend(g.moved(A, [2.0, 1.0], 0.01));
        log.extend(g.up(A, [2.0, 1.0], 0.02));
        assert_eq!(log.len(), 3);
        assert_eq!(committed(&log), (1, 0));
    }

    #[test]
    fn os_cancel_never_commits_a_pending_tap() {
        let mut g = GestureArbiter::default();
        let mut log = g.down(A, Touch, [0.0, 0.0], 0.0, Intent::Tap);
        log.extend(g.cancel(A, 0.05));
        assert!(log.is_empty());
        let mut log = g.down(A, Touch, [0.0, 0.0], 1.0, Intent::Draw);
        log.extend(g.moved(A, [40.0, 0.0], 1.05));
        log.extend(g.cancel(A, 1.1));
        assert_eq!(log.last(), Some(&Cancel));
        assert_eq!(committed(&log), (0, 0));
    }

    #[test]
    fn randomized_multitouch_never_leaves_open_or_ghost_markup() {
        // Fuzz: any interleaving of 1-3 fingers. Invariants: protocol is well-formed, and any
        // gesture that ever had 2+ fingers down at once produces no committed markup.
        let mut seed: u64 = 0x1234_5678;
        let mut rnd = |n: u64| {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed % n
        };
        for round in 0..3000 {
            let mut g = GestureArbiter::default();
            let intent = [Intent::Draw, Intent::Tap, Intent::Navigate][rnd(3) as usize];
            let mut down: Vec<PointerId> = vec![];
            let mut log = vec![];
            let mut max_down = 0;
            let mut t = 0.0;
            let mut started = false;
            for _ in 0..40 {
                t += 0.01 * (1 + rnd(5)) as f64;
                let pos = [rnd(500) as f32, rnd(500) as f32];
                match rnd(4) {
                    0 if down.len() < 3 && (!started || !down.is_empty()) => {
                        let id = PointerId(10 + rnd(1000));
                        if !down.contains(&id) {
                            down.push(id);
                            started = true;
                            log.extend(g.down(id, Touch, pos, t, intent));
                        }
                    }
                    1 if !down.is_empty() => {
                        let i = rnd(down.len() as u64) as usize;
                        let id = down.remove(i);
                        log.extend(g.up(id, pos, t));
                    }
                    _ if !down.is_empty() => {
                        let id = down[rnd(down.len() as u64) as usize];
                        log.extend(g.moved(id, pos, t));
                        log.extend(g.tick(t));
                    }
                    _ => {}
                }
                max_down = max_down.max(down.len());
                if started && down.is_empty() {
                    break;
                }
            }
            for id in down.drain(..) {
                t += 0.01;
                log.extend(g.up(id, [0.0, 0.0], t));
            }
            let (strokes, taps) = committed(&log);
            if max_down >= 2 {
                assert_eq!((strokes, taps), (0, 0), "round {round}: multi-touch produced markup {log:?}");
            }
            assert!(g.is_idle());
        }
    }
}
