//! 路由层 — Request 结构体、请求读取、GET 路由处理

use std::io::Read;
use std::net::TcpStream;
use std::sync::Arc;
use std::time::Duration;

use crate::config::get_api_token;
use crate::handler::{build_rss_xml, send_html, send_json, send_text, url_decode};
use crate::store::Store;
use crate::utils::json_escape;

pub const INDEX_HTML: &str = include_str!("index.html");
pub const FAVICON_SVG: &str = include_str!("../static/favicon.svg");

// ── HTTP 请求结构 ──────────────────────────────────

pub struct Request {
    pub method: String,
    pub path: String,
    pub query: String,
    pub headers: Vec<(String, String)>,
    pub body: String,
}

impl Request {
    pub fn get_header(&self, name: &str) -> Option<&str> {
        for (k, v) in &self.headers {
            if k.eq_ignore_ascii_case(name) {
                return Some(v);
            }
        }
        None
    }

    pub fn query_param(&self, name: &str) -> Option<String> {
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

    pub fn check_token(&self) -> bool {
        let token = get_api_token();
        if token.is_empty() {
            return false;
        }
        self.get_header("X-API-Token").unwrap_or("") == token
    }
}

/// 从 TcpStream 读取 HTTP 请求
pub fn read_request(stream: &mut TcpStream) -> Option<Request> {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(10)));

    let mut buf = Vec::new();
    let mut chunk = [0u8; 4096];

    loop {
        match stream.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => {
                buf.extend_from_slice(&chunk[..n]);
                if let Some(header_end) = find_subsequence(&buf, b"\r\n\r\n") {
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
                if buf.len() > 65536 {
                    return None;
                }
            }
            Err(_) => return None,
        }
    }
    None
}

pub fn find_subsequence(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

pub fn parse_content_length(headers: &str) -> usize {
    for line in headers.lines() {
        if line.to_lowercase().starts_with("content-length:") {
            let val = line.split(':').nth(1).unwrap_or("0").trim();
            return val.parse().unwrap_or(0);
        }
    }
    0
}

fn parse_request(
    header_str: &str,
    buf: &[u8],
    body_start: usize,
    content_length: usize,
) -> Option<Request> {
    let mut lines = header_str.lines();
    let first_line = lines.next()?;

    let parts: Vec<&str> = first_line.split_whitespace().collect();
    if parts.len() < 2 {
        return None;
    }

    let method = parts[0].to_string();
    let raw_path = parts[1];

    let (path, query) = match raw_path.find('?') {
        Some(pos) => (raw_path[..pos].to_string(), raw_path[pos + 1..].to_string()),
        None => (raw_path.to_string(), String::new()),
    };

    let mut headers = Vec::new();
    for line in lines {
        if let Some(pos) = line.find(':') {
            let key = line[..pos].trim().to_string();
            let val = line[pos + 1..].trim().to_string();
            headers.push((key, val));
        }
    }

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

// ── GET 路由处理 ────────────────────────────────────

pub fn handle_get(stream: &mut TcpStream, req: &Request, ip: &str, store: &Arc<Store>) {
    let path = req.path.as_str();

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

    if path == "/health" {
        send_json(stream, r#"{"status":"ok"}"#, 200, Some("no-cache"));
        return;
    }
    if path == "/favicon.svg" {
        send_text(stream, FAVICON_SVG, 200, "image/svg+xml");
        return;
    }

    if path == "/api/notes/tags" {
        let tags = store.get_all_tags();
        let parts: Vec<String> = tags
            .iter()
            .map(|t| format!("\"{}\"", json_escape(t)))
            .collect();
        send_json(
            stream,
            &format!("[{}]", parts.join(",")),
            200,
            Some("public, max-age=60, s-maxage=300"),
        );
        return;
    }

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

        let sort_str = if sort.is_empty() {
            "time"
        } else {
            sort.as_str()
        };
        let notes = store.get_notes_sorted(limit, offset, tag_ref, cat_ref, sort_str);
        let total = store.count_notes(tag_ref, cat_ref);
        let pages = if limit == 0 {
            1
        } else {
            (total + limit - 1) / limit
        };
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

    if path.starts_with("/api/notes/") {
        let rest = &path["/api/notes/".len()..];
        let (id_part, sub) = match rest.find('/') {
            Some(pos) => (&rest[..pos], &rest[pos + 1..]),
            None => (rest, ""),
        };
        if let Ok(id) = id_part.parse::<i64>() {
            match sub {
                "" => {
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

    if path == "/rss.xml" {
        let xml = build_rss_xml(store);
        send_text(stream, &xml, 200, "application/rss+xml; charset=utf-8");
        return;
    }

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

    if path == "/api/documents" {
        send_json(stream, "[]", 200, Some("no-cache"));
        return;
    }

    send_text(stream, "404 Not Found", 404, "text/plain");
}
