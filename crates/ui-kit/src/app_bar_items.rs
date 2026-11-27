//! Clean-room customizable application bar layout model.

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Zone {
    Start,
    End,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Preset {
    Default,
    Minimal,
    Full,
    Compact,
    Custom,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DragRelease {
    Insert,
    Remove,
    Return,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DropSlot {
    pub zone: Zone,
    pub before: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Placement {
    pub id: String,
    pub zone: Zone,
    #[serde(default = "default_shown")]
    pub shown: bool,
}

fn default_shown() -> bool {
    true
}

#[derive(Clone, Debug, PartialEq)]
pub struct EditItem {
    pub id: String,
    pub zone: Zone,
    pub left: f32,
    pub right: f32,
    pub locked: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Layout {
    pub items: Vec<Placement>,
}

const LOCKED_IDS: &[&str] = &["brand", "tabs", "file", "home", "report", "settings", "help"];

pub fn can_remove(id: &str) -> bool {
    !LOCKED_IDS.contains(&id)
}

pub fn is_drag(dx: f32, dy: f32) -> bool {
    dx * dx + dy * dy >= 16.0
}

pub fn drag_out(pointer_y: f32, bar_bottom: f32) -> bool {
    pointer_y > bar_bottom + 24.0
}

pub fn drag_release(id: &str, from_bar: bool, below: bool, has_slot: bool, _reserved: bool) -> DragRelease {
    if has_slot {
        DragRelease::Insert
    } else if from_bar && below && can_remove(id) {
        DragRelease::Remove
    } else {
        DragRelease::Return
    }
}

pub fn drop_slot(pointer_x: f32, tabs_centre: f32, items: &[EditItem], self_id: &str) -> Option<DropSlot> {
    let zone = if pointer_x < tabs_centre { Zone::Start } else { Zone::End };
    let before = items
        .iter()
        .filter(|p| p.zone == zone && p.id != self_id)
        .find(|p| pointer_x < (p.left + p.right) * 0.5)
        .map(|p| p.id.clone());
    Some(DropSlot { zone, before })
}

impl Layout {
    pub fn default_for<'a>(quick_access: impl IntoIterator<Item = &'a str>) -> Self {
        let mut items = vec![
            Placement { id: "documents".into(), zone: Zone::Start, shown: true },
            Placement { id: "home".into(), zone: Zone::Start, shown: true },
            Placement { id: "file".into(), zone: Zone::Start, shown: true },
            Placement { id: "save".into(), zone: Zone::Start, shown: true },
            Placement { id: "undo".into(), zone: Zone::Start, shown: true },
            Placement { id: "redo".into(), zone: Zone::Start, shown: true },
        ];
        for id in quick_access {
            items.push(Placement { id: format!("quick:{id}"), zone: Zone::Start, shown: true });
        }
        items.extend([
            Placement { id: "search".into(), zone: Zone::End, shown: true },
            Placement { id: "assistant".into(), zone: Zone::End, shown: true },
            Placement { id: "report".into(), zone: Zone::End, shown: true },
            Placement { id: "help".into(), zone: Zone::End, shown: true },
            Placement { id: "settings".into(), zone: Zone::End, shown: true },
            Placement { id: "version".into(), zone: Zone::End, shown: true },
        ]);
        Self { items }
    }

    pub fn from_value(value: &Value, defaults: &Self) -> Self {
        let Ok(mut parsed) = serde_json::from_value::<Self>(value.clone()) else {
            return defaults.clone();
        };
        parsed.sanitize(defaults);
        parsed
    }

    pub fn sanitize(&mut self, defaults: &Self) {
        if self.items.is_empty() {
            *self = defaults.clone();
            return;
        }
        let mut seen = std::collections::BTreeSet::new();
        self.items.retain(|p| seen.insert(p.id.clone()));
        for d in &defaults.items {
            if !seen.contains(&d.id) {
                self.items.push(d.clone());
            }
        }
    }

    pub fn editable(&self, id: &str) -> bool {
        !LOCKED_IDS.contains(&id) && self.items.iter().any(|p| p.id == id)
    }

    pub fn set_shown(&mut self, id: &str, shown: bool, defaults: &Self) {
        self.sanitize(defaults);
        if !shown && !can_remove(id) {
            return;
        }
        if let Some(p) = self.items.iter_mut().find(|p| p.id == id) {
            p.shown = shown;
        }
    }

    pub fn add_object(&mut self, id: &str, zone: Zone, before: Option<&str>, defaults: &Self) {
        self.sanitize(defaults);
        let n = self.items.iter().filter(|p| p.id.starts_with(&format!("{id}:"))).count() + 1;
        let full = format!("{id}:{n}");
        self.items.push(Placement { id: full.clone(), zone, shown: true });
        self.move_before(&full, zone, before, defaults);
    }

    pub fn move_before(&mut self, id: &str, zone: Zone, before: Option<&str>, defaults: &Self) {
        self.sanitize(defaults);
        if before == Some(id) {
            return;
        }
        let Some(pos) = self.items.iter().position(|p| p.id == id) else { return };
        let mut item = self.items.remove(pos);
        item.zone = zone;
        let idx = before
            .and_then(|b| self.items.iter().position(|p| p.id == b))
            .unwrap_or_else(|| {
                self.items
                    .iter()
                    .rposition(|p| p.zone == zone)
                    .map_or(self.items.len(), |i| i + 1)
            });
        self.items.insert(idx.min(self.items.len()), item);
    }

    pub fn move_step(&mut self, id: &str, forward: bool, defaults: &Self) {
        self.sanitize(defaults);
        let Some(idx) = self.items.iter().position(|p| p.id == id) else { return };
        let zone = self.items[idx].zone;
        let zone_indices: Vec<usize> = self.items.iter().enumerate().filter(|(_, p)| p.zone == zone).map(|(i, _)| i).collect();
        let Some(zpos) = zone_indices.iter().position(|&i| i == idx) else { return };
        let target_zpos = if forward { zpos.saturating_add(1) } else { zpos.saturating_sub(1) };
        if target_zpos < zone_indices.len() && target_zpos != zpos {
            self.items.swap(idx, zone_indices[target_zpos]);
        }
    }

    pub fn set_preset(&mut self, preset: Preset, defaults: &Self) {
        *self = defaults.clone();
        match preset {
            Preset::Default | Preset::Custom => {}
            Preset::Minimal => {
                for p in &mut self.items {
                    if matches!(p.id.as_str(), "undo" | "redo" | "version") || p.id.starts_with("quick:") {
                        p.shown = false;
                    }
                }
            }
            Preset::Full => {
                for p in &mut self.items {
                    p.shown = true;
                }
            }
            Preset::Compact => {
                for p in &mut self.items {
                    if p.id == "version" {
                        p.shown = false;
                    }
                }
            }
        }
    }

    pub fn preset(&self, defaults: &Self) -> Preset {
        if self == defaults {
            Preset::Default
        } else {
            Preset::Custom
        }
    }
}
