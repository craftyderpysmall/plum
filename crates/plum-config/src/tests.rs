use super::*;

fn t(s: &str) -> Table {
    toml::from_str(s).unwrap()
}

fn bundled_only(name: &str) -> Result<(String, Table), ConfigError> {
    bundled_theme(name)
}

#[test]
fn embedded_default_resolves() {
    let r = resolve(None, bundled_only).unwrap();
    assert!(r.warnings.is_empty(), "{:?}", r.warnings);
    assert_eq!(r.config.theme, "dark");
    assert_eq!(r.config.colors.canvas, Color([0x15, 0x15, 0x15, 255]));
    assert_eq!(r.config.keybinds["redo"].0, vec!["Ctrl+Shift+Z", "Ctrl+Y"]);
    assert_eq!(r.config.keybinds["reset_position"].0, vec!["Ctrl+9"]);
    assert_eq!(r.config.tools.pen.color, ColorRef::Palette(0));
    assert_eq!(r.config.tools.shape.fill, OptColorRef(None));
}

#[test]
fn tool_colors_accept_index_or_hex() {
    let user = t("[tools.pen]\ncolor = \"#112233\"\n[tools.shape]\nfill = 4");
    let r = resolve(Some(("config.toml", &user)), bundled_only).unwrap().config;
    assert_eq!(r.tools.pen.color, ColorRef::Fixed(Color([0x11, 0x22, 0x33, 255])));
    assert_eq!(r.tools.shape.fill, OptColorRef(Some(ColorRef::Palette(4))));
}

#[test]
fn keybind_edits_preserve_file_and_fall_back() {
    let src = "# mine\ntheme = \"dark\"\n\n[colors]\ncanvas = \"#000000\"\n";
    let set = edit_keybind(src, "undo", Some(&["Ctrl+U".to_string()])).unwrap();
    let set = edit_keybind(&set, "copy", Some(&[])).unwrap();
    assert!(set.starts_with("# mine\ntheme = \"dark\"\n"), "{set}");
    let user: Table = toml::from_str(&set).unwrap();
    let r = resolve(Some(("config.toml", &user)), bundled_only).unwrap();
    assert!(r.warnings.is_empty(), "{:?}", r.warnings);
    assert_eq!(r.config.keybinds["undo"].0, vec!["Ctrl+U"]);
    assert_eq!(r.config.keybinds["redo"].0, vec!["Ctrl+Shift+Z", "Ctrl+Y"]);
    assert!(r.config.keybinds["copy"].0.is_empty());
    // Removing the overrides drops the now-empty table again.
    let back = edit_keybind(&set, "undo", None).unwrap();
    let back = edit_keybind(&back, "copy", None).unwrap();
    assert!(!back.contains("keybinds"), "{back}");
}

#[test]
fn every_bundled_theme_resolves_cleanly() {
    for (name, _) in BUNDLED_THEMES {
        let user = t(&format!("theme = {name:?}"));
        let r = resolve(Some(("config.toml", &user)), bundled_only).unwrap();
        assert!(r.warnings.is_empty(), "{name}: {:?}", r.warnings);
        assert_eq!(&r.config.theme, name);
    }
}

#[test]
fn partial_theme_falls_back_per_key() {
    let user = t("theme = \"mine\"");
    let mine = t("[colors]\ncanvas = \"#000000\"");
    let r = resolve(Some(("config.toml", &user)), |n| if n == "mine" { Ok(("mine.toml".into(), mine.clone())) } else { bundled_only(n) }).unwrap();
    let d = resolve(None, bundled_only).unwrap().config;
    assert_eq!(r.config.colors.canvas, Color([0, 0, 0, 255]));
    assert_eq!(r.config.colors.grid, d.colors.grid);
    assert_eq!(r.config.ui, d.ui);
    assert_eq!(r.config.keybinds, d.keybinds);
}

