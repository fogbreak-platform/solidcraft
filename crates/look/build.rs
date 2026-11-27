use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

fn esc(s: &str) -> String {
    format!("{s:?}")
}

fn hex_rgb(s: &str) -> [u8; 3] {
    let h = s.trim().trim_start_matches('#');
    let v = u32::from_str_radix(h, 16).unwrap_or(0);
    [((v >> 16) & 0xff) as u8, ((v >> 8) & 0xff) as u8, (v & 0xff) as u8]
}

fn collect_svgs(dir: &Path, family: &str, out: &mut Vec<(String, String, PathBuf)>) {
    let Ok(entries) = fs::read_dir(dir) else { return };
    let mut paths: Vec<PathBuf> = entries.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e == "svg")).collect();
    paths.sort();
    for p in paths {
        if let Some(stem) = p.file_stem().and_then(|s| s.to_str()) {
            let low = stem.to_ascii_lowercase();
            if matches!(low.as_str(), "kofi" | "discord" | "solidcraft" | "artcraft") {
                continue;
            }
            out.push((stem.to_owned(), family.to_owned(), p));
        }
    }
}

fn main() {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let design_dir = manifest_dir.join("../../assets/icons/ui/design");
    let palette_path = design_dir.join("palette.toml");
    let aliases_path = design_dir.join("aliases.toml");

    println!("cargo:rerun-if-changed={}", palette_path.display());
    println!("cargo:rerun-if-changed={}", aliases_path.display());

    let palette_src = fs::read_to_string(&palette_path).expect("read palette.toml");
    let palette_val: toml::Value = toml::from_str(&palette_src).expect("parse palette.toml");
    let aliases_src = fs::read_to_string(&aliases_path).expect("read aliases.toml");
    let aliases_val: toml::Value = toml::from_str(&aliases_src).expect("parse aliases.toml");

    let mut role_names = Vec::new();
    let mut role_source_hex = Vec::new();
    let mut light_pal = Vec::new();
    let mut dark_pal = Vec::new();
    let mut role_idx: BTreeMap<String, usize> = BTreeMap::new();

    if let Some(roles) = palette_val.get("roles").and_then(|v| v.as_table()) {
        for (name, val) in roles {
            let idx = role_names.len();
            role_idx.insert(name.clone(), idx);
            role_names.push(name.clone());
            role_source_hex.push(val.get("source").and_then(|v| v.as_str()).unwrap_or("#000000").to_ascii_lowercase());
            light_pal.push(hex_rgb(val.get("light").and_then(|v| v.as_str()).unwrap_or("#000000")));
            dark_pal.push(hex_rgb(val.get("dark").and_then(|v| v.as_str()).unwrap_or("#ffffff")));
        }
    }

    let mut accent_names = Vec::new();
    let mut accent_idx: BTreeMap<String, usize> = BTreeMap::new();
    let mut accents_vec: Vec<Vec<(usize, [u8; 3], [u8; 3])>> = Vec::new();

    if let Some(accents) = palette_val.get("accents").and_then(|v| v.as_table()) {
        for (aname, roles_tbl) in accents {
            let idx = accent_names.len();
            accent_idx.insert(aname.clone(), idx);
            accent_names.push(aname.clone());
            let mut entries = Vec::new();
            if let Some(tbl) = roles_tbl.as_table() {
                for (rname, val) in tbl {
                    if let Some(&ri) = role_idx.get(rname) {
                        let l = hex_rgb(val.get("light").and_then(|v| v.as_str()).unwrap_or("#000000"));
                        let d = hex_rgb(val.get("dark").and_then(|v| v.as_str()).unwrap_or("#ffffff"));
                        entries.push((ri, l, d));
                    }
                }
            }
            accents_vec.push(entries);
        }
    }

    let mut variants = Vec::new();
    if let Some(vtbl) = palette_val.get("variants").and_then(|v| v.as_table()) {
        for (vname, atbl) in vtbl {
            if let Some(atbl) = atbl.as_table() {
                for (aname, rtbl) in atbl {
                    if let (Some(&ai), Some(rtbl)) = (accent_idx.get(aname), rtbl.as_table()) {
                        for (rname, val) in rtbl {
                            if let Some(&ri) = role_idx.get(rname) {
                                let l = hex_rgb(val.get("light").and_then(|v| v.as_str()).unwrap_or("#000000"));
                                let d = hex_rgb(val.get("dark").and_then(|v| v.as_str()).unwrap_or("#ffffff"));
                                variants.push((vname.clone(), ai, ri, l, d));
                            }
                        }
                    }
                }
            }
        }
    }

    let mut icon_accents = Vec::new();
    if let Some(iatbl) = palette_val.get("icon-accents").and_then(|v| v.as_table()) {
        for (accent, arr) in iatbl {
            if let Some(items) = arr.as_array() {
                for item in items {
                    if let Some(id) = item.as_str() {
                        icon_accents.push((id.to_owned(), accent.clone()));
                    }
                }
            }
        }
    }

    let mut aliases = Vec::new();
    if let Some(atbl) = aliases_val.get("alias").and_then(|v| v.as_table()) {
        for (alias, val) in atbl {
            if let Some(master) = val.as_str() {
                aliases.push((alias.clone(), master.to_owned(), String::new()));
            } else if let Some(tbl) = val.as_table() {
                let master = tbl.get("master").and_then(|v| v.as_str()).unwrap_or("");
                let accent = tbl.get("accent").and_then(|v| v.as_str()).unwrap_or("");
                aliases.push((alias.clone(), master.to_owned(), accent.to_owned()));
            }
        }
    }

    let svg_root = design_dir.join("svg");
    let mut masters = Vec::new();
    collect_svgs(&svg_root.join("model"), "model", &mut masters);
    collect_svgs(&svg_root.join("workspace"), "workspace", &mut masters);
    collect_svgs(&svg_root.join("flat"), "flat", &mut masters);
    collect_svgs(&svg_root.join("small"), "small", &mut masters);
    for (_, _, p) in &masters {
        println!("cargo:rerun-if-changed={}", p.display());
    }

    let mut code = String::new();
    code.push_str("pub const ROLE_NAMES: &[&str] = &[\n");
    for r in &role_names {
        code.push_str(&format!("    {},\n", esc(r)));
    }
    code.push_str("];\n");

    code.push_str("const ROLE_SOURCE_HEX: &[&str] = &[\n");
    for h in &role_source_hex {
        code.push_str(&format!("    {},\n", esc(h)));
    }
    code.push_str("];\n");

    code.push_str("const LIGHT_PALETTE: &[[u8; 3]] = &[\n");
    for [r, g, b] in &light_pal {
        code.push_str(&format!("    [{r}, {g}, {b}],\n"));
    }
    code.push_str("];\n");

    code.push_str("const DARK_PALETTE: &[[u8; 3]] = &[\n");
    for [r, g, b] in &dark_pal {
        code.push_str(&format!("    [{r}, {g}, {b}],\n"));
    }
    code.push_str("];\n");

    code.push_str("pub const ACCENT_NAMES: &[&str] = &[\n");
    for a in &accent_names {
        code.push_str(&format!("    {},\n", esc(a)));
    }
    code.push_str("];\n");

    for (i, entries) in accents_vec.iter().enumerate() {
        code.push_str(&format!("const ACCENT_{i}: &[(usize, [u8; 3], [u8; 3])] = &[\n"));
        for (ri, [lr, lg, lb], [dr, dg, db]) in entries {
            code.push_str(&format!("    ({ri}, [{lr}, {lg}, {lb}], [{dr}, {dg}, {db}]),\n"));
        }
        code.push_str("];\n");
    }
    code.push_str("const ACCENTS: &[&[(usize, [u8; 3], [u8; 3])]] = &[\n");
    for i in 0..accents_vec.len() {
        code.push_str(&format!("    ACCENT_{i},\n"));
    }
    code.push_str("];\n");

    code.push_str("pub const ACCENT_VARIANTS: &[(&str, usize, usize, [u8; 3], [u8; 3])] = &[\n");
    for (v, ai, ri, [lr, lg, lb], [dr, dg, db]) in &variants {
        code.push_str(&format!("    ({}, {ai}, {ri}, [{lr}, {lg}, {lb}], [{dr}, {dg}, {db}]),\n", esc(v)));
    }
    code.push_str("];\n");

    code.push_str("const MASTERS: &[(&str, &str, &str)] = &[\n");
    for (id, fam, path) in &masters {
        let p = path.to_string_lossy().replace('\\', "/");
        code.push_str(&format!("    ({}, {}, include_str!({})),\n", esc(id), esc(fam), esc(&p)));
    }
    code.push_str("];\n");

    code.push_str("const ALIASES: &[(&str, &str, &str)] = &[\n");
    for (a, m, ac) in &aliases {
        code.push_str(&format!("    ({}, {}, {}),\n", esc(a), esc(m), esc(ac)));
    }
    code.push_str("];\n");

    code.push_str("const ICON_ACCENTS: &[(&str, &str)] = &[\n");
    for (id, ac) in &icon_accents {
        code.push_str(&format!("    ({}, {}),\n", esc(id), esc(ac)));
    }
    code.push_str("];\n");

    code.push_str(
        "pub const DATA: solidcraft_ui_kit::icons::IconData = solidcraft_ui_kit::icons::IconData {\n\
         \x20   role_names: ROLE_NAMES,\n\
         \x20   role_source_hex: ROLE_SOURCE_HEX,\n\
         \x20   palette: [LIGHT_PALETTE, DARK_PALETTE],\n\
         \x20   accent_names: ACCENT_NAMES,\n\
         \x20   accents: ACCENTS,\n\
         \x20   accent_variants: ACCENT_VARIANTS,\n\
         \x20   masters: MASTERS,\n\
         \x20   aliases: ALIASES,\n\
         \x20   icon_accents: ICON_ACCENTS,\n\
         \x20   danger_accent: \"constraint\",\n\
         };\n",
    );

    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    fs::write(out_dir.join("icon_data.rs"), code).expect("write icon_data.rs");
}
