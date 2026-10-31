//! The portal bundles each built-in theme as proto3 JSON
//! (`portal/assets/themes/<id>.json`) because Dart cannot parse textproto. This
//! is the drift guard: every asset must equal the JSON form of its source of
//! truth, `config/themes/<id>.textproto`, and there must be no stray assets.
//!
//! After editing a theme, regenerate the assets with
//! `THEME_ASSETS_WRITE=1 cargo test -p agent-theme --test portal_assets_parity`.

use agent_theme::{builtins, textproto};
use serde_json::Value;
use std::path::PathBuf;

fn assets_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../portal/assets/themes")
}

fn render(v: &Value) -> String {
    let mut s = serde_json::to_string_pretty(v).expect("JSON value serialises");
    s.push('\n');
    s
}

#[test]
fn positive_portal_assets_match_textprotos() {
    let write = std::env::var_os("THEME_ASSETS_WRITE").is_some();
    let mut stale = Vec::new();
    for mut theme in builtins().expect("built-ins validate") {
        // `builtin` is server-set; the asset carries the authored document only.
        theme.builtin = false;
        let want = render(&textproto::to_json(&theme));
        let path = assets_dir().join(format!("{}.json", theme.id));
        if write {
            std::fs::create_dir_all(assets_dir()).unwrap();
            std::fs::write(&path, &want).unwrap();
            continue;
        }
        let got = std::fs::read_to_string(&path).unwrap_or_default();
        let got_json: Option<Value> = serde_json::from_str(&got).ok();
        if got_json.as_ref().map(render).as_deref() != Some(want.as_str()) {
            stale.push(path.display().to_string());
        }
    }
    assert!(
        stale.is_empty(),
        "portal theme assets are stale: {stale:?}\nregenerate: THEME_ASSETS_WRITE=1 cargo test -p agent-theme --test portal_assets_parity"
    );
}

#[test]
fn negative_no_stray_assets() {
    let ids: Vec<String> = builtins()
        .unwrap()
        .into_iter()
        .map(|t| format!("{}.json", t.id))
        .collect();
    for entry in std::fs::read_dir(assets_dir()).expect("portal/assets/themes exists") {
        let name = entry.unwrap().file_name().into_string().unwrap();
        assert!(
            ids.contains(&name),
            "portal/assets/themes/{name} has no config/themes twin"
        );
    }
}

/// Check the check: a changed colour in the JSON must not compare equal.
#[test]
fn negative_edited_asset_is_detected() {
    let theme = builtins().unwrap().remove(2);
    let mut json = textproto::to_json(&theme);
    json["light"]["primary"] = Value::String("#123456".into());
    assert_ne!(render(&json), render(&textproto::to_json(&theme)));
}

#[test]
fn positive_json_uses_proto3_names() {
    let accessible = builtins()
        .unwrap()
        .into_iter()
        .find(|t| t.id == "accessible")
        .unwrap();
    let json = textproto::to_json(&accessible);
    assert_eq!(json["light"]["onSurface"], "#000000");
    assert_eq!(json["typography"]["bodyFamily"], "AtkinsonHyperlegibleNext");
    assert_eq!(json["accessibility"]["contrast"], "CONTRAST_LEVEL_AAA");
    assert_eq!(json["density"], "DENSITY_COMFORTABLE");
    assert_eq!(json["builtin"], true);
    assert!(json.get("version").is_none(), "unset fields are omitted");
}
