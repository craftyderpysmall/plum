use toml::{Table, Value};

/// Deep-merge `over` into `base`: tables merge key by key, everything else
/// (including arrays) is replaced.
pub fn deep_merge(base: &mut Table, over: &Table) {
    for (k, v) in over {
        match (base.get_mut(k), v) {
            (Some(Value::Table(b)), Value::Table(o)) => deep_merge(b, o),
            _ => {
                base.insert(k.clone(), v.clone());
            }
        }
    }
}

/// Dotted paths present in `layer` but not in `reference` (typo detection).
pub fn unknown_keys(reference: &Table, layer: &Table) -> Vec<String> {
    let mut out = Vec::new();
    walk(reference, layer, "", &mut out);
    out
}

fn walk(reference: &Table, layer: &Table, prefix: &str, out: &mut Vec<String>) {
    for (k, v) in layer {
        let path = if prefix.is_empty() { k.clone() } else { format!("{prefix}.{k}") };
        match (reference.get(k), v) {
            (None, _) => out.push(path),
            (Some(Value::Table(r)), Value::Table(l)) => walk(r, l, &path, out),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(s: &str) -> Table {
        toml::from_str(s).unwrap()
    }

    #[test]
    fn merges_nested_tables_and_replaces_arrays() {
        let mut base = t("a = 1\n[x]\nb = 2\nc = 3\nlist = [1, 2]");
        deep_merge(&mut base, &t("[x]\nc = 30\nlist = [9]"));
        assert_eq!(base, t("a = 1\n[x]\nb = 2\nc = 30\nlist = [9]"));
    }

    #[test]
    fn reports_unknown_paths() {
        let r = t("[colors]\ncanvas = 1\n[tools.pen]\nwidth = 1");
        let l = t("[colors]\ncanvs = 1\n[tools.pen]\nwidth = 2\ncolour = 3\n[nope]");
        assert_eq!(unknown_keys(&r, &l), vec!["colors.canvs", "nope", "tools.pen.colour"]);
    }
}
