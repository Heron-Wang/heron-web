//! Heron Wang · 个人网站主服务 (Rust 版)
//! 纯标准库实现，零第三方依赖。
//! 监听 0.0.0.0:8080，多线程处理并发。

mod store;

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use store::{Store, JsonParser, JsonValue};

// ── 配置 ────────────────────────────────────────────

const HOST: &str = "0.0.0.0";
const PORT: u16 = 8080;
const DATA_DIR: &str = "data";

/// 从环境变量读取 API Token，避免硬编码泄露到源码
fn get_api_token() -> String {
    std::env::var("API_TOKEN").unwrap_or_else(|_| {
        eprintln!("⚠️  警告: 未设置 API_TOKEN 环境变量，管理接口将不可用");
        String::new()
    })
}

// ── 前端 HTML（编译时 include）────────────────────────

const INDEX_HTML: &str = include_str!("index.html");

// ── 敏感信息脱敏 ─────────────────────────────────────

/// 脱敏并返回 (脱敏后文本, 替换次数)
/// 采用直接字符串扫描方式，避免实现完整正则引擎。
fn sanitize_count(text: &str) -> (String, usize) {
    if text.is_empty() {
        return (text.to_string(), 0);
    }

    let mut result = text.to_string();
    let mut total = 0;

    // 按优先级顺序应用各个脱敏规则
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
            // 查找结束标记
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
            // 匹配 eyJ 后跟至少 10 个 base64 字符，然后 .，再至少 10 个，再 .，再可选的
            let after = i + prefix.len();
            // 第一段
            let seg1_end = collect_while(&chars, after, &|c| {
                c.is_ascii_alphanumeric() || c == '_' || c == '-'
            });
            if seg1_end - after >= 10 && seg1_end < chars.len() && chars[seg1_end] == '.' {
                // 第二段
                let seg2_start = seg1_end + 1;
                let seg2_end = collect_while(&chars, seg2_start, &|c| {
                    c.is_ascii_alphanumeric() || c == '_' || c == '-'
                });
                if seg2_end - seg2_start >= 10 && seg2_end < chars.len() && chars[seg2_end] == '.'
                {
                    // 第三段（可选）
                    let seg3_start = seg2_end + 1;
                    let seg3_end = collect_while(&chars, seg3_start, &|c| {
                        c.is_ascii_alphanumeric() || c == '_' || c == '-'
                    });
                    // 至少要有点和后面内容
                    result.push_str("***REDACTED_JWT***");
                    i = if seg3_end > seg3_start {
                        seg3_end
                    } else {
                        seg2_end + 1 // 只有两个段 + 点
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
                // 收集后续的 base64 字符（至少 10 个才算）
                let after = i + pchars.len();
                let end = collect_while(&chars, after, &|c| {
                    c.is_ascii_alphanumeric() || c == '-' || c == '_'
                });
                // sk- 至少 20 个字符，ghp_ 至少 36 个，其他至少 10 个
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
/// 匹配: password=xxx, token: xxx, api_key=xxx, 密码=xxx, etc.
fn redact_key_value(text: &str) -> (String, usize) {
    // 敏感关键词列表（小写）
    let keywords: &[&str] = &[
        "password", "passwd", "pwd", "密码",
        "token", "apikey", "api_key", "api-key",
        "secret", "bearer", "authorization",
        "api_key", "private_key", "credential",
    ];

    let mut result = String::new();
    let mut count = 0;
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;

    while i < chars.len() {
        // 检查边界：要么是开头，要么前一个字符不是单词字符
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
                    // 确保后面不是单词字符（单词边界）
                    let after = i + kw_chars.len();
                    let boundary_ok = after >= chars.len() || !is_word_char(chars[after]);

                    // 特殊处理：api_key, api-key 后面可能有 _xxx 这种继续的
                    // 对于 PASSWORD/TOKEN 等环境变量形式，后面可以跟 _XXX
                    let env_vars = ["secret", "password", "token", "api_key",
                                    "private_key", "credential"];
                    if env_vars.contains(kw) {
                        // 允许后面跟 _\w*
                    }

                    if matches && (boundary_ok || env_vars.contains(kw)) {
                        found_keyword = Some(*kw);
                        break;
                    }
                }
            }

            if let Some(kw) = found_keyword {
                let kw_len = kw.chars().count();
                let after_kw = i + kw_len;

                // 允许环境变量扩展: SECRET_XXX, PASSWORD_XXX 等
                let mut key_end = after_kw;
                if chars.get(after_kw) == Some(&'_') {
                    key_end = collect_while(&chars, after_kw, &|c| is_word_char(c));
                }

                // 跳过空白
                let after_ws = skip_ws(&chars, key_end);

                // 检查后面是否是 : 或 =
                if after_ws < chars.len() && (chars[after_ws] == ':' || chars[after_ws] == '=') {
                    // 跳过 : 或 = 和后续空白
                    let after_sep = skip_ws(&chars, after_ws + 1);

                    // 收集值（非空白字符）
                    let val_end = collect_while(&chars, after_sep, &|c| is_non_space(c));

                    if val_end > after_sep {
                        // 替换: keyword=***REDACTED***
                        // 保留 keyword，替换 value
                        result.push_str(
                            &chars[i..key_end].iter().collect::<String>(),
                        );
                        // 保留中间的空白和分隔符
                        result.push_str(
                            &chars[key_end..after_sep].iter().collect::<String>(),
                        );
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
        // 查找 :// 模式
        if chars[i] == ':' && i + 2 < chars.len() && chars[i + 1] == '/' && chars[i + 2] == '/' {
            // :// 后面收集用户名部分（到 : 或 @ 或 / 为止）
            let after_slash = i + 3;
            let user_end = collect_while(&chars, after_slash, &|c| {
                !(c == ':' || c == '@' || c == '/' || c.is_whitespace())
            });

            // 必须有用户名
            if user_end > after_slash && user_end < chars.len() && chars[user_end] == ':' {
                // 收集密码部分（到 @ 为止）
                let pass_start = user_end + 1;
                let pass_end = collect_while(&chars, pass_start, &|c| {
                    !(c == '@' || c == '/' || c.is_whitespace())
                });

                if pass_end > pass_start && pass_end < chars.len() && chars[pass_end] == '@' {
                    // 匹配成功，替换密码部分
                    result.push_str(&chars[i..user_end + 1].iter().collect::<String>());
                    result.push_str("***REDACTED***");
                    result.push_str(&chars[pass_end..].iter().collect::<String>());
                    i = chars.len(); // 一次只处理一个
                    count += 1;
                    continue;
                }
            }
        }
        result.push(chars[i]);
        i += 1;
    }

    // 处理剩余字符
    while i < chars.len() {
        result.push(chars[i]);
        i += 1;
    }

    (result, count)
}

// ── HTTP 请求/响应 ──────────────────────────────────

struct Request {
    method: String,
    path: String,
    query: String,
    headers: Vec<(String, String)>,
    body: String,
}

impl Request {
    fn get_header(&self, name: &str) -> Option<&str> {
        for (k, v) in &self.headers {
            if k.eq_ignore_ascii_case(name) {
                return Some(v);
            }
        }
        None
    }

    fn query_param(&self, name: &str) -> Option<String> {
        if self.query.is_empty() {
            return None;
        }
        for pair in self.query.split('&') {
            let parts: Vec<&str> = pair.splitn(2, '=').collect();
            if parts[0] == name {
                return Some(parts.get(1).unwrap_or(&"").to_string());
            }
        }
        None
    }

    fn check_token(&self) -> bool {
        let token = get_api_token();
        if token.is_empty() { return false; }
        self.get_header("X-API-Token").unwrap_or("") == token
    }
}

/// 从 TcpStream 读取 HTTP 请求
fn read_request(stream: &mut TcpStream) -> Option<Request> {
    // 设置读取超时
    let _ = stream.set_read_timeout(Some(Duration::from_secs(10)));

    let mut buf = Vec::new();
    let mut chunk = [0u8; 4096];

    // 读取直到找到 \r\n\r\n (header 结束)
    loop {
        match stream.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => {
                buf.extend_from_slice(&chunk[..n]);
                // 检查是否已读完整 header
                if let Some(header_end) = find_subsequence(&buf, b"\r\n\r\n") {
                    // 如果有 Content-Length，继续读取 body
                    let header_str = String::from_utf8_lossy(&buf[..header_end]).to_string();
                    let content_length = parse_content_length(&header_str);

                    let body_start = header_end + 4;
                    while buf.len() < body_start + content_length {
                        match stream.read(&mut chunk) {
                            Ok(0) => break,
                            Ok(n) => buf.extend_from_slice(&chunk[..n]),
                            Err(_) => break,
                        }
                    }

                    return parse_request(&header_str, &buf, body_start, content_length);
                }
                // 防止过大 header
                if buf.len() > 65536 {
                    return None;
                }
            }
            Err(_) => return None,
        }
    }
    None
}

fn find_subsequence(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

fn parse_content_length(headers: &str) -> usize {
    for line in headers.lines() {
        if line.to_lowercase().starts_with("content-length:") {
            let val = line.split(':').nth(1).unwrap_or("0").trim();
            return val.parse().unwrap_or(0);
        }
    }
    0
}

fn parse_request(header_str: &str, buf: &[u8], body_start: usize, content_length: usize) -> Option<Request> {
    let mut lines = header_str.lines();
    let first_line = lines.next()?;

    let parts: Vec<&str> = first_line.split_whitespace().collect();
    if parts.len() < 2 {
        return None;
    }

    let method = parts[0].to_string();
    let raw_path = parts[1];

    // 分割 path 和 query
    let (path, query) = match raw_path.find('?') {
        Some(pos) => (raw_path[..pos].to_string(), raw_path[pos + 1..].to_string()),
        None => (raw_path.to_string(), String::new()),
    };

    // 解析 headers
    let mut headers = Vec::new();
    for line in lines {
        if let Some(pos) = line.find(':') {
            let key = line[..pos].trim().to_string();
            let val = line[pos + 1..].trim().to_string();
            headers.push((key, val));
        }
    }

    // 提取 body
    let body = if content_length > 0 && body_start + content_length <= buf.len() {
        String::from_utf8_lossy(&buf[body_start..body_start + content_length]).to_string()
    } else {
        String::new()
    };

    Some(Request {
        method,
        path,
        query,
        headers,
        body,
    })
}

/// 获取客户端 IP（支持 X-Forwarded-For）
fn get_client_ip(stream: &TcpStream, req: &Request) -> String {
    if let Some(fwd) = req.get_header("X-Forwarded-For") {
        return fwd.split(',').next().unwrap_or("").trim().to_string();
    }
    stream
        .peer_addr()
        .map(|addr| addr.ip().to_string())
        .unwrap_or_default()
}

// ── 响应工具 ────────────────────────────────────────

fn send_json(stream: &mut TcpStream, data: &str, status: u16, cache: Option<&str>) {
    let body = data.as_bytes();
    let status_text = status_text(status);
    let mut response = format!(
        "HTTP/1.1 {} {}\r\nContent-Type: application/json; charset=utf-8\r\nContent-Length: {}\r\nAccess-Control-Allow-Origin: *\r\n",
        status, status_text, body.len()
    );
    if let Some(c) = cache {
        response.push_str(&format!("Cache-Control: {}\r\n", c));
    } else {
        response.push_str("Cache-Control: no-cache\r\n");
    }
    response.push_str("Connection: close\r\n\r\n");

    let _ = stream.write_all(response.as_bytes());
    let _ = stream.write_all(body);
    let _ = stream.flush();
}

fn send_html(stream: &mut TcpStream, html: &str, status: u16, cache: Option<&str>) {
    let body = html.as_bytes();
    let status_text = status_text(status);
    let mut response = format!(
        "HTTP/1.1 {} {}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\n",
        status, status_text, body.len()
    );
    if let Some(c) = cache {
        response.push_str(&format!("Cache-Control: {}\r\n", c));
    }
    response.push_str("Access-Control-Allow-Origin: *\r\nConnection: close\r\n\r\n");

    let _ = stream.write_all(response.as_bytes());
    let _ = stream.write_all(body);
    let _ = stream.flush();
}

fn send_text(stream: &mut TcpStream, text: &str, status: u16, content_type: &str) {
    let body = text.as_bytes();
    let status_text = status_text(status);
    let response = format!(
        "HTTP/1.1 {} {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nAccess-Control-Allow-Origin: *\r\nConnection: close\r\n\r\n",
        status, status_text, content_type, body.len()
    );

    let _ = stream.write_all(response.as_bytes());
    let _ = stream.write_all(body);
    let _ = stream.flush();
}

fn send_cors_preflight(stream: &mut TcpStream) {
    let response = "HTTP/1.1 200 OK\r\nAccess-Control-Allow-Origin: *\r\nAccess-Control-Allow-Methods: GET, POST, DELETE, OPTIONS\r\nAccess-Control-Allow-Headers: Content-Type, X-API-Token\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
    let _ = stream.write_all(response.as_bytes());
    let _ = stream.flush();
}

fn status_text(status: u16) -> &'static str {
    match status {
        200 => "OK",
        201 => "Created",
        400 => "Bad Request",
        401 => "Unauthorized",
        404 => "Not Found",
        500 => "Internal Server Error",
        _ => "Unknown",
    }
}

// ── URL 解码 ────────────────────────────────────────

fn url_decode(s: &str) -> String {
    let mut result = String::new();
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '%' {
            let mut hex = String::new();
            if let Some(h1) = chars.next() {
                hex.push(h1);
            }
            if let Some(h2) = chars.next() {
                hex.push(h2);
            }
            if let Ok(code) = u32::from_str_radix(&hex, 16) {
                if let Some(decoded) = char::from_u32(code) {
                    result.push(decoded);
                }
            }
        } else if c == '+' {
            result.push(' ');
        } else {
            result.push(c);
        }
    }
    result
}

// ── 路由处理 ────────────────────────────────────────

fn handle_request(stream: &mut TcpStream, req: &Request, store: &Arc<Store>) {
    let ip = get_client_ip(stream, req);

    match req.method.as_str() {
        "OPTIONS" => {
            send_cors_preflight(stream);
        }
        "GET" => handle_get(stream, req, &ip, store),
        "POST" => handle_post(stream, req, &ip, store),
        "DELETE" => handle_delete(stream, req, store),
        _ => {
            send_text(stream, "Method Not Allowed", 405, "text/plain");
        }
    }
}

fn handle_get(stream: &mut TcpStream, req: &Request, ip: &str, store: &Arc<Store>) {
    let path = req.path.as_str();

    // 页面
    if path == "/" || path == "/index.html" {
        store.record_visit(ip);
        send_html(
            stream,
            INDEX_HTML,
            200,
            Some("public, max-age=300, s-maxage=600"),
        );
        return;
    }

    // 访问统计 API
    if path == "/api/stats" {
        let online = store.get_online_count();
        let total = store.get_total_visits();
        send_json(
            stream,
            &format!(r#"{{"online":{},"total_visits":{}}}"#, online, total),
            200,
            Some("no-cache"),
        );
        return;
    }

    if path == "/api/heartbeat" {
        store.record_heartbeat(ip);
        send_json(stream, r#"{"ok":true}"#, 200, Some("no-cache"));
        return;
    }

    // 健康检查
    if path == "/health" {
        send_json(stream, r#"{"status":"ok"}"#, 200, Some("no-cache"));
        return;
    }

    // 笔记标签列表
    if path == "/api/notes/tags" {
        let tags = store.get_all_tags();
        let parts: Vec<String> = tags
            .iter()
            .map(|t| format!("\"{}\"", store::json_escape(t)))
            .collect();
        send_json(
            stream,
            &format!("[{}]", parts.join(",")),
            200,
            Some("public, max-age=60, s-maxage=300"),
        );
        return;
    }

    // 笔记列表（支持 q 搜索、limit/offset 分页、tag/category 过滤、sort 排序）
    if path == "/api/notes" {
        let limit: usize = req
            .query_param("limit")
            .and_then(|s| s.parse().ok())
            .unwrap_or(50);
        let offset: usize = req
            .query_param("offset")
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);
        let tag = req.query_param("tag");
        let category = req.query_param("category");
        let q_raw = req.query_param("q").unwrap_or_default();
        let q = url_decode(q_raw.trim());
        let sort = req.query_param("sort").unwrap_or_default();
        let tag_ref = tag.as_deref();
        let cat_ref = category.as_deref();

        // 有搜索关键词时走搜索路径
        if !q.is_empty() {
            let notes = store.search_notes(&q, limit, offset, tag_ref, cat_ref);
            let parts: Vec<String> = notes.iter().map(|n| n.to_json()).collect();
            send_json(
                stream,
                &format!("[{}]", parts.join(",")),
                200,
                Some("public, max-age=30, s-maxage=60"),
            );
            return;
        }

        // 无搜索：带排序的列表 + 分页元信息
        let sort_str = if sort.is_empty() { "time" } else { sort.as_str() };
        let notes = store.get_notes_sorted(limit, offset, tag_ref, cat_ref, sort_str);
        let total = store.count_notes(tag_ref, cat_ref);
        let pages = if limit == 0 { 1 } else { (total + limit - 1) / limit };
        let page = if limit == 0 { 1 } else { (offset / limit) + 1 };
        let parts: Vec<String> = notes.iter().map(|n| n.to_json()).collect();
        send_json(
            stream,
            &format!(
                r#"{{"data":[{}],"total":{},"page":{},"pages":{}}}"#,
                parts.join(","),
                total,
                page,
                pages
            ),
            200,
            Some("public, max-age=60, s-maxage=300"),
        );
        return;
    }

    // 笔记导出（必须在 /api/notes/<id> 之前匹配）
    if path == "/api/notes/export" {
        let notes = store.get_all_notes_export();
        let parts: Vec<String> = notes.iter().map(|n| n.to_json()).collect();
        send_json(
            stream,
            &format!("[{}]", parts.join(",")),
            200,
            Some("public, max-age=60, s-maxage=300"),
        );
        return;
    }

    // 笔记子路径：/api/notes/<id>、/api/notes/<id>/prev|next|related
    if path.starts_with("/api/notes/") {
        let rest = &path["/api/notes/".len()..];
        // 可能是 "123" 或 "123/prev" 等
        let (id_part, sub) = match rest.find('/') {
            Some(pos) => (&rest[..pos], &rest[pos + 1..]),
            None => (rest, ""),
        };
        if let Ok(id) = id_part.parse::<i64>() {
            match sub {
                "" => {
                    // 笔记详情
                    if let Some(note) = store.get_note(id) {
                        send_json(
                            stream,
                            &note.to_json(),
                            200,
                            Some("public, max-age=120, s-maxage=600"),
                        );
                    } else {
                        send_json(stream, r#"{"error":"not found"}"#, 404, Some("no-cache"));
                    }
                    return;
                }
                "prev" => {
                    if let Some(note) = store.get_prev_note(id) {
                        send_json(stream, &note.to_json(), 200, Some("no-cache"));
                    } else {
                        send_json(stream, "null", 200, Some("no-cache"));
                    }
                    return;
                }
                "next" => {
                    if let Some(note) = store.get_next_note(id) {
                        send_json(stream, &note.to_json(), 200, Some("no-cache"));
                    } else {
                        send_json(stream, "null", 200, Some("no-cache"));
                    }
                    return;
                }
                "related" => {
                    let related = store.get_related_notes(id, 5);
                    let parts: Vec<String> = related.iter().map(|n| n.to_json_compact()).collect();
                    send_json(
                        stream,
                        &format!("[{}]", parts.join(",")),
                        200,
                        Some("public, max-age=120, s-maxage=600"),
                    );
                    return;
                }
                _ => {
                    send_json(stream, r#"{"error":"not found"}"#, 404, Some("no-cache"));
                    return;
                }
            }
        }
    }

    // RSS 订阅
    if path == "/rss.xml" {
        let xml = build_rss_xml(store);
        send_text(stream, &xml, 200, "application/rss+xml; charset=utf-8");
        return;
    }

    // 留言列表
    if path == "/api/guestbook" {
        let limit: usize = req
            .query_param("limit")
            .and_then(|s| s.parse().ok())
            .unwrap_or(20);
        let offset: usize = req
            .query_param("offset")
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);
        let entries = store.get_guestbook(limit, offset);
        let parts: Vec<String> = entries.iter().map(|g| g.to_json()).collect();
        send_json(
            stream,
            &format!("[{}]", parts.join(",")),
            200,
            Some("public, max-age=30, s-maxage=60"),
        );
        return;
    }

    // 作品集列表
    if path == "/api/portfolio" {
        let items = store.get_portfolio();
        let parts: Vec<String> = items.iter().map(|p| p.to_json()).collect();
        send_json(
            stream,
            &format!("[{}]", parts.join(",")),
            200,
            Some("public, max-age=300, s-maxage=600"),
        );
        return;
    }

    // 文档列表（可选 - 转发到共享文档平台，这里返回空列表）
    if path == "/api/documents" {
        send_json(stream, "[]", 200, Some("no-cache"));
        return;
    }

    // 404
    send_text(stream, "404 Not Found", 404, "text/plain");
}

fn handle_post(stream: &mut TcpStream, req: &Request, ip: &str, store: &Arc<Store>) {
    let path = req.path.as_str();

    // 笔记导入（批量）— 需要 Token
    if path == "/api/notes/import" {
        if !req.check_token() {
            send_json(stream, r#"{"error":"unauthorized"}"#, 401, Some("no-cache"));
            return;
        }
        match handle_import_notes(req, store) {
            Ok(result) => send_json(stream, &result, 200, Some("no-cache")),
            Err(e) => send_json(stream, &format!(r#"{{"error":"{}"}}"#, store::json_escape(&e)), 400, Some("no-cache")),
        }
        return;
    }

    // 笔记阅读次数 +1 — POST /api/notes/<id>/view
    if path.starts_with("/api/notes/") && path.ends_with("/view") {
        let rest = &path["/api/notes/".len()..path.len() - "/view".len()];
        if let Ok(id) = rest.parse::<i64>() {
            if store.increment_view(id) {
                send_json(stream, r#"{"ok":true}"#, 200, Some("no-cache"));
            } else {
                send_json(stream, r#"{"error":"not found"}"#, 404, Some("no-cache"));
            }
            return;
        }
    }

    // 创建笔记
    if path == "/api/notes" {
        if !req.check_token() {
            send_json(stream, r#"{"error":"unauthorized"}"#, 401, Some("no-cache"));
            return;
        }
        match handle_create_note(req, store) {
            Ok(result) => send_json(stream, &result, 200, Some("no-cache")),
            Err(e) => send_json(stream, &format!(r#"{{"error":"{}"}}"#, store::json_escape(&e)), 400, Some("no-cache")),
        }
        return;
    }

    // 创建留言
    if path == "/api/guestbook" {
        match handle_create_guestbook(req, ip, store) {
            Ok(result) => send_json(stream, &result, 200, Some("no-cache")),
            Err(e) => send_json(stream, &format!(r#"{{"error":"{}"}}"#, store::json_escape(&e)), 400, Some("no-cache")),
        }
        return;
    }

    // 创建作品
    if path == "/api/portfolio" {
        if !req.check_token() {
            send_json(stream, r#"{"error":"unauthorized"}"#, 401, Some("no-cache"));
            return;
        }
        match handle_create_portfolio(req, store) {
            Ok(result) => send_json(stream, &result, 200, Some("no-cache")),
            Err(e) => send_json(stream, &format!(r#"{{"error":"{}"}}"#, store::json_escape(&e)), 400, Some("no-cache")),
        }
        return;
    }

    send_json(stream, r#"{"error":"not found"}"#, 404, Some("no-cache"));
}

fn handle_delete(stream: &mut TcpStream, req: &Request, store: &Arc<Store>) {
    let path = req.path.as_str();

    if !req.check_token() {
        send_json(stream, r#"{"error":"unauthorized"}"#, 401, Some("no-cache"));
        return;
    }

    // 删除笔记（仅匹配纯数字 id，不匹配子路径如 /123/view）
    if path.starts_with("/api/notes/") {
        let id_str = &path["/api/notes/".len()..];
        // 确保是纯 id（无子路径）
        if !id_str.contains('/') {
            if let Ok(id) = id_str.parse::<i64>() {
                store.delete_note(id);
                send_json(stream, r#"{"status":"deleted"}"#, 200, Some("no-cache"));
                return;
            }
        }
    }

    // 删除作品
    if path.starts_with("/api/portfolio/") {
        let id_str = &path["/api/portfolio/".len()..];
        if let Ok(id) = id_str.parse::<i64>() {
            store.delete_portfolio(id);
            send_json(stream, r#"{"status":"deleted"}"#, 200, Some("no-cache"));
            return;
        }
    }

    send_json(stream, r#"{"error":"not found"}"#, 404, Some("no-cache"));
}

// ── POST 处理器 ─────────────────────────────────────

fn handle_create_note(req: &Request, store: &Arc<Store>) -> Result<String, String> {
    if req.body.is_empty() {
        return Err("empty body".to_string());
    }

    let parsed = JsonParser::parse(&req.body).map_err(|e| format!("invalid JSON: {}", e))?;

    let title = parsed.get("title").and_then(|v| v.as_str()).unwrap_or("");
    let content = parsed.get("content").and_then(|v| v.as_str()).unwrap_or("");
    let category = parsed
        .get("category")
        .and_then(|v| v.as_str())
        .unwrap_or("经验总结");
    let source = parsed.get("source").and_then(|v| v.as_str()).unwrap_or("hermes");

    let tags = match parsed.get("tags") {
        Some(JsonValue::Array(arr)) => arr
            .iter()
            .filter_map(|v| v.as_str().map(|s| s.to_string()))
            .collect(),
        _ => Vec::new(),
    };

    // 服务端脱敏
    let (sanitized_content, count) = sanitize_count(content);
    let (sanitized_title, _) = sanitize_count(title);

    let nid = store.create_note(
        &sanitized_title,
        &sanitized_content,
        tags,
        category,
        source,
        1,
    );

    let mut result = format!(r#"{{"id":{},"status":"created"}}"#, nid);
    if count >= 3 {
        result = format!(
            r#"{{"id":{},"status":"created","warning":"检测到 {} 处敏感信息已脱敏，建议人工检查"}}"#,
            nid, count
        );
    }
    Ok(result)
}

fn handle_create_guestbook(req: &Request, ip: &str, store: &Arc<Store>) -> Result<String, String> {
    if req.body.is_empty() {
        return Err("empty body".to_string());
    }

    let parsed = JsonParser::parse(&req.body).map_err(|e| format!("invalid JSON: {}", e))?;

    let name_raw = parsed.get("name").and_then(|v| v.as_str()).unwrap_or("");
    let message_raw = parsed.get("message").and_then(|v| v.as_str()).unwrap_or("");
    let contact_raw = parsed.get("contact").and_then(|v| v.as_str()).unwrap_or("");

    // trim and truncate
    let name: String = name_raw.trim().chars().take(30).collect();
    let message: String = message_raw.trim().chars().take(500).collect();
    let contact: String = contact_raw.trim().chars().take(50).collect();

    if name.is_empty() || message.is_empty() {
        return Err("name and message required".to_string());
    }

    let gid = store.create_guestbook_entry(&name, &message, &contact, ip);
    Ok(format!(r#"{{"id":{},"status":"created"}}"#, gid))
}

fn handle_create_portfolio(req: &Request, store: &Arc<Store>) -> Result<String, String> {
    if req.body.is_empty() {
        return Err("empty body".to_string());
    }

    let parsed = JsonParser::parse(&req.body).map_err(|e| format!("invalid JSON: {}", e))?;

    let title = parsed.get("title").and_then(|v| v.as_str()).unwrap_or("");
    let description = parsed
        .get("description")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let url = parsed.get("url").and_then(|v| v.as_str()).unwrap_or("");
    let repo_url = parsed
        .get("repo_url")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    let tech_stack = match parsed.get("tech_stack") {
        Some(JsonValue::Array(arr)) => arr
            .iter()
            .filter_map(|v| v.as_str().map(|s| s.to_string()))
            .collect(),
        _ => Vec::new(),
    };

    let sort_order = parsed
        .get("sort_order")
        .and_then(|v| {
            if let JsonValue::Number(n) = v {
                Some(*n as i64)
            } else {
                None
            }
        })
        .unwrap_or(0);

    let pid = store.create_portfolio(title, description, url, repo_url, tech_stack, sort_order);
    Ok(format!(r#"{{"id":{},"status":"created"}}"#, pid))
}

// ── RSS / 导入 辅助函数 ──────────────────────────────

/// 构建 RSS 2.0 XML（最近 20 条笔记）
fn build_rss_xml(store: &Arc<Store>) -> String {
    let notes = store.get_notes_sorted(20, 0, None, None, "time");
    let mut items: Vec<String> = Vec::new();
    for n in &notes {
        let link = format!("https://heronwang.cn/#/note/{}", n.id);
        // 描述取 content 前 200 字符
        let desc: String = n.content.chars().take(200).collect();
        items.push(format!(
            r#"    <item>
      <title>{}</title>
      <link>{}</link>
      <description>{}</description>
      <guid isPermaLink="false">note-{}</guid>
      <pubDate>{}</pubDate>
    </item>"#,
            xml_escape(&n.title),
            xml_escape(&link),
            xml_escape(&desc),
            n.id,
            xml_escape(&n.created_at),
        ));
    }
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<rss version="2.0">
  <channel>
    <title>Heron Wang 的笔记</title>
    <link>https://heronwang.cn</link>
    <description>个人笔记与经验总结</description>
    <language>zh-CN</language>
{}
  </channel>
</rss>"#,
        items.join("\n")
    )
}

/// XML 字符串转义
fn xml_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            c => out.push(c),
        }
    }
    out
}

/// 批量导入笔记
/// 接收格式: [{"title","content","tags","category"}, ...] 或 {"notes":[...]}
fn handle_import_notes(req: &Request, store: &Arc<Store>) -> Result<String, String> {
    if req.body.is_empty() {
        return Err("empty body".to_string());
    }

    let parsed = JsonParser::parse(&req.body).map_err(|e| format!("invalid JSON: {}", e))?;

    // 支持数组或 {notes:[...]} 两种格式
    let arr = match &parsed {
        JsonValue::Array(_) => &parsed,
        JsonValue::Object(_) => {
            match parsed.get("notes") {
                Some(v) if v.as_array().is_some() => v,
                _ => return Err("expected array or {notes: [...]}".to_string()),
            }
        }
        _ => return Err("expected array or {notes: [...]}".to_string()),
    };

    let items_arr = match arr.as_array() {
        Some(a) => a,
        None => return Err("invalid notes array".to_string()),
    };

    let mut items: Vec<(String, String, Vec<String>, String)> = Vec::new();
    for v in items_arr {
        let title = v.get("title").and_then(|x| x.as_str()).unwrap_or("");
        let content = v.get("content").and_then(|x| x.as_str()).unwrap_or("");
        let category = v
            .get("category")
            .and_then(|x| x.as_str())
            .unwrap_or("经验总结");

        let tags = match v.get("tags") {
            Some(JsonValue::Array(ta)) => ta
                .iter()
                .filter_map(|x| x.as_str().map(|s| s.to_string()))
                .collect(),
            _ => Vec::new(),
        };

        if title.is_empty() && content.is_empty() {
            continue; // 跳过空条目
        }

        // 导入时也做脱敏
        let (san_title, _) = sanitize_count(title);
        let (san_content, _) = sanitize_count(content);
        items.push((san_title, san_content, tags, category.to_string()));
    }

    if items.is_empty() {
        return Err("no valid notes to import".to_string());
    }

    let count = items.len();
    let ids = store.import_notes(&items);
    Ok(format!(
        r#"{{"status":"imported","count":{},"ids":[{}]}}"#,
        count,
        ids.iter().map(|i| i.to_string()).collect::<Vec<_>>().join(",")
    ))
}

// ── 主函数 ──────────────────────────────────────────

fn main() {
    // 初始化数据存储
    let store = Arc::new(Store::new(DATA_DIR));

    // 启动 TCP 监听
    let addr = format!("{}:{}", HOST, PORT);
    let listener = match TcpListener::bind(&addr) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("❌ 无法绑定 {} : {}", addr, e);
            std::process::exit(1);
        }
    };

    println!("🌐 主站已启动 (Rust): http://localhost:{}", PORT);
    println!("   监听: {}", addr);
    println!("   外网: https://heronwang.cn");
    println!("   API Token: 读取自环境变量 API_TOKEN");
    println!("{}", "-".repeat(50));

    // 多线程处理并发
    for incoming in listener.incoming() {
        match incoming {
            Ok(mut stream) => {
                let store = Arc::clone(&store);
                thread::spawn(move || {
                    if let Some(req) = read_request(&mut stream) {
                        handle_request(&mut stream, &req, &store);
                    }
                });
            }
            Err(e) => {
                eprintln!("⚠️ 连接错误: {}", e);
            }
        }
    }
}
