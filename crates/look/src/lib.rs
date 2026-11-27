//! Clean-room MIT OR Apache-2.0 look crate for Fogbreak Solid:
//! bundled OFL IBM Plex fonts, MIT/Apache-2.0 icon artwork, Tidal Strata built-in themes,
//! and clean geometric Fogbreak brand mark.

pub fn install_tables() {
    solidcraft_ui_kit::icons::install(&icons::SET);
    solidcraft_ui_kit::theme_files::install_builtin(themes::BUILTIN);
}

pub fn install() {
    install_tables();
}

pub mod icons {
    include!(concat!(env!("OUT_DIR"), "/icon_data.rs"));

    pub static SET: solidcraft_ui_kit::icons::IconSet = solidcraft_ui_kit::icons::IconSet::new(DATA);
}

pub mod fonts {
    use std::sync::Arc;

    use egui::{FontDefinitions, FontFamily};
    use solidcraft_ui_kit::type_face::{BOLD_FAMILY, MEDIUM_FAMILY, SEMIBOLD_FAMILY, sans_families, semibold};

    pub const PLEX_SANS: &[u8] =
        include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/fonts/ibm-plex-sans/IBMPlexSans-Variable.ttf"));
    pub const PLEX_MONO: &[u8] =
        include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/fonts/ibm-plex-mono/IBMPlexMono-Regular.ttf"));
    pub const WEIGHTS: [(&str, f32); 4] = [
        ("plex-sans", 400.0),
        (MEDIUM_FAMILY, 500.0),
        (SEMIBOLD_FAMILY, 600.0),
        (BOLD_FAMILY, 700.0),
    ];

    pub fn with_symbol_fallback(mut fonts: FontDefinitions) -> FontDefinitions {
        let defaults = FontDefinitions::default();
        let default_prop = defaults.families.get(&FontFamily::Proportional).cloned().unwrap_or_default();
        let default_mono = defaults.families.get(&FontFamily::Monospace).cloned().unwrap_or_default();
        for (name, data) in defaults.font_data {
            fonts.font_data.entry(name).or_insert(data);
        }
        for family in sans_families() {
            let list = fonts.families.entry(family).or_default();
            for entry in &default_prop {
                if !list.contains(entry) {
                    list.push(entry.clone());
                }
            }
        }
        let mono = fonts.families.entry(FontFamily::Monospace).or_default();
        for entry in &default_mono {
            if !mono.contains(entry) {
                mono.push(entry.clone());
            }
        }
        fonts
    }

    pub fn definitions() -> FontDefinitions {
        let mut fonts = FontDefinitions::default();
        let fallbacks = fonts.families.get(&FontFamily::Proportional).cloned().unwrap_or_default();
        for ((name, weight), family) in WEIGHTS.into_iter().zip(sans_families()) {
            let mut data = egui::FontData::from_static(PLEX_SANS);
            data.tweak.coords = egui::epaint::text::VariationCoords::new([(b"wght", weight)]);
            fonts.font_data.insert(name.to_owned(), Arc::new(data));
            fonts.families.insert(family, std::iter::once(name.to_owned()).chain(fallbacks.iter().cloned()).collect());
        }
        fonts.font_data.insert("plex-mono".to_owned(), Arc::new(egui::FontData::from_static(PLEX_MONO)));
        fonts.families.entry(FontFamily::Monospace).or_default().insert(0, "plex-mono".to_owned());
        with_symbol_fallback(fonts)
    }

    pub fn apply(ctx: &egui::Context, fonts: FontDefinitions) {
        ctx.set_fonts(fonts);
        ctx.global_style_mut(|style| {
            if let Some(heading) = style.text_styles.get_mut(&egui::TextStyle::Heading) {
                heading.family = semibold(heading.size).family;
            }
        });
    }
}

pub mod glyphs {
    use egui::{Pos2, Shape, Stroke, pos2};

    /// Clean-room geometric Fogbreak Tidal Strata layered-strata prism mark.
    pub fn brand_mark(p: &egui::Painter, c: Pos2) {
        let [top, mid, bot] = solidcraft_theme::Tokens::get().logo;
        let pts_top = vec![pos2(c.x, c.y - 8.0), pos2(c.x + 8.0, c.y - 3.5), pos2(c.x, c.y + 1.0), pos2(c.x - 8.0, c.y - 3.5)];
        let pts_left = vec![pos2(c.x - 8.0, c.y - 1.5), pos2(c.x, c.y + 3.0), pos2(c.x, c.y + 8.5), pos2(c.x - 8.0, c.y + 4.0)];
        let pts_right = vec![pos2(c.x + 8.0, c.y - 1.5), pos2(c.x, c.y + 3.0), pos2(c.x, c.y + 8.5), pos2(c.x + 8.0, c.y + 4.0)];
        p.add(Shape::convex_polygon(pts_top, top, Stroke::NONE));
        p.add(Shape::convex_polygon(pts_left, mid, Stroke::NONE));
        p.add(Shape::convex_polygon(pts_right, bot, Stroke::NONE));
    }
}

