//! Layered TOML configuration and themes.
//!
//! Resolution: embedded `default.toml` → theme chain (base first, via
//! `meta.extends`) → user `config.toml`. Tables merge key by key, so every
//! layer only needs the keys it changes.

mod merge;
mod model;
mod watch;

use std::fs;
use std::path::{Path, PathBuf};

use toml::Table;

pub use merge::{deep_merge, unknown_keys};
pub use model::*;
pub use watch::{ConfigWatcher, watch};

pub const DEFAULT_TOML: &str = include_str!("../default.toml");

/// Themes shipped inside the binary. `dark` is the empty theme (`default.toml` is dark).
pub const BUNDLED_THEMES: &[(&str, &str)] = &[("dark", ""), ("light", include_str!("../themes/light.toml"))];

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("{file}: {message}")]
    Parse { file: String, message: String },
    #[error("theme {0:?} not found")]
    ThemeNotFound(String),
    #[error("theme inheritance cycle: {0}")]
    Cycle(String),
}

/// A successfully resolved configuration plus non-fatal warnings.
pub struct Resolved {
    pub config: Config,
    pub warnings: Vec<String>,
}

/// Where user files live. `None` dir = defaults + bundled themes only.
#[derive(Clone, Debug)]
pub struct ConfigDirs {
    pub root: Option<PathBuf>,
}

impl ConfigDirs {
    /// `%APPDATA%\plum`, `~/.config/plum`, ...
    pub fn system() -> Self {
        Self { root: dirs::config_dir().map(|d| d.join("plum")) }
    }

    pub fn config_file(&self) -> Option<PathBuf> {
        self.root.as_ref().map(|r| r.join("config.toml"))
    }

    pub fn themes_dir(&self) -> Option<PathBuf> {
        self.root.as_ref().map(|r| r.join("themes"))
    }

    /// Create the folder layout and a reference copy of the defaults.
    /// Never overwrites user files (the `.example` reference is refreshed).
    pub fn ensure_scaffold(&self) -> std::io::Result<()> {
        let Some(root) = &self.root else { return Ok(()) };
        fs::create_dir_all(root.join("themes"))?;
        fs::write(root.join("default.toml.example"), DEFAULT_TOML)?;
        let cfg = root.join("config.toml");
        if !cfg.exists() {
            fs::write(
                &cfg,
                "# Your Plum overrides. Only add the keys you want to change;\n\
                 # see default.toml.example for every option.\n\n\
                 theme = \"dark\"\n",
            )?;
        }
        Ok(())
    }

    /// Bundled theme names followed by user themes (sorted, deduplicated).
    pub fn theme_names(&self) -> Vec<String> {
        let mut names: Vec<String> = BUNDLED_THEMES.iter().map(|(n, _)| n.to_string()).collect();
        if let Some(Ok(rd)) = self.themes_dir().map(fs::read_dir) {
            let mut user: Vec<String> = rd
                .filter_map(|e| e.ok())
                .map(|e| e.path())
                .filter(|p| p.extension().is_some_and(|x| x == "toml"))
                .filter_map(|p| p.file_stem().map(|s| s.to_string_lossy().into_owned()))
                .filter(|n| !names.contains(n))
                .collect();
            user.sort();
            names.extend(user);
        }
        names
    }

    /// Persist the chosen theme into `config.toml`, keeping the rest of the file intact.
    pub fn set_theme(&self, name: &str) -> std::io::Result<()> {
        let Some(path) = self.config_file() else { return Ok(()) };
        let text = fs::read_to_string(&path).unwrap_or_default();
        fs::write(&path, set_top_level_key(&text, "theme", &format!("{name:?}")))
    }

    /// Set (`Some`, an empty list unbinds) or remove (`None`, falls back to
    /// theme/default) one `[keybinds]` entry in `config.toml`, preserving
    /// comments and layout.
    pub fn set_keybind(&self, action: &str, chords: Option<&[String]>) -> std::io::Result<()> {
        let Some(path) = self.config_file() else { return Ok(()) };
        let text = fs::read_to_string(&path).unwrap_or_default();
        fs::write(&path, edit_keybind(&text, action, chords)?)
    }

    /// Remove every keybind override from `config.toml`.
    pub fn reset_keybinds(&self) -> std::io::Result<()> {
        let Some(path) = self.config_file() else { return Ok(()) };
        let text = fs::read_to_string(&path).unwrap_or_default();
        let mut doc = parse_doc(&text)?;
        doc.remove("keybinds");
        fs::write(&path, doc.to_string())
    }

    pub fn load(&self) -> Result<Resolved, ConfigError> {
        let user = match self.config_file() {
            Some(p) if p.exists() => Some((p.display().to_string(), read_file(&p)?)),
            _ => None,
        };
        resolve(user.as_ref().map(|(f, t)| (f.as_str(), t)), |name| self.theme_source(name))
    }

    fn theme_source(&self, name: &str) -> Result<(String, Table), ConfigError> {
        if let Some(dir) = self.themes_dir() {
            let p = dir.join(format!("{name}.toml"));
            if p.exists() {
                return Ok((p.display().to_string(), read_file(&p)?));
            }
        }
        bundled_theme(name)
    }
}

