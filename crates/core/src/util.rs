/// Single-line preview, truncated on a char boundary.
pub(crate) fn preview(text: &str, max_chars: usize) -> String {
    let flat: String = text
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .take(max_chars + 1)
        .collect();
    let flat = flat.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() > max_chars {
        let mut s: String = flat.chars().take(max_chars).collect();
        s.push('…');
        s
    } else {
        flat
    }
}

/// `2024-01-18 01:30:22 UTC` for a Unix timestamp in seconds.
pub(crate) fn format_unix(secs: i64) -> String {
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let (y, m, d) = civil_from_days(days);
    format!("{y:04}-{m:02}-{d:02} {:02}:{:02}:{:02} UTC", rem / 3600, rem % 3600 / 60, rem % 60)
}

/// Howard Hinnant's days → (year, month, day) algorithm.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (yoe + era * 400 + i64::from(m <= 2), m, d)
}

pub(crate) fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// "in 3h 20m" / "2d 4h ago".
pub(crate) fn relative(target: i64, now: i64) -> String {
    let delta = target - now;
    let abs = delta.unsigned_abs();
    let (d, h, m, s) = (abs / 86_400, abs % 86_400 / 3600, abs % 3600 / 60, abs % 60);
    let span = if d >= 365 {
        format!("{:.1} years", d as f64 / 365.25)
    } else if d > 0 {
        format!("{d}d {h}h")
    } else if h > 0 {
        format!("{h}h {m}m")
    } else if m > 0 {
        format!("{m}m {s}s")
    } else {
        format!("{s}s")
    };
    if delta >= 0 { format!("in {span}") } else { format!("{span} ago") }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates() {
        assert_eq!(format_unix(0), "1970-01-01 00:00:00 UTC");
        assert_eq!(format_unix(1_516_239_022), "2018-01-18 01:30:22 UTC");
        assert_eq!(relative(100, 0), "in 1m 40s");
        assert_eq!(relative(0, 90_000), "1d 1h ago");
        assert_eq!(preview("a\n  b\tc", 10), "a b c");
        assert_eq!(preview("abcdef", 3), "abc…");
    }
}
