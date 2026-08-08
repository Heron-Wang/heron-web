//! 数据模型 — 笔记 / 留言 / 作品集
//! 包含结构体定义和 JSON 序列化/反序列化方法。

use crate::json::{json_array_to_strings, JsonValue};
use crate::utils::{json_escape, json_string_array};

// ── 数据模型 ──────────────────────────────────────

#[derive(Debug, Clone)]
pub struct Note {
    pub id: i64,
    pub title: String,
    pub content: String,
    pub tags: Vec<String>,
    pub category: String,
    pub source: String,
    pub sanitized: i64,
    pub view_count: i64,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone)]
pub struct GuestbookEntry {
    pub id: i64,
    pub name: String,
    pub message: String,
    pub contact: String,
    pub ip: String,
    pub created_at: String,
    pub is_approved: i64,
}

#[derive(Debug, Clone)]
pub struct PortfolioItem {
    pub id: i64,
    pub title: String,
    pub description: String,
    pub url: String,
    pub repo_url: String,
    pub tech_stack: Vec<String>,
    pub sort_order: i64,
    pub created_at: String,
}

// ── 序列化 ────────────────────────────────────────

impl Note {
    pub fn to_json(&self) -> String {
        format!(
            r#"{{"id":{},"title":"{}","content":"{}","tags":{},"category":"{}","source":"{}","sanitized":{},"view_count":{},"created_at":"{}","updated_at":"{}"}}"#,
            self.id,
            json_escape(&self.title),
            json_escape(&self.content),
            json_string_array(&self.tags),
            json_escape(&self.category),
            json_escape(&self.source),
            self.sanitized,
            self.view_count,
            json_escape(&self.created_at),
            json_escape(&self.updated_at),
        )
    }

    /// 精简 JSON（用于相关推荐等场景，只含 id/title/tags）
    pub fn to_json_compact(&self) -> String {
        format!(
            r#"{{"id":{},"title":"{}","tags":{}}}"#,
            self.id,
            json_escape(&self.title),
            json_string_array(&self.tags),
        )
    }

    pub fn from_json(v: &JsonValue) -> Option<Self> {
        let obj = match v {
            JsonValue::Object(_) => v,
            _ => return None,
        };
        Some(Note {
            id: obj.get("id")?.as_number()? as i64,
            title: obj.get("title")?.as_str()?.to_string(),
            content: obj.get("content")?.as_str()?.to_string(),
            tags: json_array_to_strings(obj.get("tags")?),
            category: obj
                .get("category")?
                .as_str()
                .unwrap_or("经验总结")
                .to_string(),
            source: obj.get("source")?.as_str().unwrap_or("hermes").to_string(),
            sanitized: obj.get("sanitized")?.as_number().unwrap_or(1.0) as i64,
            view_count: obj
                .get("view_count")
                .and_then(|v| v.as_number())
                .unwrap_or(0.0) as i64,
            created_at: obj.get("created_at")?.as_str().unwrap_or("").to_string(),
            updated_at: obj.get("updated_at")?.as_str().unwrap_or("").to_string(),
        })
    }
}

impl GuestbookEntry {
    pub fn to_json(&self) -> String {
        // 列表 API 不返回 ip 和 is_approved（与 Python 版一致）
        format!(
            r#"{{"id":{},"name":"{}","message":"{}","contact":"{}","created_at":"{}"}}"#,
            self.id,
            json_escape(&self.name),
            json_escape(&self.message),
            json_escape(&self.contact),
            json_escape(&self.created_at),
        )
    }

    pub fn from_json(v: &JsonValue) -> Option<Self> {
        let obj = match v {
            JsonValue::Object(_) => v,
            _ => return None,
        };
        Some(GuestbookEntry {
            id: obj.get("id")?.as_number()? as i64,
            name: obj.get("name")?.as_str()?.to_string(),
            message: obj.get("message")?.as_str()?.to_string(),
            contact: obj.get("contact")?.as_str().unwrap_or("").to_string(),
            ip: obj.get("ip")?.as_str().unwrap_or("").to_string(),
            created_at: obj.get("created_at")?.as_str().unwrap_or("").to_string(),
            is_approved: obj.get("is_approved")?.as_number().unwrap_or(1.0) as i64,
        })
    }
}

impl PortfolioItem {
    pub fn to_json(&self) -> String {
        format!(
            r#"{{"id":{},"title":"{}","description":"{}","url":"{}","repo_url":"{}","tech_stack":{},"sort_order":{},"created_at":"{}"}}"#,
            self.id,
            json_escape(&self.title),
            json_escape(&self.description),
            json_escape(&self.url),
            json_escape(&self.repo_url),
            json_string_array(&self.tech_stack),
            self.sort_order,
            json_escape(&self.created_at),
        )
    }

    pub fn from_json(v: &JsonValue) -> Option<Self> {
        let obj = match v {
            JsonValue::Object(_) => v,
            _ => return None,
        };
        Some(PortfolioItem {
            id: obj.get("id")?.as_number()? as i64,
            title: obj.get("title")?.as_str()?.to_string(),
            description: obj.get("description")?.as_str().unwrap_or("").to_string(),
            url: obj.get("url")?.as_str().unwrap_or("").to_string(),
            repo_url: obj.get("repo_url")?.as_str().unwrap_or("").to_string(),
            tech_stack: json_array_to_strings(obj.get("tech_stack")?),
            sort_order: obj.get("sort_order")?.as_number().unwrap_or(0.0) as i64,
            created_at: obj.get("created_at")?.as_str().unwrap_or("").to_string(),
        })
    }
}
