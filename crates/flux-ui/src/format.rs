//! Display formatting: sizes, timestamps. UI-only concerns live here.

use std::time::SystemTime;

pub fn format_size(size: u64) -> String {
    if size == flux_core::UNKNOWN_SIZE {
        return String::new();
    }
    const UNITS: [&str; 6] = ["B", "KB", "MB", "GB", "TB", "PB"];
    let mut v = size as f64;
    let mut u = 0;
    while v >= 1024.0 && u < UNITS.len() - 1 {
        v /= 1024.0;
        u += 1;
    }
    if u == 0 {
        format!("{} B", size)
    } else if v >= 100.0 {
        format!("{v:.0} {}", UNITS[u])
    } else {
        format!("{v:.1} {}", UNITS[u])
    }
}

pub fn format_size_compact(size: u64) -> String {
    format_size(size)
}

pub fn format_mtime(m: Option<SystemTime>) -> String {
    let Some(m) = m else { return String::new() };
    let Ok(ts) = jiff::Timestamp::try_from(m) else {
        return String::new();
    };
    let zoned = ts.to_zoned(jiff::tz::TimeZone::system());
    zoned.strftime("%Y-%m-%d %H:%M").to_string()
}

pub fn format_count(n: usize) -> String {
    if n < 1_000 {
        return n.to_string();
    }
    let s = n.to_string();
    let mut out = String::with_capacity(s.len() + s.len() / 3);
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    out
}
