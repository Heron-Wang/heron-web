//! 敏感信息脱敏 — 私钥/JWT/API key/密码/数据库连接串
//! 采用直接字符串扫描方式，避免实现完整正则引擎。

/// 脱敏并返回 (脱敏后文本, 替换次数)
pub fn sanitize_count(text: &str) -> (String, usize) {
    if text.is_empty() {
        return (text.to_string(), 0);
    }

    let mut result = text.to_string();
    let mut total = 0;

    // 1. 私钥整段（先处理，避免被其他规则破坏）
    let (r, c) = redact_private_keys(&result);
    result = r;
    total += c;

    // 2. JWT tokens (eyJxxx.yyy.zzz)
    let (r, c) = redact_jwt(&result);
    result = r;
    total += c;

    // 3. 各种已知 API key 前缀
    let (r, c) = redact_prefixed_keys(&result);
    result = r;
    total += c;

    // 4. key=value 形式的敏感信息 (password=xxx, token=xxx, etc.)
    let (r, c) = redact_key_value(&result);
    result = r;
    total += c;

    // 5. 数据库连接串中的密码 (://user:pass@host)
    let (r, c) = redact_db_connection(&result);
    result = r;
    total += c;

    (result, total)
}

/// 检查子串在指定位置是否匹配（大小写不敏感）
fn matches_ci(text: &str, pos: usize, needle: &str) -> bool {
    let text_chars: Vec<char> = text.chars().collect();
    let needle_chars: Vec<char> = needle.chars().collect();
    if pos + needle_chars.len() > text_chars.len() {
        return false;
    }
    for (i, nc) in needle_chars.iter().enumerate() {
        if text_chars[pos + i].to_ascii_lowercase() != nc.to_ascii_lowercase() {
            return false;
        }
    }
    true
}

/// 获取字符在 char vector 中的位置对应的字符串索引
fn char_index_to_byte(text: &str, char_idx: usize) -> usize {
    text.char_indices()
        .nth(char_idx)
        .map(|(byte_idx, _)| byte_idx)
        .unwrap_or(text.len())
}

/// 检查字符是否为"单词字符"（字母、数字、下划线）
fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// 检查字符是否为"非空白字符"
fn is_non_space(c: char) -> bool {
    !c.is_whitespace()
}

/// 从指定位置开始，收集连续的满足条件的字符，返回结束位置（char index）
fn collect_while(chars: &[char], start: usize, pred: &dyn Fn(char) -> bool) -> usize {
    let mut i = start;
    while i < chars.len() && pred(chars[i]) {
        i += 1;
    }
    i
}

/// 从指定位置开始跳过空白字符，返回新位置
fn skip_ws(chars: &[char], start: usize) -> usize {
    collect_while(chars, start, &|c| c.is_whitespace())
}

/// 脱敏私钥整段 (-----BEGIN ... PRIVATE KEY-----)
fn redact_private_keys(text: &str) -> (String, usize) {
    let marker = "-----BEGIN";
    let end_marker = "PRIVATE KEY-----";
    let mut result = String::new();
    let mut count = 0;
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;

    while i < chars.len() {
        if matches_ci(text, i, marker) {
            let begin_end = i + marker.len();
            if let Some(rel_pos) = text[char_index_to_byte(text, begin_end)..]
                .to_lowercase()
                .find(&end_marker.to_lowercase())
            {
                let abs_end_byte = char_index_to_byte(text, begin_end) + rel_pos + end_marker.len();
                let abs_end_char = text[..abs_end_byte].chars().count();
                result.push_str("***REDACTED***");
                i = abs_end_char;
                count += 1;
                continue;
            }
        }
        result.push(chars[i]);
        i += 1;
    }

    (result, count)
}

/// 脱敏 JWT tokens (eyJxxx.yyy.zzz)
fn redact_jwt(text: &str) -> (String, usize) {
    let prefix = "eyJ";
    let mut result = String::new();
    let mut count = 0;
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;

    while i < chars.len() {
        if i + prefix.len() <= chars.len()
            && chars[i] == 'e'
            && chars[i + 1] == 'y'
            && chars[i + 2] == 'J'
        {
            let after = i + prefix.len();
            let seg1_end = collect_while(&chars, after, &|c| {
                c.is_ascii_alphanumeric() || c == '_' || c == '-'
            });
            if seg1_end - after >= 10 && seg1_end < chars.len() && chars[seg1_end] == '.' {
                let seg2_start = seg1_end + 1;
                let seg2_end = collect_while(&chars, seg2_start, &|c| {
                    c.is_ascii_alphanumeric() || c == '_' || c == '-'
                });
                if seg2_end - seg2_start >= 10 && seg2_end < chars.len() && chars[seg2_end] == '.' {
                    let seg3_start = seg2_end + 1;
                    let seg3_end = collect_while(&chars, seg3_start, &|c| {
                        c.is_ascii_alphanumeric() || c == '_' || c == '-'
                    });
                    result.push_str("***REDACTED_JWT***");
                    i = if seg3_end > seg3_start {
                        seg3_end
                    } else {
                        seg2_end + 1
                    };
                    count += 1;
                    continue;
                }
            }
        }
        result.push(chars[i]);
        i += 1;
    }

    (result, count)
}

