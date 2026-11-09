// SPDX-License-Identifier: LGPL-2.1-or-later
//! `os-release` key/value files.

/// Parses an os-release file into `(key, value)` pairs, unquoting values.
pub fn parse(text: &str) -> Vec<(String, String)> {
    text.lines()
        .filter_map(|l| {
            let l = l.trim();
            if l.is_empty() || l.starts_with('#') {
                return None;
            }
            let (k, v) = l.split_once('=')?;
            let v = v.trim();
            let v = v
                .strip_prefix('"')
                .and_then(|x| x.strip_suffix('"'))
                .or_else(|| v.strip_prefix('\'').and_then(|x| x.strip_suffix('\'')))
                .unwrap_or(v);
            Some((k.trim().to_string(), v.replace("\\\"", "\"")))
        })
        .collect()
}

/// Looks up a key.
pub fn get<'a>(pairs: &'a [(String, String)], key: &str) -> Option<&'a str> {
    pairs
        .iter()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v.as_str())
}

#[cfg(test)]
mod tests {
    #[test]
    fn parses() {
        let p = super::parse("ID=debian\nVERSION_ID=\"13\"\n# c\nID_LIKE='rhel fedora'\n");
        assert_eq!(super::get(&p, "ID"), Some("debian"));
        assert_eq!(super::get(&p, "VERSION_ID"), Some("13"));
        assert_eq!(super::get(&p, "ID_LIKE"), Some("rhel fedora"));
    }
}
