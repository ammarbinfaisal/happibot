use std::{fs, path::Path};

pub fn load_dotenv_if_present() {
    let path = std::env::var("DOTENV_PATH").unwrap_or_else(|_| ".env".to_string());
    let path = Path::new(&path);
    let Ok(contents) = fs::read_to_string(path) else {
        return;
    };

    for raw_line in contents.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        let line = line.strip_prefix("export ").unwrap_or(line).trim();
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };

        let key = key.trim();
        if key.is_empty() {
            continue;
        }

        if std::env::var_os(key).is_some() {
            continue;
        }

        let mut value = value.trim().to_string();
        if value.len() >= 2 {
            let bytes = value.as_bytes();
            let first = bytes[0];
            let last = bytes[value.len() - 1];
            if (first == b'"' && last == b'"') || (first == b'\'' && last == b'\'') {
                value = value[1..value.len() - 1].to_string();
            }
        }

        // `set_var` is `unsafe` in Rust 2024 because concurrent access to the
        // process environment is UB. We call this before spinning up any
        // runtime threads.
        unsafe { std::env::set_var(key, value) };
    }
}