/// 脱敏已知前缀的 API keys
fn redact_prefixed_keys(text: &str) -> (String, usize) {
    let prefixes: &[&str] = &["sk-", "ghp_", "glpat-", "xoxb", "xoxp", "xoxo", "xoxa"];
    let mut result = String::new();
    let mut count = 0;
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;

    while i < chars.len() {
        let mut matched = false;
        for prefix in prefixes {
            let pchars: Vec<char> = prefix.chars().collect();
            if i + pchars.len() <= chars.len()
                && chars[i..i + pchars.len()]
                    .iter()
                    .zip(pchars.iter())
                    .all(|(a, b)| a == b)
            {
                let after = i + pchars.len();
                let end = collect_while(&chars, after, &|c| {
                    c.is_ascii_alphanumeric() || c == '-' || c == '_'
                });
                let min_len = if *prefix == "sk-" {
                    20
                } else if *prefix == "ghp_" {
                    36
                } else {
                    10
                };
                if end - after >= min_len {
                    result.push_str("***REDACTED***");
                    i = end;
                    count += 1;
                    matched = true;
                    break;
                }
            }
        }
        if !matched {
            result.push(chars[i]);
            i += 1;
        }
    }

    (result, count)
}

/// 脱敏 key=value 形式的敏感信息
fn redact_key_value(text: &str) -> (String, usize) {
    let keywords: &[&str] = &[
        "password",
        "passwd",
        "pwd",
        "密码",
        "token",
        "apikey",
        "api_key",
        "api-key",
        "secret",
        "bearer",
        "authorization",
        "api_key",
        "private_key",
        "credential",
    ];

    let mut result = String::new();
    let mut count = 0;
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;

    while i < chars.len() {
        let at_word_boundary = i == 0 || !is_word_char(chars[i - 1]);

        if at_word_boundary {
            let mut found_keyword = None;
            for kw in keywords {
                let kw_chars: Vec<char> = kw.chars().collect();
                if i + kw_chars.len() <= chars.len() {
                    let matches = chars[i..i + kw_chars.len()]
                        .iter()
                        .zip(kw_chars.iter())
                        .all(|(a, b)| a.to_ascii_lowercase() == b.to_ascii_lowercase());
                    let after = i + kw_chars.len();
                    let boundary_ok = after >= chars.len() || !is_word_char(chars[after]);

                    let env_vars = [
                        "secret",
                        "password",
                        "token",
                        "api_key",
                        "private_key",
                        "credential",
                    ];

                    if matches && (boundary_ok || env_vars.contains(kw)) {
                        found_keyword = Some(*kw);
                        break;
                    }
                }
            }

            if let Some(kw) = found_keyword {
                let kw_len = kw.chars().count();
                let after_kw = i + kw_len;
                let mut key_end = after_kw;
                if chars.get(after_kw) == Some(&'_') {
                    key_end = collect_while(&chars, after_kw, &|c| is_word_char(c));
                }
                let after_ws = skip_ws(&chars, key_end);

                if after_ws < chars.len() && (chars[after_ws] == ':' || chars[after_ws] == '=') {
                    let after_sep = skip_ws(&chars, after_ws + 1);
                    let val_end = collect_while(&chars, after_sep, &|c| is_non_space(c));

                    if val_end > after_sep {
                        result.push_str(&chars[i..key_end].iter().collect::<String>());
                        result.push_str(&chars[key_end..after_sep].iter().collect::<String>());
                        result.push_str("***REDACTED***");
                        i = val_end;
                        count += 1;
                        continue;
                    }
                }
            }
        }

        result.push(chars[i]);
        i += 1;
    }

    (result, count)
}

/// 脱敏数据库连接串中的密码 (://user:password@host)
fn redact_db_connection(text: &str) -> (String, usize) {
    let mut result = String::new();
    let mut count = 0;
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;

    while i + 3 < chars.len() {
        if chars[i] == ':' && i + 2 < chars.len() && chars[i + 1] == '/' && chars[i + 2] == '/' {
            let after_slash = i + 3;
            let user_end = collect_while(&chars, after_slash, &|c| {
                !(c == ':' || c == '@' || c == '/' || c.is_whitespace())
            });

            if user_end > after_slash && user_end < chars.len() && chars[user_end] == ':' {
                let pass_start = user_end + 1;
                let pass_end = collect_while(&chars, pass_start, &|c| {
                    !(c == '@' || c == '/' || c.is_whitespace())
                });

                if pass_end > pass_start && pass_end < chars.len() && chars[pass_end] == '@' {
                    result.push_str(&chars[i..user_end + 1].iter().collect::<String>());
                    result.push_str("***REDACTED***");
                    result.push_str(&chars[pass_end..].iter().collect::<String>());
                    i = chars.len();
                    count += 1;
                    continue;
                }
            }
        }
        result.push(chars[i]);
        i += 1;
    }

    while i < chars.len() {
        result.push(chars[i]);
        i += 1;
    }

    (result, count)
}
