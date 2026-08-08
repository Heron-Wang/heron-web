//! HTTP handler — 连接处理、请求分发、响应工具

use std::io::Write;
use std::net::TcpStream;
use std::sync::Arc;

use crate::api::{handle_delete, handle_post, handle_put};
use crate::routes::{handle_get, Request};
use crate::store::Store;

/// 处理一个完整的 HTTP 请求（按 method 分发）
pub fn handle_request(stream: &mut TcpStream, req: &Request, store: &Arc<Store>) {
    let ip = get_client_ip(stream, req);

    match req.method.as_str() {
        "OPTIONS" => {
            send_cors_preflight(stream);
        }
        "GET" => handle_get(stream, req, &ip, store),
        "POST" => handle_post(stream, req, &ip, store),
        "PUT" => handle_put(stream, req, store),
        "DELETE" => handle_delete(stream, req, store),
        _ => {
            send_text(stream, "Method Not Allowed", 405, "text/plain");
        }
    }
}

/// 获取客户端 IP（支持 X-Forwarded-For）
pub fn get_client_ip(stream: &TcpStream, req: &Request) -> String {
    if let Some(fwd) = req.get_header("X-Forwarded-For") {
        return fwd.split(',').next().unwrap_or("").trim().to_string();
    }
    stream
        .peer_addr()
        .map(|addr| addr.ip().to_string())
        .unwrap_or_default()
}

// ── 响应工具 ────────────────────────────────────────

pub fn send_json(stream: &mut TcpStream, data: &str, status: u16, cache: Option<&str>) {
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

pub fn send_html(stream: &mut TcpStream, html: &str, status: u16, cache: Option<&str>) {
    let body = html.as_bytes();
    let status_text = status_text(status);
    let mut response = format!(
        "HTTP/1.1 {} {}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\n",
        status,
        status_text,
        body.len()
    );
    if let Some(c) = cache {
        response.push_str(&format!("Cache-Control: {}\r\n", c));
    }
    response.push_str("Access-Control-Allow-Origin: *\r\nConnection: close\r\n\r\n");

    let _ = stream.write_all(response.as_bytes());
    let _ = stream.write_all(body);
    let _ = stream.flush();
}

pub fn send_text(stream: &mut TcpStream, text: &str, status: u16, content_type: &str) {
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

pub fn send_cors_preflight(stream: &mut TcpStream) {
    let response = "HTTP/1.1 200 OK\r\nAccess-Control-Allow-Origin: *\r\nAccess-Control-Allow-Methods: GET, POST, DELETE, OPTIONS\r\nAccess-Control-Allow-Headers: Content-Type, X-API-Token\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
    let _ = stream.write_all(response.as_bytes());
    let _ = stream.flush();
}

pub fn status_text(status: u16) -> &'static str {
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

/// URL 解码
pub fn url_decode(s: &str) -> String {
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

/// XML 字符串转义
pub fn xml_escape(s: &str) -> String {
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

/// 构建 RSS 2.0 XML（最近 20 条笔记）
pub fn build_rss_xml(store: &Arc<Store>) -> String {
    let notes = store.get_notes_sorted(20, 0, None, None, "time");
    let mut items: Vec<String> = Vec::new();
    for n in &notes {
        let link = format!("https://heronwang.cn/#/note/{}", n.id);
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