#[test]
fn extends_chain_applies_base_first() {
    // child extends light: child's canvas wins, light's panel_border shows through.
    let user = t("theme = \"child\"");
    let child = t("[meta]\nextends = \"light\"\n[colors]\ncanvas = \"#2e3440\"\n[ui]\ncorner_radius = 6.0");
    let find = |n: &str| if n == "child" { Ok(("child.toml".into(), child.clone())) } else { bundled_only(n) };
    let r = resolve(Some(("config.toml", &user)), find).unwrap().config;
    assert_eq!(r.colors.canvas, Color::parse("#2e3440").unwrap());
    assert_eq!(r.colors.panel_border, Color::parse("#15151514").unwrap());
    assert_eq!(r.ui.corner_radius, 6.0);
}

#[test]
fn extends_cycle_is_an_error() {
    let user = t("theme = \"a\"");
    let find = |n: &str| -> Result<(String, Table), ConfigError> {
        let parent = if n == "a" { "b" } else { "a" };
        Ok((n.into(), t(&format!("[meta]\nextends = {parent:?}"))))
    };
    assert!(matches!(resolve(Some(("config.toml", &user)), find), Err(ConfigError::Cycle(_))));
}

#[test]
fn user_config_overrides_theme() {
    let user = t("theme = \"light\"\n[colors]\ncanvas = \"#123456\"\n[ui]\ntoolbar_position = \"top\"");
    let r = resolve(Some(("config.toml", &user)), bundled_only).unwrap().config;
    assert_eq!(r.colors.canvas, Color::parse("#123456").unwrap());
    assert_eq!(r.colors.text, Color::parse("#151515").unwrap());
    assert_eq!(r.ui.toolbar_position, ToolbarPosition::Top);
}

#[test]
fn unknown_keys_warn_but_load() {
    let user = t("[colors]\ncanvs = \"#000000\"");
    let r = resolve(Some(("config.toml", &user)), bundled_only).unwrap();
    assert_eq!(r.warnings.len(), 1);
    assert!(r.warnings[0].contains("colors.canvs"));
}

#[test]
fn bad_value_is_an_error() {
    let user = t("[colors]\ncanvas = \"red\"");
    assert!(matches!(resolve(Some(("config.toml", &user)), bundled_only), Err(ConfigError::Parse { .. })));
}

#[test]
fn missing_theme_falls_back_with_warning() {
    let user = t("theme = \"ghost\"");
    let r = resolve(Some(("config.toml", &user)), bundled_only).unwrap();
    assert_eq!(r.warnings.len(), 1);
    assert_eq!(r.config.colors, resolve(None, bundled_only).unwrap().config.colors);
}

#[test]
fn set_theme_preserves_rest_of_file() {
    let src = "# my config\ntheme = \"dark\"\n\n[colors]\ncanvas = \"#000000\"\n";
    let out = set_top_level_key(src, "theme", "\"light\"");
    assert_eq!(out, "# my config\ntheme = \"light\"\n\n[colors]\ncanvas = \"#000000\"\n");
    let out = set_top_level_key("[ui]\nshadow = false\n", "theme", "\"light\"");
    assert_eq!(out, "theme = \"light\"\n[ui]\nshadow = false\n");
}

#[test]
fn user_dir_themes_override_bundled() {
    let root = std::env::temp_dir().join(format!("plum-cfg-test-{}", std::process::id()));
    let dirs = ConfigDirs { root: Some(root.clone()) };
    dirs.ensure_scaffold().unwrap();
    fs::write(root.join("themes").join("light.toml"), "[colors]\ncanvas = \"#010203\"").unwrap();
    fs::write(root.join("themes").join("zzz.toml"), "").unwrap();
    dirs.set_theme("light").unwrap();
    let r = dirs.load().unwrap().config;
    assert_eq!(r.colors.canvas, Color([1, 2, 3, 255]));
    // User "light" replaces the bundled one entirely, so its text color is the default's.
    assert_eq!(r.colors.text, Color::parse("#fafafa").unwrap());
    assert!(dirs.theme_names().ends_with(&["zzz".to_string()]));
    fs::remove_dir_all(root).unwrap();
}
