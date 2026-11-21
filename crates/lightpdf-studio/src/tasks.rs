//! Background work (saving, merging, compressing …) without freezing the UI.

use crate::app::Studio;
use eframe::egui;
use std::sync::mpsc::{Receiver, Sender, channel};

/// What a finished task does to the app (runs on the UI thread).
pub type Apply = Box<dyn FnOnce(&mut Studio) + Send>;

enum Msg {
    Progress(f32),
    Done(Apply),
}

pub struct Task {
    pub label: String,
    pub progress: Option<f32>,
    rx: Receiver<Msg>,
}

#[derive(Default)]
pub struct Tasks {
    list: Vec<Task>,
}

/// Handed to the worker to report progress (0..1).
pub struct Progress {
    tx: Sender<Msg>,
    ctx: egui::Context,
}

impl Progress {
    pub fn set(&self, done: usize, total: usize) {
        let _ = self.tx.send(Msg::Progress(done as f32 / total.max(1) as f32));
        self.ctx.request_repaint();
    }
}

impl Tasks {
    pub fn spawn(&mut self, ctx: &egui::Context, label: impl Into<String>, work: impl FnOnce(&Progress) -> Apply + Send + 'static) {
        let (tx, rx) = channel();
        let p = Progress { tx: tx.clone(), ctx: ctx.clone() };
        let ctx2 = ctx.clone();
        std::thread::Builder::new()
            .name("lightpdf-task".into())
            .stack_size(16 << 20)
            .spawn(move || {
                let apply = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| work(&p))).unwrap_or_else(|_| {
                    Box::new(|s: &mut Studio| s.toast_err(crate::i18n::tr("Ein unerwarteter Fehler ist aufgetreten.")))
                });
                let _ = tx.send(Msg::Done(apply));
                ctx2.request_repaint();
            })
            .expect("spawn task");
        self.list.push(Task { label: label.into(), progress: None, rx });
    }

    pub fn busy(&self) -> Option<&Task> {
        self.list.first()
    }

    /// Collects finished tasks; returns their result closures.
    pub fn poll(&mut self) -> Vec<Apply> {
        let mut out = Vec::new();
        self.list.retain_mut(|t| {
            let mut keep = true;
            while let Ok(m) = t.rx.try_recv() {
                match m {
                    Msg::Progress(p) => t.progress = Some(p),
                    Msg::Done(a) => {
                        out.push(a);
                        keep = false;
                    }
                }
            }
            keep
        });
        out
    }
}
