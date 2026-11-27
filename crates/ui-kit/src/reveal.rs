//! Clean-room edge reveal state for auto-hiding ribbons and toolbars.

/// Inputs sampled on a frame to advance [`reveal_step`].
#[derive(Clone, Copy, Debug, Default)]
pub struct Input {
    pub hovered: bool,
    pub focused: bool,
    pub held: bool,
    pub keyboard_show: bool,
    pub escape: bool,
    pub dismiss_on_escape: bool,
    pub reduced_motion: bool,
    pub show_delay: Option<f64>,
    pub hide_delay: Option<f64>,
}

/// Persistent reveal transition state.
#[derive(Clone, Copy, Debug, Default)]
pub struct State {
    shown: bool,
    keyboard: bool,
    pending_show_since: Option<f64>,
    pending_hide_since: Option<f64>,
    anim_from: f32,
    anim_to: f32,
    anim_start: f64,
    anim_duration: f64,
}

impl State {
    /// Whether the reveal was opened via keyboard shortcut.
    pub fn keyboard(self) -> bool {
        self.keyboard
    }

    /// Clear keyboard-latch state when focus leaves the bar.
    pub fn release_keyboard(&mut self) {
        self.keyboard = false;
    }

    /// Current reveal fraction in `0.0..=1.0`.
    pub fn fraction(self, now: f64) -> f32 {
        if self.anim_duration <= 0.0 || now >= self.anim_start + self.anim_duration {
            return if self.shown { 1.0 } else { self.anim_to };
        }
        let u = ((now - self.anim_start) / self.anim_duration).clamp(0.0, 1.0) as f32;
        egui::lerp(self.anim_from..=self.anim_to, u)
    }

    /// Delay until next animation or timer step, if any.
    pub fn repaint_after(self, now: f64) -> Option<f64> {
        if self.anim_duration > 0.0 && now < self.anim_start + self.anim_duration {
            return Some(0.016);
        }
        if let Some(since) = self.pending_hide_since {
            return Some((0.02_f64).max(0.15 - (now - since).max(0.0)));
        }
        None
    }
}

/// Advance `state` for `input` at frame timestamp `now` (seconds).
pub fn reveal_step(mut state: State, input: Input, now: f64) -> State {
    let cur = state.fraction(now);
    if input.escape {
        state.shown = false;
        state.keyboard = false;
        state.pending_show_since = None;
        state.pending_hide_since = None;
        state.anim_from = cur;
        state.anim_to = 0.0;
        state.anim_start = now;
        state.anim_duration = if input.reduced_motion { 0.0 } else { 0.12 };
        return state;
    }
    if input.keyboard_show {
        state.keyboard = true;
    }
    let want = input.hovered || input.held || state.keyboard || (input.focused && !input.dismiss_on_escape);
    if want {
        state.pending_hide_since = None;
        let delay = input.show_delay.unwrap_or(0.0);
        if delay > 0.0 && !state.shown {
            let since = *state.pending_show_since.get_or_insert(now);
            if now - since < delay {
                return state;
            }
        }
        if !state.shown {
            state.shown = true;
            state.anim_from = cur;
            state.anim_to = 1.0;
            state.anim_start = now;
            state.anim_duration = if input.reduced_motion { 0.0 } else { 0.12 };
        }
    } else {
        state.pending_show_since = None;
        if state.shown {
            let delay = input.hide_delay.unwrap_or(0.0);
            if delay > 0.0 {
                let since = *state.pending_hide_since.get_or_insert(now);
                if now - since < delay {
                    return state;
                }
            }
            state.shown = false;
            state.keyboard = false;
            state.pending_hide_since = None;
            state.anim_from = cur;
            state.anim_to = 0.0;
            state.anim_start = now;
            state.anim_duration = if input.reduced_motion { 0.0 } else { 0.14 };
        }
    }
    if !input.focused && !input.held {
        state.keyboard = false;
    }
    state
}