pub mod pointer {
    use egui::{Color32, FontId, Pos2, Shape, Stroke, vec2};

    pub fn agent() -> Color32 {
        solidcraft_theme::Tokens::get().icons.agent
    }

    pub fn paint(painter: &egui::Painter, p: Pos2) {
        let ic = solidcraft_theme::Tokens::get().icons;
        let pts: Vec<Pos2> = [(0.0, 0.0), (0.0, 17.0), (4.5, 13.0), (7.5, 20.0), (10.5, 18.8), (7.5, 12.0), (13.0, 12.0)]
            .iter()
            .map(|(x, y)| p + vec2(*x, *y))
            .collect();
        painter.add(Shape::convex_polygon(pts.clone(), ic.agent, Stroke::NONE));
        painter.add(Shape::closed_line(pts, Stroke::new(1.5, ic.highlight)));
        let label = painter.layout_no_wrap("Agent".into(), FontId::proportional(solidcraft_theme::Tokens::get().font_small), ic.highlight);
        let r = egui::Rect::from_min_size(p + vec2(15.0, 18.0), label.size() + vec2(10.0, 4.0));
        painter.rect_filled(r, 4.0, ic.agent);
        painter.galley(r.min + vec2(5.0, 2.0), label, ic.highlight);
    }
}

pub mod compass {
    use egui::{Color32, FontId, Painter, Pos2};
    use solidcraft_theme::Tokens;

    #[allow(clippy::too_many_arguments)]
    pub fn axis_tip_scaled(
        painter: &Painter,
        center: Pos2,
        name: &str,
        _dir: [f64; 3],
        at: Pos2,
        _depth: f64,
        hovered: bool,
        tokens: Tokens,
        scale: f32,
    ) {
        let c = match name {
            "X" | "+X" | "-X" => tokens.axis_x,
            "Y" | "+Y" | "-Y" => tokens.axis_y,
            _ => tokens.axis_z,
        };
        let r = (if hovered { 9.5 } else { 7.5 }) * scale.max(0.5);
        painter.line_segment([center, at], egui::Stroke::new(2.0 * scale.max(0.5), c));
        painter.circle_filled(at, r, c);
        painter.text(at, egui::Align2::CENTER_CENTER, name, FontId::proportional(9.5 * scale.max(0.5)), Color32::WHITE);
    }
}

pub mod themes {
    /// Sorted by id, formatted for `solidcraft_ui_kit::theme_files`.
    pub const BUILTIN: &[(&str, &str)] = &[
        (
            "dark",
            r##"{
  "format": "solidcraft-theme/1",
  "name": "Fogbreak Dark",
  "base": "dark",
  "order": 1,
  "provenance": "Fogbreak Tidal Strata",
  "colors": {
    "window": "#111821",
    "panel": "#161F2B",
    "panel_header": "#1D2938",
    "toolbar": "#161F2B",
    "app_bar": "#0D131B",
    "accent": "#087F9C",
    "text": "#F7FAFC",
    "border": "#2F3F52"
  }
}"##,
        ),
        (
            "fogbreak-dark",
            r##"{
  "format": "solidcraft-theme/1",
  "name": "Fogbreak Dark",
  "base": "dark",
  "order": 2,
  "provenance": "Fogbreak Tidal Strata",
  "colors": {
    "window": "#111821",
    "panel": "#161F2B",
    "panel_header": "#1D2938",
    "toolbar": "#161F2B",
    "app_bar": "#0D131B",
    "accent": "#087F9C",
    "text": "#F7FAFC",
    "border": "#2F3F52"
  }
}"##,
        ),
        (
            "fogbreak-light",
            r##"{
  "format": "solidcraft-theme/1",
  "name": "Fogbreak Light",
  "base": "light",
  "order": 3,
  "provenance": "Fogbreak Tidal Strata",
  "colors": {
    "window": "#F7FAFC",
    "panel": "#F7FAFC",
    "panel_header": "#EDF3F7",
    "toolbar": "#F7FAFC",
    "app_bar": "#EDF3F7",
    "accent": "#087F9C",
    "text": "#111821",
    "border": "#D1DBE4"
  }
}"##,
        ),
        (
            "light",
            r##"{
  "format": "solidcraft-theme/1",
  "name": "Fogbreak Light",
  "base": "light",
  "order": 0,
  "provenance": "Fogbreak Tidal Strata",
  "colors": {
    "window": "#F7FAFC",
    "panel": "#F7FAFC",
    "panel_header": "#EDF3F7",
    "toolbar": "#F7FAFC",
    "app_bar": "#EDF3F7",
    "accent": "#087F9C",
    "text": "#111821",
    "border": "#D1DBE4"
  }
}"##,
        ),
    ];
}
