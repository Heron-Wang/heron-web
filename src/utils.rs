//! 工具函数 — 时间转换、JSON 序列化辅助
//! 零第三方依赖，纯标准库实现。

use std::time::{SystemTime, UNIX_EPOCH};

/// 返回 ISO 8601 格式的时间戳字符串（如 2024-01-15T12:30:45）
pub fn now_iso() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    epoch_to_iso(secs)
}

/// 将 epoch 秒转为 ISO 8601 字符串 (UTC)
fn epoch_to_iso(secs: u64) -> String {
    let days = secs / 86400;
    let remainder = secs % 86400;
    let hour = remainder / 3600;
    let min = (remainder % 3600) / 60;
    let sec = remainder % 60;
    let (year, month, day) = days_to_ymd(days as i64);
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}",
        year, month, day, hour, min, sec
    )
}

/// 将天数（从 1970-01-01 起）转为年月日
fn days_to_ymd(days: i64) -> (i64, u32, u32) {
    let mut days = days;
    let mut year = 1970i64;
    loop {
        let days_in_year = if is_leap(year) { 366 } else { 365 };
        if days < days_in_year {
            break;
        }
        days -= days_in_year;
        year += 1;
    }
    let months = [31u32, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    let mut month = 1u32;
    for &dm in months.iter() {
        let dm = if month == 2 && is_leap(year) { 29 } else { dm };
        if days < dm as i64 {
            break;
        }
        days -= dm as i64;
        month += 1;
    }
    (year, month, (days + 1) as u32)
}

fn is_leap(year: i64) -> bool {
    (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0)
}

/// JSON 字符串转义
pub fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 8);
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                out.push_str(&format!("\\u{:04x}", c as u32));
            }
            c => out.push(c),
        }
    }
    out
}

/// 将 JSON 字符串数组序列化为 JSON 数组字符串
pub fn json_string_array(arr: &[String]) -> String {
    let parts: Vec<String> = arr
        .iter()
        .map(|s| format!("\"{}\"", json_escape(s)))
        .collect();
    format!("[{}]", parts.join(","))
}

/// 将 i64 数组序列化为 JSON 数组字符串
pub fn json_int_array(arr: &[i64]) -> String {
    let parts: Vec<String> = arr.iter().map(|v| v.to_string()).collect();
    format!("[{}]", parts.join(","))
}
