//! JSON 文件存储层 — 笔记 / 留言 / 作品集 / 访问统计
//! 零第三方依赖，纯标准库实现。
//! 使用 Mutex 保证线程安全，每次写操作全量序列化到文件。

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

// ── 工具函数 ──────────────────────────────────────

/// 返回 ISO 8601 格式的时间戳字符串（如 2024-01-15T12:30:45）
pub fn now_iso() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    // 简单实现：将 epoch 秒转为 UTC 时间字符串
    epoch_to_iso(secs)
}

/// 将 epoch 秒转为 ISO 8601 字符串 (UTC)
fn epoch_to_iso(secs: u64) -> String {
    let days = secs / 86400;
    let remainder = secs % 86400;
    let hour = remainder / 3600;
    let min = (remainder % 3600) / 60;
    let sec = remainder % 60;

    // 从 1970-01-01 开始计算日期
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

// ── JSON 序列化（手写最小实现）──────────────────────

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
    let parts: Vec<String> = arr.iter().map(|s| format!("\"{}\"", json_escape(s))).collect();
    format!("[{}]", parts.join(","))
}

/// 将 i64 数组序列化为 JSON 数组字符串
pub fn json_int_array(arr: &[i64]) -> String {
    let parts: Vec<String> = arr.iter().map(|v| v.to_string()).collect();
    format!("[{}]", parts.join(","))
}

// ── JSON 解析（手写最小实现，仅支持我们需要的格式）─────

/// 简易 JSON 值枚举
#[derive(Debug, Clone)]
pub enum JsonValue {
    Null,
    Bool(bool),
    Number(f64),
    String(String),
    Array(Vec<JsonValue>),
    Object(Vec<(String, JsonValue)>),
}

