use std::path::Path;

use notify::{RecommendedWatcher, RecursiveMode, Watcher};

/// Keeps the file watcher alive; dropping it stops watching.
pub struct ConfigWatcher {
    _inner: RecommendedWatcher,
}

/// Call `on_change` whenever a `.toml` file under `dir` changes.
pub fn watch(dir: &Path, on_change: impl Fn() + Send + 'static) -> notify::Result<ConfigWatcher> {
    let mut w = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
        let Ok(ev) = res else { return };
        let relevant = ev.paths.iter().any(|p| p.extension().is_some_and(|x| x == "toml"));
        if relevant && (ev.kind.is_modify() || ev.kind.is_create() || ev.kind.is_remove()) {
            on_change();
        }
    })?;
    w.watch(dir, RecursiveMode::Recursive)?;
    Ok(ConfigWatcher { _inner: w })
}