fn bundled_theme(name: &str) -> Result<(String, Table), ConfigError> {
    let (_, src) = BUNDLED_THEMES.iter().find(|(n, _)| *n == name).ok_or_else(|| ConfigError::ThemeNotFound(name.into()))?;
    Ok((format!("<bundled theme {name}>"), parse(&format!("<bundled theme {name}>"), src)?))
}

fn read_file(p: &Path) -> Result<Table, ConfigError> {
    let file = p.display().to_string();
    let src = fs::read_to_string(p).map_err(|e| ConfigError::Parse { file: file.clone(), message: e.to_string() })?;
    parse(&file, &src)
}

fn parse(file: &str, src: &str) -> Result<Table, ConfigError> {
    toml::from_str(src).map_err(|e| ConfigError::Parse { file: file.into(), message: e.to_string() })
}

pub fn default_table() -> Table {
    toml::from_str(DEFAULT_TOML).expect("embedded default.toml is valid")
}

/// Resolve layers into a [`Config`]. `user` is `(file label, table)`;
/// `find_theme` returns `(file label, table)` for a theme name.
pub fn resolve(
    user: Option<(&str, &Table)>,
    find_theme: impl Fn(&str) -> Result<(String, Table), ConfigError>,
) -> Result<Resolved, ConfigError> {
    let defaults = default_table();
    let mut warnings = Vec::new();

    let theme_name = user
        .and_then(|(_, t)| t.get("theme"))
        .and_then(|v| v.as_str())
        .or_else(|| defaults.get("theme").and_then(|v| v.as_str()))
        .unwrap_or("dark")
        .to_string();

    // Walk `extends` from the selected theme to its root.
    let mut chain: Vec<(String, Table)> = Vec::new();
    let mut seen: Vec<String> = Vec::new();
    let mut next = Some(theme_name.clone());
    while let Some(name) = next.take() {
        if seen.contains(&name) {
            seen.push(name);
            return Err(ConfigError::Cycle(seen.join(" -> ")));
        }
        seen.push(name.clone());
        let (file, table) = match find_theme(&name) {
            Ok(t) => t,
            // A missing selected theme falls back to defaults instead of failing.
            Err(ConfigError::ThemeNotFound(n)) if chain.is_empty() => {
                warnings.push(format!("theme {n:?} not found; using default"));
                break;
            }
            Err(e) => return Err(e),
        };
        let parent = table.get("meta").and_then(|m| m.get("extends")).and_then(|v| v.as_str()).filter(|s| !s.is_empty());
        next = parent.map(str::to_string);
        chain.push((file, table));
    }

    let mut merged = defaults.clone();
    for (file, table) in chain.iter().rev() {
        warn_unknown(&defaults, table, file, &mut warnings);
        deep_merge(&mut merged, table);
    }
    if let Some((file, table)) = user {
        warn_unknown(&defaults, table, file, &mut warnings);
        // Theme metadata describes themes, not the user file.
        let mut table = table.clone();
        table.remove("meta");
        deep_merge(&mut merged, &table);
    }
    merged.insert("theme".into(), toml::Value::String(theme_name));

    let config: Config = merged.try_into().map_err(|e: toml::de::Error| ConfigError::Parse { file: "resolved config".into(), message: e.to_string() })?;
    Ok(Resolved { config, warnings })
}

fn warn_unknown(defaults: &Table, layer: &Table, file: &str, warnings: &mut Vec<String>) {
    for key in unknown_keys(defaults, layer) {
        warnings.push(format!("{file}: unknown key `{key}` (ignored)"));
    }
}

fn parse_doc(text: &str) -> std::io::Result<toml_edit::DocumentMut> {
    text.parse().map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, format!("config.toml: {e}")))
}

fn edit_keybind(text: &str, action: &str, chords: Option<&[String]>) -> std::io::Result<String> {
    let mut doc = parse_doc(text)?;
    match chords {
        Some(list) => {
            if !doc.contains_table("keybinds") {
                doc["keybinds"] = toml_edit::table();
            }
            doc["keybinds"][action] = match list {
                [one] => toml_edit::value(one.as_str()),
                many => toml_edit::value(many.iter().map(String::as_str).collect::<toml_edit::Array>()),
            };
        }
        None => {
            let now_empty = doc.get_mut("keybinds").and_then(|k| k.as_table_like_mut()).is_some_and(|t| {
                t.remove(action);
                t.is_empty()
            });
            if now_empty {
                doc.remove("keybinds");
            }
        }
    }
    Ok(doc.to_string())
}

/// Replace or insert `key = value` before the first table header.
fn set_top_level_key(text: &str, key: &str, value: &str) -> String {
    let mut lines: Vec<String> = text.lines().map(str::to_string).collect();
    let header = lines.iter().position(|l| l.trim_start().starts_with('[')).unwrap_or(lines.len());
    let existing = lines[..header].iter().position(|l| {
        let t = l.trim_start();
        t.strip_prefix(key).is_some_and(|rest| rest.trim_start().starts_with('='))
    });
    let line = format!("{key} = {value}");
    match existing {
        Some(i) => lines[i] = line,
        None => lines.insert(header, line),
    }
    let mut out = lines.join("\n");
    out.push('\n');
    out
}

#[cfg(test)]
mod tests;