impl JsonValue {
    pub fn as_str(&self) -> Option<&str> {
        match self {
            JsonValue::String(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_array(&self) -> Option<&Vec<JsonValue>> {
        match self {
            JsonValue::Array(a) => Some(a),
            _ => None,
        }
    }

    pub fn as_number(&self) -> Option<f64> {
        match self {
            JsonValue::Number(n) => Some(*n),
            _ => None,
        }
    }

    pub fn get(&self, key: &str) -> Option<&JsonValue> {
        match self {
            JsonValue::Object(obj) => {
                for (k, v) in obj {
                    if k == key {
                        return Some(v);
                    }
                }
                None
            }
            _ => None,
        }
    }
}

/// JSON 解析器
pub struct JsonParser<'a> {
    chars: Vec<char>,
    pos: usize,
    _lifetime: std::marker::PhantomData<&'a ()>,
}

impl<'a> JsonParser<'a> {
    pub fn new(input: &'a str) -> Self {
        JsonParser {
            chars: input.chars().collect(),
            pos: 0,
            _lifetime: std::marker::PhantomData,
        }
    }

    pub fn parse(input: &str) -> Result<JsonValue, String> {
        let mut parser = JsonParser {
            chars: input.chars().collect(),
            pos: 0,
            _lifetime: std::marker::PhantomData,
        };
        parser.skip_ws();
        let val = parser.parse_value()?;
        parser.skip_ws();
        Ok(val)
    }

    fn skip_ws(&mut self) {
        while self.pos < self.chars.len() {
            match self.chars[self.pos] {
                ' ' | '\t' | '\n' | '\r' => self.pos += 1,
                _ => break,
            }
        }
    }

    fn parse_value(&mut self) -> Result<JsonValue, String> {
        self.skip_ws();
        if self.pos >= self.chars.len() {
            return Err("unexpected end".to_string());
        }
        match self.chars[self.pos] {
            '"' => self.parse_string(),
            '{' => self.parse_object(),
            '[' => self.parse_array(),
            't' | 'f' => self.parse_bool(),
            'n' => self.parse_null(),
            '-' | '0'..='9' => self.parse_number(),
            _ => Err(format!("unexpected char: {}", self.chars[self.pos])),
        }
    }

    fn parse_string(&mut self) -> Result<JsonValue, String> {
        self.pos += 1; // skip opening "
        let mut out = String::new();
        while self.pos < self.chars.len() {
            match self.chars[self.pos] {
                '"' => {
                    self.pos += 1;
                    return Ok(JsonValue::String(out));
                }
                '\\' => {
                    self.pos += 1;
                    if self.pos >= self.chars.len() {
                        return Err("unterminated escape".to_string());
                    }
                    match self.chars[self.pos] {
                        '"' => out.push('"'),
                        '\\' => out.push('\\'),
                        '/' => out.push('/'),
                        'n' => out.push('\n'),
                        'r' => out.push('\r'),
                        't' => out.push('\t'),
                        'u' => {
                            // \uXXXX
                            let mut hex = String::new();
                            for _ in 0..4 {
                                self.pos += 1;
                                if self.pos < self.chars.len() {
                                    hex.push(self.chars[self.pos]);
                                }
                            }
                            if let Ok(code) = u32::from_str_radix(&hex, 16) {
                                if let Some(c) = char::from_u32(code) {
                                    out.push(c);
                                }
                            }
                        }
                        c => out.push(c),
                    }
                    self.pos += 1;
                }
                c => {
                    out.push(c);
                    self.pos += 1;
                }
            }
        }
        Err("unterminated string".to_string())
    }

    fn parse_object(&mut self) -> Result<JsonValue, String> {
        self.pos += 1; // skip {
        let mut items = Vec::new();
        self.skip_ws();
        if self.pos < self.chars.len() && self.chars[self.pos] == '}' {
            self.pos += 1;
            return Ok(JsonValue::Object(items));
        }
        loop {
            self.skip_ws();
            if self.pos >= self.chars.len() || self.chars[self.pos] != '"' {
                return Err("expected key string".to_string());
            }
            let key = match self.parse_string()? {
                JsonValue::String(s) => s,
                _ => return Err("key must be string".to_string()),
            };
            self.skip_ws();
            if self.pos >= self.chars.len() || self.chars[self.pos] != ':' {
                return Err("expected colon".to_string());
            }
            self.pos += 1;
            let val = self.parse_value()?;
            items.push((key, val));
            self.skip_ws();
            if self.pos >= self.chars.len() {
                return Err("unterminated object".to_string());
            }
            match self.chars[self.pos] {
                ',' => {
                    self.pos += 1;
                }
                '}' => {
                    self.pos += 1;
                    return Ok(JsonValue::Object(items));
                }
                _ => return Err(format!("expected , or }}: got {}", self.chars[self.pos])),
            }
        }
    }

    fn parse_array(&mut self) -> Result<JsonValue, String> {
        self.pos += 1; // skip [
        let mut items = Vec::new();
        self.skip_ws();
        if self.pos < self.chars.len() && self.chars[self.pos] == ']' {
            self.pos += 1;
            return Ok(JsonValue::Array(items));
        }
        loop {
            let val = self.parse_value()?;
            items.push(val);
            self.skip_ws();
            if self.pos >= self.chars.len() {
                return Err("unterminated array".to_string());
            }
            match self.chars[self.pos] {
                ',' => {
                    self.pos += 1;
                }
                ']' => {
                    self.pos += 1;
                    return Ok(JsonValue::Array(items));
                }
                _ => return Err(format!("expected , or ]: got {}", self.chars[self.pos])),
            }
        }
    }

    fn parse_bool(&mut self) -> Result<JsonValue, String> {
        if self.chars[self.pos..].starts_with(&['t', 'r', 'u', 'e']) {
            self.pos += 4;
            Ok(JsonValue::Bool(true))
        } else if self.chars[self.pos..].starts_with(&['f', 'a', 'l', 's', 'e']) {
            self.pos += 5;
            Ok(JsonValue::Bool(false))
        } else {
            Err("invalid bool".to_string())
        }
    }

    fn parse_null(&mut self) -> Result<JsonValue, String> {
        if self.chars[self.pos..].starts_with(&['n', 'u', 'l', 'l']) {
            self.pos += 4;
            Ok(JsonValue::Null)
        } else {
            Err("invalid null".to_string())
        }
    }

    fn parse_number(&mut self) -> Result<JsonValue, String> {
        let start = self.pos;
        if self.chars[self.pos] == '-' {
            self.pos += 1;
        }
        while self.pos < self.chars.len()
            && (self.chars[self.pos].is_ascii_digit()
                || self.chars[self.pos] == '.'
                || self.chars[self.pos] == 'e'
                || self.chars[self.pos] == 'E'
                || self.chars[self.pos] == '+'
                || self.chars[self.pos] == '-')
        {
            self.pos += 1;
        }
        let s: String = self.chars[start..self.pos].iter().collect();
        s.parse::<f64>()
            .map(JsonValue::Number)
            .map_err(|e| format!("invalid number: {}", e))
    }
}

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
            category: obj.get("category")?.as_str().unwrap_or("经验总结").to_string(),
            source: obj.get("source")?.as_str().unwrap_or("hermes").to_string(),
            sanitized: obj.get("sanitized")?.as_number().unwrap_or(1.0) as i64,
            view_count: obj.get("view_count").and_then(|v| v.as_number()).unwrap_or(0.0) as i64,
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

fn json_array_to_strings(v: &JsonValue) -> Vec<String> {
    match v.as_array() {
        Some(arr) => arr
            .iter()
            .filter_map(|item| item.as_str().map(|s| s.to_string()))
            .collect(),
        None => Vec::new(),
    }
}

// ── 存储 ──────────────────────────────────────────

pub struct Store {
    pub data_dir: PathBuf,
    pub notes: Arc<Mutex<Vec<Note>>>,
    pub guestbook: Arc<Mutex<Vec<GuestbookEntry>>>,
    pub portfolio: Arc<Mutex<Vec<PortfolioItem>>>,
    pub total_visits: Arc<Mutex<i64>>,
    pub heartbeats: Arc<Mutex<HashMap<String, u64>>>,
}

impl Store {
    pub fn new(data_dir: &str) -> Self {
        let dir = PathBuf::from(data_dir);
        fs::create_dir_all(&dir).ok();

        let store = Store {
            data_dir: dir,
            notes: Arc::new(Mutex::new(Vec::new())),
            guestbook: Arc::new(Mutex::new(Vec::new())),
            portfolio: Arc::new(Mutex::new(Vec::new())),
            total_visits: Arc::new(Mutex::new(0)),
            heartbeats: Arc::new(Mutex::new(HashMap::new())),
        };
        store.load_all();
        store
    }

    fn load_all(&self) {
        self.load_notes();
        self.load_guestbook();
        self.load_portfolio();
        self.load_visits();
    }

    // ── Notes ──────────────────────────────

    fn notes_path(&self) -> PathBuf {
        self.data_dir.join("notes.json")
    }

    fn load_notes(&self) {
        let path = self.notes_path();
        if !path.exists() {
            return;
        }
        match fs::read_to_string(&path) {
            Ok(content) => {
                if content.trim().is_empty() {
                    return;
                }
                match JsonParser::parse(&content) {
                    Ok(JsonValue::Array(arr)) => {
                        let mut notes = self.notes.lock().unwrap();
                        notes.clear();
                        for v in arr {
                            if let Some(note) = Note::from_json(&v) {
                                notes.push(note);
                            }
                        }
                        println!("📝 加载 {} 条笔记", notes.len());
                    }
                    _ => {
                        eprintln!("⚠️ notes.json 格式错误，忽略");
                    }
                }
            }
            Err(e) => eprintln!("⚠️ 读取 notes.json 失败: {}", e),
        }
    }

    fn save_notes(&self) {
        let notes = self.notes.lock().unwrap();
        let parts: Vec<String> = notes.iter().map(|n| n.to_json()).collect();
        let json = format!("[{}]", parts.join(",\n"));
        let _ = fs::write(self.notes_path(), json);
    }

    pub fn create_note(
        &self,
        title: &str,
        content: &str,
        tags: Vec<String>,
        category: &str,
        source: &str,
        sanitized: i64,
    ) -> i64 {
        let mut notes = self.notes.lock().unwrap();
        let id = notes.iter().map(|n| n.id).max().unwrap_or(0) + 1;
        let now = now_iso();
        notes.push(Note {
            id,
            title: title.to_string(),
            content: content.to_string(),
            tags,
            category: category.to_string(),
            source: source.to_string(),
            sanitized,
            view_count: 0,
            created_at: now.clone(),
            updated_at: now,
        });
        drop(notes);
        self.save_notes();
        id
    }

    pub fn get_notes(&self, limit: usize, offset: usize, tag: Option<&str>, category: Option<&str>) -> Vec<Note> {
        let notes = self.notes.lock().unwrap();
        let mut result: Vec<Note> = Vec::new();
        for n in notes.iter().rev() {
            // notes stored in creation order, but we want DESC by created_at
            if let Some(cat) = category {
                if n.category != cat {
                    continue;
                }
            }
            if let Some(t) = tag {
                if !n.tags.iter().any(|nt| nt == t) {
                    continue;
                }
            }
            result.push(n.clone());
        }
        // Apply offset and limit
        result.into_iter().skip(offset).take(limit).collect()
    }

    pub fn get_note(&self, id: i64) -> Option<Note> {
        let notes = self.notes.lock().unwrap();
        notes.iter().find(|n| n.id == id).cloned()
    }

    pub fn delete_note(&self, id: i64) -> bool {
        let mut notes = self.notes.lock().unwrap();
        let before = notes.len();
        notes.retain(|n| n.id != id);
        let deleted = notes.len() < before;
        drop(notes);
        if deleted {
            self.save_notes();
        }
        deleted
    }

    pub fn get_all_tags(&self) -> Vec<String> {
        let notes = self.notes.lock().unwrap();
        let mut tags: Vec<String> = Vec::new();
        for n in notes.iter() {
            for t in n.tags.iter() {
                if !tags.contains(t) {
                    tags.push(t.clone());
                }
            }
        }
        tags.sort();
        tags
    }

    // ── Notes: 搜索 / 分页 / 排序 / 导航 / 推荐 ────────

    /// 搜索笔记（title + content，不区分大小写），同时支持 tag/category 过滤。
    pub fn search_notes(
        &self,
        q: &str,
        limit: usize,
        offset: usize,
        tag: Option<&str>,
        category: Option<&str>,
    ) -> Vec<Note> {
        let notes = self.notes.lock().unwrap();
        let q_lower = q.to_lowercase();
        let mut result: Vec<Note> = Vec::new();
        for n in notes.iter().rev() {
            if let Some(cat) = category {
                if n.category != cat {
                    continue;
                }
            }
            if let Some(t) = tag {
                if !n.tags.iter().any(|nt| nt == t) {
                    continue;
                }
            }
            if !q_lower.is_empty() {
                let title_ok = n.title.to_lowercase().contains(&q_lower);
                let content_ok = n.content.to_lowercase().contains(&q_lower);
                if !title_ok && !content_ok {
                    continue;
                }
            }
            result.push(n.clone());
        }
        result.into_iter().skip(offset).take(limit).collect()
    }

    /// 返回 笔记列表（带排序）。sort: "time" | "title" | "views"
    pub fn get_notes_sorted(
        &self,
        limit: usize,
        offset: usize,
        tag: Option<&str>,
        category: Option<&str>,
        sort: &str,
    ) -> Vec<Note> {
        let notes = self.notes.lock().unwrap();
        let mut result: Vec<Note> = Vec::new();
        for n in notes.iter() {
            if let Some(cat) = category {
                if n.category != cat {
                    continue;
                }
            }
            if let Some(t) = tag {
                if !n.tags.iter().any(|nt| nt == t) {
                    continue;
                }
            }
            result.push(n.clone());
        }
        match sort {
            "title" => {
                // 标题字母序 ASC（不区分大小写）
                result.sort_by(|a, b| a.title.to_lowercase().cmp(&b.title.to_lowercase()));
            }
            "views" => {
                // 阅读次数降序，相同则按 id 降序
                result.sort_by(|a, b| {
                    match b.view_count.cmp(&a.view_count) {
                        std::cmp::Ordering::Equal => b.id.cmp(&a.id),
                        other => other,
                    }
                });
            }
            _ => {
                // time（默认）：按 id 降序（即创建顺序倒序）
                result.sort_by(|a, b| b.id.cmp(&a.id));
            }
        }
        result.into_iter().skip(offset).take(limit).collect()
    }

    /// 统计 笔记总数（带 tag/category 过滤）
    pub fn count_notes(&self, tag: Option<&str>, category: Option<&str>) -> usize {
        let notes = self.notes.lock().unwrap();
        notes
            .iter()
            .filter(|n| {
                if let Some(cat) = category {
                    if n.category != cat {
                        return false;
                    }
                }
                if let Some(t) = tag {
                    if !n.tags.iter().any(|nt| nt == t) {
                        return false;
                    }
                }
                true
            })
            .count()
    }

    /// 阅读次数 +1
    pub fn increment_view(&self, id: i64) -> bool {
        let mut notes = self.notes.lock().unwrap();
        for n in notes.iter_mut() {
            if n.id == id {
                n.view_count += 1;
                drop(notes);
                self.save_notes();
                return true;
            }
        }
        false
    }

    /// 上一篇笔记（id 小于当前的最大 id）
    pub fn get_prev_note(&self, id: i64) -> Option<Note> {
        let notes = self.notes.lock().unwrap();
        notes
            .iter()
            .filter(|n| n.id < id)
            .max_by_key(|n| n.id)
            .cloned()
    }

    /// 下一篇笔记（id 大于当前的最小 id）
    pub fn get_next_note(&self, id: i64) -> Option<Note> {
        let notes = self.notes.lock().unwrap();
        notes
            .iter()
            .filter(|n| n.id > id)
            .min_by_key(|n| n.id)
            .cloned()
    }

    /// 相关推荐：同标签的其他笔记（最多 5 条），无则同 category
    pub fn get_related_notes(&self, id: i64, max: usize) -> Vec<Note> {
        let notes = self.notes.lock().unwrap();
        let current = match notes.iter().find(|n| n.id == id) {
            Some(n) => n.clone(),
            None => return Vec::new(),
        };

        // 1. 同标签的其他笔记
        let mut related: Vec<Note> = notes
            .iter()
            .filter(|n| {
                n.id != id && n.tags.iter().any(|t| current.tags.contains(t))
            })
            .cloned()
            .collect();

        // 如果同标签不足，补充同 category 的
        if related.len() < max {
            for n in notes.iter() {
                if related.len() >= max {
                    break;
                }
                if n.id != id
                    && n.category == current.category
                    && !related.iter().any(|r| r.id == n.id)
                {
                    related.push(n.clone());
                }
            }
        }

        related.truncate(max);
        // 按 id 降序
        related.sort_by(|a, b| b.id.cmp(&a.id));
        related
    }

    /// 导出所有笔记（含全文，按 id 升序）
    pub fn get_all_notes_export(&self) -> Vec<Note> {
        let notes = self.notes.lock().unwrap();
        let mut result: Vec<Note> = notes.clone();
        result.sort_by(|a, b| a.id.cmp(&b.id));
        result
    }

    /// 批量导入笔记（返回创建的 id 列表）
    pub fn import_notes(&self, items: &[(String, String, Vec<String>, String)]) -> Vec<i64> {
        let mut notes = self.notes.lock().unwrap();
        let mut next_id = notes.iter().map(|n| n.id).max().unwrap_or(0) + 1;
        let now = now_iso();
        let mut ids = Vec::new();
        for (title, content, tags, category) in items {
            notes.push(Note {
                id: next_id,
                title: title.clone(),
                content: content.clone(),
                tags: tags.clone(),
                category: if category.is_empty() {
                    "经验总结".to_string()
                } else {
                    category.clone()
                },
                source: "import".to_string(),
                sanitized: 1,
                view_count: 0,
                created_at: now.clone(),
                updated_at: now.clone(),
            });
            ids.push(next_id);
            next_id += 1;
        }
        drop(notes);
        self.save_notes();
        ids
    }

    // ── Guestbook ─────────────────────────

    fn guestbook_path(&self) -> PathBuf {
        self.data_dir.join("guestbook.json")
    }

    fn load_guestbook(&self) {
        let path = self.guestbook_path();
        if !path.exists() {
            return;
        }
        match fs::read_to_string(&path) {
            Ok(content) => {
                if content.trim().is_empty() {
                    return;
                }
                match JsonParser::parse(&content) {
                    Ok(JsonValue::Array(arr)) => {
                        let mut gb = self.guestbook.lock().unwrap();
                        gb.clear();
                        for v in arr {
                            if let Some(entry) = GuestbookEntry::from_json(&v) {
                                gb.push(entry);
                            }
                        }
                        println!("💬 加载 {} 条留言", gb.len());
                    }
                    _ => eprintln!("⚠️ guestbook.json 格式错误，忽略"),
                }
            }
            Err(e) => eprintln!("⚠️ 读取 guestbook.json 失败: {}", e),
        }
    }

    fn save_guestbook(&self) {
        let gb = self.guestbook.lock().unwrap();
        let parts: Vec<String> = gb
            .iter()
            .map(|g| {
                // 保存完整字段（含 ip, is_approved）
                format!(
                    r#"{{"id":{},"name":"{}","message":"{}","contact":"{}","ip":"{}","created_at":"{}","is_approved":{}}}"#,
                    g.id,
                    json_escape(&g.name),
                    json_escape(&g.message),
                    json_escape(&g.contact),
                    json_escape(&g.ip),
                    json_escape(&g.created_at),
                    g.is_approved,
                )
            })
            .collect();
        let json = format!("[{}]", parts.join(",\n"));
        let _ = fs::write(self.guestbook_path(), json);
    }

    pub fn create_guestbook_entry(
        &self,
        name: &str,
        message: &str,
        contact: &str,
        ip: &str,
    ) -> i64 {
        let mut gb = self.guestbook.lock().unwrap();
        let id = gb.iter().map(|g| g.id).max().unwrap_or(0) + 1;
        let now = now_iso();
        gb.push(GuestbookEntry {
            id,
            name: name.to_string(),
            message: message.to_string(),
            contact: contact.to_string(),
            ip: ip.to_string(),
            created_at: now,
            is_approved: 1,
        });
        drop(gb);
        self.save_guestbook();
        id
    }

    pub fn get_guestbook(&self, limit: usize, offset: usize) -> Vec<GuestbookEntry> {
        let gb = self.guestbook.lock().unwrap();
        // 只返回 is_approved=1 的，按时间倒序
        let result: Vec<GuestbookEntry> = gb
            .iter()
            .filter(|g| g.is_approved == 1)
            .rev()
            .cloned()
            .collect();
        result.into_iter().skip(offset).take(limit).collect()
    }

    // ── Portfolio ─────────────────────────

    fn portfolio_path(&self) -> PathBuf {
        self.data_dir.join("portfolio.json")
    }

    fn load_portfolio(&self) {
        let path = self.portfolio_path();
        if !path.exists() {
            return;
        }
        match fs::read_to_string(&path) {
            Ok(content) => {
                if content.trim().is_empty() {
                    return;
                }
                match JsonParser::parse(&content) {
                    Ok(JsonValue::Array(arr)) => {
                        let mut pf = self.portfolio.lock().unwrap();
                        pf.clear();
                        for v in arr {
                            if let Some(item) = PortfolioItem::from_json(&v) {
                                pf.push(item);
                            }
                        }
                        println!("🚀 加载 {} 个作品", pf.len());
                    }
                    _ => eprintln!("⚠️ portfolio.json 格式错误，忽略"),
                }
            }
            Err(e) => eprintln!("⚠️ 读取 portfolio.json 失败: {}", e),
        }
    }

    fn save_portfolio(&self) {
        let pf = self.portfolio.lock().unwrap();
        let parts: Vec<String> = pf.iter().map(|p| p.to_json()).collect();
        let json = format!("[{}]", parts.join(",\n"));
        let _ = fs::write(self.portfolio_path(), json);
    }

    pub fn create_portfolio(
        &self,
        title: &str,
        description: &str,
        url: &str,
        repo_url: &str,
        tech_stack: Vec<String>,
        sort_order: i64,
    ) -> i64 {
        let mut pf = self.portfolio.lock().unwrap();
        let id = pf.iter().map(|p| p.id).max().unwrap_or(0) + 1;
        let now = now_iso();
        pf.push(PortfolioItem {
            id,
            title: title.to_string(),
            description: description.to_string(),
            url: url.to_string(),
            repo_url: repo_url.to_string(),
            tech_stack,
            sort_order,
            created_at: now,
        });
        drop(pf);
        self.save_portfolio();
        id
    }

    pub fn get_portfolio(&self) -> Vec<PortfolioItem> {
        let pf = self.portfolio.lock().unwrap();
        let mut result: Vec<PortfolioItem> = pf.clone();
        // 按 sort_order ASC, created_at DESC 排序
        result.sort_by(|a, b| {
            match a.sort_order.cmp(&b.sort_order) {
                std::cmp::Ordering::Equal => b.created_at.cmp(&a.created_at),
                other => other,
            }
        });
        result
    }

    pub fn delete_portfolio(&self, id: i64) -> bool {
        let mut pf = self.portfolio.lock().unwrap();
        let before = pf.len();
        pf.retain(|p| p.id != id);
        let deleted = pf.len() < before;
        drop(pf);
        if deleted {
            self.save_portfolio();
        }
        deleted
    }

    // ── Visits ────────────────────────────

    fn visits_path(&self) -> PathBuf {
        self.data_dir.join("visits.json")
    }

    fn load_visits(&self) {
        let path = self.visits_path();
        if !path.exists() {
            return;
        }
        match fs::read_to_string(&path) {
            Ok(content) => {
                if let Ok(JsonValue::Object(obj)) = JsonParser::parse(&content) {
                    for (k, v) in &obj {
                        if k == "total_visits" {
                            if let JsonValue::Number(n) = v {
                                *self.total_visits.lock().unwrap() = *n as i64;
                            }
                        }
                    }
                    println!("📊 累计访问量: {}", *self.total_visits.lock().unwrap());
                }
            }
            Err(e) => eprintln!("⚠️ 读取 visits.json 失败: {}", e),
        }
    }

    fn save_visits(&self) {
        let tv = *self.total_visits.lock().unwrap();
        let json = format!(r#"{{"total_visits":{}}}"#, tv);
        let _ = fs::write(self.visits_path(), json);
    }

    pub fn record_visit(&self, ip: &str) {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        {
            let mut hb = self.heartbeats.lock().unwrap();
            hb.insert(ip.to_string(), now);
        }
        {
            let mut tv = self.total_visits.lock().unwrap();
            *tv += 1;
        }
        self.save_visits();
    }

    pub fn record_heartbeat(&self, ip: &str) {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let mut hb = self.heartbeats.lock().unwrap();
        hb.insert(ip.to_string(), now);
    }

    pub fn get_online_count(&self) -> usize {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let mut hb = self.heartbeats.lock().unwrap();
        // 清理过期心跳（30秒）
        let timeout: u64 = 30;
        hb.retain(|_, t| now >= *t && now - *t <= timeout);
        hb.len()
    }

    pub fn get_total_visits(&self) -> i64 {
        *self.total_visits.lock().unwrap()
    }
}
