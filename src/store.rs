//! JSON 文件存储层 — Store 结构体定义与文件 I/O
//! 零第三方依赖，纯标准库实现。
//! 使用 Mutex 保证线程安全，每次写操作全量序列化到文件。

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::json::{JsonParser, JsonValue};
use crate::mdnotes::parse_markdown_note;
use crate::models::{GuestbookEntry, Note, PortfolioItem};
use crate::utils::{json_escape, now_iso};

// ── 存储 ──────────────────────────────────────────

/// 笔记文档目录（hermes-notes/doc/）
const NOTES_DOC_DIR: &str = "../hermes-notes/doc";

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

    // ── Notes: 从 hermes-notes/doc/ 加载 Markdown ──

    pub(crate) fn notes_path(&self) -> PathBuf {
        self.data_dir.join("notes.json")
    }

    /// 笔记文档目录路径（hermes-notes/doc/）
    fn notes_doc_dir(&self) -> PathBuf {
        PathBuf::from(NOTES_DOC_DIR)
    }

    /// 从 hermes-notes/doc/*.md 加载笔记，替代 notes.json
    pub(crate) fn load_notes(&self) {
        let dir = self.notes_doc_dir();
        if !dir.exists() {
            eprintln!("⚠️ 笔记目录不存在: {:?}, 回退到 notes.json", dir);
            self.load_notes_json();
            return;
        }

        let mut entries: Vec<(PathBuf, String)> = Vec::new();
        if let Ok(rd) = fs::read_dir(&dir) {
            for entry in rd.flatten() {
                let p = entry.path();
                if p.extension().and_then(|e| e.to_str()) == Some("md") {
                    if let Ok(content) = fs::read_to_string(&p) {
                        entries.push((p, content));
                    }
                }
            }
        }

        if entries.is_empty() {
            eprintln!("⚠️ 笔记目录为空: {:?}, 回退到 notes.json", dir);
            self.load_notes_json();
            return;
        }

        // 按文件名排序（YYMMDD_NNN_标题.md），确保顺序稳定
        entries.sort_by(|a, b| a.0.file_name().cmp(&b.0.file_name()));

        let mut notes = self.notes.lock().unwrap();
        notes.clear();
        for (i, (path, content)) in entries.iter().enumerate() {
            let id = (i + 1) as i64;
            let (title, tags, category, source, created_at, body) =
                parse_markdown_note(content, path);
            notes.push(Note {
                id,
                title,
                content: body,
                tags,
                category,
                source,
                sanitized: 1,
                view_count: 0,
                created_at: created_at.clone(),
                updated_at: created_at,
            });
        }
        println!("📝 加载 {} 条笔记 (from markdown)", notes.len());
    }

    /// 回退：从 notes.json 加载（兼容旧数据）
    fn load_notes_json(&self) {
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
                        println!("📝 加载 {} 条笔记 (from json fallback)", notes.len());
                    }
                    _ => eprintln!("⚠️ notes.json 格式错误，忽略"),
                }
            }
            Err(e) => eprintln!("⚠️ 读取 notes.json 失败: {}", e),
        }
    }

    pub(crate) fn save_notes(&self) {
        // Markdown 文件模式下不回写 notes.json
        // 笔记由 hermes-notes 仓库管理，主站只读
        let dir = self.notes_doc_dir();
        if dir.exists() {
            return; // markdown 模式，跳过 JSON 写入
        }
        // 回退模式：写 notes.json
        let notes = self.notes.lock().unwrap();
        let parts: Vec<String> = notes.iter().map(|n| n.to_json()).collect();
        let json = format!("[{}]", parts.join(",\n"));
        if let Err(e) = fs::write(self.notes_path(), &json) {
            eprintln!(
                "⚠️ 保存 notes.json 失败: {} (路径: {:?})",
                e,
                self.notes_path()
            );
        }
    }

    // ── Guestbook 文件 I/O ──────────────────────────

    pub(crate) fn guestbook_path(&self) -> PathBuf {
        self.data_dir.join("guestbook.json")
    }

    pub(crate) fn load_guestbook(&self) {
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

    pub(crate) fn save_guestbook(&self) {
        let gb = self.guestbook.lock().unwrap();
        let parts: Vec<String> = gb
            .iter()
            .map(|g| {
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
        if let Err(e) = fs::write(self.guestbook_path(), &json) {
            eprintln!("⚠️ 保存 guestbook.json 失败: {} (路径: {:?})", e, self.guestbook_path());
        }
    }

    // ── Portfolio 文件 I/O ──────────────────────────

    pub(crate) fn portfolio_path(&self) -> PathBuf {
        self.data_dir.join("portfolio.json")
    }

    pub(crate) fn load_portfolio(&self) {
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

    pub(crate) fn save_portfolio(&self) {
        let pf = self.portfolio.lock().unwrap();
        let parts: Vec<String> = pf.iter().map(|p| p.to_json()).collect();
        let json = format!("[{}]", parts.join(",\n"));
        if let Err(e) = fs::write(self.portfolio_path(), &json) {
            eprintln!("⚠️ 保存 portfolio.json 失败: {} (路径: {:?})", e, self.portfolio_path());
        }
    }

    // ── Visits 文件 I/O ─────────────────────────────

    pub(crate) fn visits_path(&self) -> PathBuf {
        self.data_dir.join("visits.json")
    }

    pub(crate) fn load_visits(&self) {
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

    pub(crate) fn save_visits(&self) {
        let tv = *self.total_visits.lock().unwrap();
        let json = format!(r#"{{"total_visits":{}}}"#, tv);
        if let Err(e) = fs::write(self.visits_path(), &json) {
            eprintln!("⚠️ 保存 visits.json 失败: {} (路径: {:?})", e, self.visits_path());
        }
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
        let timeout: u64 = 30;
        hb.retain(|_, t| now >= *t && now - *t <= timeout);
        hb.len()
    }

    pub fn get_total_visits(&self) -> i64 {
        *self.total_visits.lock().unwrap()
    }

    pub fn update_portfolio(
        &self,
        id: i64,
        title: Option<&str>,
        description: Option<&str>,
        url: Option<&str>,
        repo_url: Option<&str>,
        tech_stack: Option<Vec<String>>,
        sort_order: Option<i64>,
        updated_at: Option<&str>,
    ) -> bool {
        let mut pf = self.portfolio.lock().unwrap();
        let item = pf.iter_mut().find(|p| p.id == id);
        if item.is_none() {
            drop(pf);
            return false;
        }
        let it = item.unwrap();
        if let Some(t) = title {
            it.title = t.to_string();
        }
        if let Some(d) = description {
            it.description = d.to_string();
        }
        if let Some(u) = url {
            it.url = u.to_string();
        }
        if let Some(r) = repo_url {
            it.repo_url = r.to_string();
        }
        if let Some(ts) = tech_stack {
            it.tech_stack = ts;
        }
        if let Some(so) = sort_order {
            it.sort_order = so;
        }
        if let Some(ua) = updated_at {
            it.updated_at = ua.to_string();
        } else {
            it.updated_at = now_iso();
        }
        drop(pf);
        self.save_portfolio();
        true
    }
}

// ── Markdown 解析见 mdnotes.rs ────────────────────
