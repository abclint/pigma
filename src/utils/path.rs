use std::path::PathBuf;

use dirs::home_dir;

/// Replace illegal characters in a file name with `_`.
pub fn sanitize_filename(s: &str) -> String {
    s.trim()
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            _ => c,
        })
        .collect()
}

/// pigma cache root directory (cache files and play queues live under it).
pub fn pigma_cache_dir() -> PathBuf {
    dirs::cache_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("pigma")
}

/// pigma config root directory.
pub fn pigma_config_dir() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("pigma")
}

pub fn expand_tilde(path: &str) -> PathBuf {
    let home = match home_dir() {
        Some(h) => h,
        None => return PathBuf::from(path),
    };

    if path == "~" {
        return home;
    }

    if let Some(rest) = path.strip_prefix("~/") {
        return home.join(rest);
    }

    if cfg!(windows)
        && let Some(rest) = path.strip_prefix("~\\")
    {
        return home.join(rest);
    }

    PathBuf::from(path)
}

#[cfg(test)]
mod tests {
    use dirs::home_dir;

    use super::*;

    #[test]
    fn test_expand_tilde() {
        let home = home_dir().unwrap();

        assert_eq!(expand_tilde("~"), home);

        let unix_path = expand_tilde("~/.cache/dir/xx");
        assert_eq!(unix_path, home.join(".cache/dir/xx"));

        let win_input = r"~\.cache\dir\xx";
        if cfg!(windows) {
            let expected = home.join(r".cache\dir\xx");
            assert_eq!(expand_tilde(win_input), expected);
        } else {
            assert_eq!(expand_tilde(win_input), PathBuf::from(win_input));
        }
    }
}
