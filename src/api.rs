//! API 路由 — POST/DELETE 请求处理器（创建/删除/导入）

use std::net::TcpStream;
use std::sync::Arc;

use crate::handler::send_json;
use crate::json::{JsonParser, JsonValue};
use crate::redact::sanitize_count;
use crate::routes::Request;
use crate::store::Store;
use crate::utils::json_escape;

// ── POST 路由处理 ────────────────────────────────────

pub fn handle_post(stream: &mut TcpStream, req: &Request, ip: &str, store: &Arc<Store>) {
    let path = req.path.as_str();

    // 笔记导入（批量）— 需要 Token
    if path == "/api/notes/import" {
        if !req.check_token() {
            send_json(stream, r#"{"error":"unauthorized"}"#, 401, Some("no-cache"));
            return;
        }
        match handle_import_notes(req, store) {
            Ok(result) => send_json(stream, &result, 200, Some("no-cache")),
            Err(e) => send_json(
                stream,
                &format!(r#"{{"error":"{}"}}"#, json_escape(&e)),
                400,
                Some("no-cache"),
            ),
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
            Err(e) => send_json(
                stream,
                &format!(r#"{{"error":"{}"}}"#, json_escape(&e)),
                400,
                Some("no-cache"),
            ),
        }
        return;
    }

    // 创建留言
    if path == "/api/guestbook" {
        match handle_create_guestbook(req, ip, store) {
            Ok(result) => send_json(stream, &result, 200, Some("no-cache")),
            Err(e) => send_json(
                stream,
                &format!(r#"{{"error":"{}"}}"#, json_escape(&e)),
                400,
                Some("no-cache"),
            ),
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
            Err(e) => send_json(
                stream,
                &format!(r#"{{"error":"{}"}}"#, json_escape(&e)),
                400,
                Some("no-cache"),
            ),
        }
        return;
    }

    send_json(stream, r#"{"error":"not found"}"#, 404, Some("no-cache"));
}

// ── DELETE 路由处理 ──────────────────────────────────

pub fn handle_delete(stream: &mut TcpStream, req: &Request, store: &Arc<Store>) {
    let path = req.path.as_str();

    if !req.check_token() {
        send_json(stream, r#"{"error":"unauthorized"}"#, 401, Some("no-cache"));
        return;
    }

    // 删除笔记（仅匹配纯数字 id，不匹配子路径如 /123/view）
    if path.starts_with("/api/notes/") {
        let id_str = &path["/api/notes/".len()..];
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
    let source = parsed
        .get("source")
        .and_then(|v| v.as_str())
        .unwrap_or("hermes");

    let tags = match parsed.get("tags") {
        Some(JsonValue::Array(arr)) => arr
            .iter()
            .filter_map(|v| v.as_str().map(|s| s.to_string()))
            .collect(),
        _ => Vec::new(),
    };

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

/// 批量导入笔记
fn handle_import_notes(req: &Request, store: &Arc<Store>) -> Result<String, String> {
    if req.body.is_empty() {
        return Err("empty body".to_string());
    }

    let parsed = JsonParser::parse(&req.body).map_err(|e| format!("invalid JSON: {}", e))?;

    let arr = match &parsed {
        JsonValue::Array(_) => &parsed,
        JsonValue::Object(_) => match parsed.get("notes") {
            Some(v) if v.as_array().is_some() => v,
            _ => return Err("expected array or {notes: [...]}".to_string()),
        },
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
            continue;
        }

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
        ids.iter()
            .map(|i| i.to_string())
            .collect::<Vec<_>>()
            .join(",")
    ))
}
