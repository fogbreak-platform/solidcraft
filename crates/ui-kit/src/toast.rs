//! Clean-room viewport toast cards and action buttons.

use egui::{Align2, Color32, FontId, Pos2, Rect, Sense, Stroke, StrokeKind, Ui, pos2, vec2};
use solidcraft_theme::Tokens;

/// Fixed corner column width leaving room for three action buttons on one row.
pub const WIDTH: f32 = 340.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Primary,
    Secondary,
    Dismiss,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lead {
    Dot(bool),
    Spinner,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Place {
    #[default]
    Corner,
    Centre,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Extras<'a> {
    pub close: Option<&'a str>,
    pub countdown: Option<(f32, f64)>,
    pub tips: &'a [&'a str],
    pub card_handle: Option<&'a str>,
    pub card_hint: Option<&'a str>,
}

#[derive(Clone, Debug)]
pub struct Shown {
    pub rect: Rect,
    pub clicked: Option<usize>,
    pub buttons: Vec<Rect>,
    pub close: Option<Rect>,
    pub closed: bool,
    pub card_clicked: bool,
}

impl Default for Shown {
    fn default() -> Self {
        Self {
            rect: Rect::NOTHING,
            clicked: None,
            buttons: Vec::new(),
            close: None,
            closed: false,
            card_clicked: false,
        }
    }
}

/// Fraction of `total` seconds remaining from `since` at `now`, in `0.0..=1.0`.
pub fn countdown_left(since: f64, now: f64, total: f64) -> f32 {
    if total <= 0.0 || !since.is_finite() || !now.is_finite() {
        return 0.0;
    }
    let elapsed = (now - since).max(0.0);
    (1.0 - (elapsed / total) as f32).clamp(0.0, 1.0)
}

pub fn countdown_arc(painter: &egui::Painter, center: Pos2, radius: f32, fraction: f32, color: Color32) {
    let f = fraction.clamp(0.0, 1.0);
    if f <= 0.0 {
        return;
    }
    let steps = 24usize;
    let start = -std::f32::consts::FRAC_PI_2;
    let span = std::f32::consts::TAU * f;
    let pts: Vec<Pos2> = (0..=steps)
        .map(|i| {
            let a = start + span * (i as f32 / steps as f32);
            pos2(center.x + radius * a.cos(), center.y + radius * a.sin())
        })
        .collect();
    painter.add(egui::Shape::line(pts, Stroke::new(1.5, color)));
}

pub fn action_spots(free: Rect, count: usize) -> Vec<Rect> {
    let n = count.max(1) as f32;
    let w = ((free.width() - 24.0) / n).clamp(64.0, 140.0);
    (0..count)
        .map(|i| {
            let x = free.left() + 12.0 + i as f32 * (w + 6.0);
            Rect::from_min_size(pos2(x, free.bottom() - 32.0), vec2(w, 24.0))
        })
        .collect()
}

pub fn action_spots_trailing(free: Rect, count: usize) -> Vec<Rect> {
    action_spots(free, count)
}

/// Paint a non-modal toast inside `free`.
pub fn show(
    ui: &Ui,
    free: Rect,
    place: Place,
    text: String,
    lead: Lead,
    actions: &[(&str, Action)],
    extras: Extras<'_>,
) -> Option<Shown> {
    if !free.is_positive() || free.width() < 80.0 || free.height() < 36.0 {
        return None;
    }
    let t = Tokens::get();
    let pad = t.space_10;
    let width = free.width().min(WIDTH);
    let galley = ui.painter().layout(text, FontId::proportional(t.font_body), t.text, (width - 48.0).max(60.0));
    let has_actions = !actions.is_empty();
    let h = (pad * 2.0 + galley.size().y.max(18.0) + if has_actions { 30.0 } else { 0.0 }).min(free.height());
    let rect = match place {
        Place::Corner => Rect::from_min_size(pos2(free.right() - width, free.bottom() - h), vec2(width, h)),
        Place::Centre => Rect::from_center_size(pos2(free.center().x, free.bottom() - h * 0.5), vec2(width, h)),
    };
    let p = ui.painter();
    p.rect(rect, t.radius_lg, t.dialog_body, Stroke::new(1.0, t.border), StrokeKind::Inside);

    let lead_pos = pos2(rect.left() + pad + 6.0, rect.top() + pad + 9.0);
    match lead {
        Lead::Dot(err) => {
            let c = if err { t.error } else { t.accent };
            p.circle_filled(lead_pos, 4.0, c);
        }
        Lead::Spinner => {
            p.circle_stroke(lead_pos, 5.0, Stroke::new(1.5, t.accent));
        }
    }
    p.galley(pos2(rect.left() + pad + 18.0, rect.top() + pad), galley, t.text);

    let mut close_rect = None;
    let mut closed = false;
    if extras.close.is_some() {
        let cr = Rect::from_center_size(pos2(rect.right() - pad - 8.0, rect.top() + pad + 8.0), vec2(18.0, 18.0));
        close_rect = Some(cr);
        if let Some((rem, _total)) = extras.countdown {
            countdown_arc(p, cr.center(), 8.0, rem.clamp(0.0, 1.0), t.accent);
        }
        p.text(cr.center(), Align2::CENTER_CENTER, "×", FontId::proportional(t.font_label), t.text_dim);
        let resp = ui.interact(cr, ui.scope_id().with(("toast_close", extras.close)), Sense::click());
        if resp.clicked() {
            closed = true;
        }
    }

    let mut buttons = Vec::with_capacity(actions.len());
    let mut clicked = None;
    if has_actions {
        let mut bx = rect.left() + pad + 18.0;
        let by = rect.bottom() - pad - 22.0;
        for (idx, &(label, kind)) in actions.iter().enumerate() {
            let bg = ui.painter().layout_no_wrap(label.to_owned(), FontId::proportional(t.font_small), t.text);
            let bw = (bg.size().x + 16.0).min(rect.right() - bx - pad).max(40.0);
            let br = Rect::from_min_size(pos2(bx, by), vec2(bw, 22.0));
            let (fill, ink, stroke) = match kind {
                Action::Primary => (t.accent, t.on_accent, Stroke::NONE),
                Action::Secondary | Action::Dismiss => (t.control_fill, t.text, Stroke::new(1.0, t.border)),
            };
            p.rect(br, t.radius_sm, fill, stroke, StrokeKind::Inside);
            p.galley(pos2(br.center().x - bg.size().x * 0.5, br.center().y - bg.size().y * 0.5), bg, ink);
            let resp = ui.interact(br, ui.scope_id().with(("toast_btn", idx, label)), Sense::click());
            if let Some(tip) = extras.tips.get(idx).filter(|s| !s.is_empty()) {
                let _ = resp.clone().on_hover_text(*tip);
            }
            if resp.clicked() {
                clicked = Some(idx);
            }
            buttons.push(br);
            bx += bw + 6.0;
        }
    }

    let mut card_clicked = false;
    if extras.card_handle.is_some() || extras.card_hint.is_some() {
        let card_resp = ui.interact(rect, ui.scope_id().with("toast_card"), Sense::click());
        if let Some(hint) = extras.card_hint.filter(|s| !s.is_empty()) {
            let _ = card_resp.clone().on_hover_text(hint);
        }
        if let Some(handle) = extras.card_handle {
            solidcraft_ui_registry::publish_rect(handle, rect);
        }
        if card_resp.clicked() && clicked.is_none() && !closed {
            card_clicked = true;
        }
    }

    Some(Shown { rect, clicked, buttons, close: close_rect, closed, card_clicked })
}
