//! Store 业务逻辑 — CRUD 操作、搜索、排序、导航、推荐
//! 这些方法操作 Store 内部数据并触发文件持久化。

use std::sync::Arc;

use crate::models::{Note, PortfolioItem};
use crate::store::Store;
use crate::utils::now_iso;

impl Store {
    // ── Notes: 热重载 ─────────────────────────────

    /// 检测 doc/ 目录 .md 文件数变化，变化则自动重载（无需重启）
    pub fn check_reload_notes(&self) {
        let dir = self.notes_doc_dir();
        if !dir.exists() {
            return;
        }
        let current = match std::fs::read_dir(&dir) {
            Ok(rd) => rd
                .flatten()
                .filter(|e| e.path().extension().and_then(|s| s.to_str()) == Some("md"))
                .count(),
            Err(_) => return,
        };
        let last = *self.notes_file_count.lock().unwrap();
        if current != last {
            println!("🔄 检测到笔记变化 ({} → {}), 自动重载...", last, current);
            self.load_notes();
        }
    }

    // ── Notes: CRUD ───────────────────────────────

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

    /// 返回笔记列表（带排序）。sort: "time" | "title" | "views"
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
                result.sort_by(|a, b| a.title.to_lowercase().cmp(&b.title.to_lowercase()));
            }
            "views" => {
                result.sort_by(|a, b| match b.view_count.cmp(&a.view_count) {
                    std::cmp::Ordering::Equal => b.id.cmp(&a.id),
                    other => other,
                });
            }
            _ => {
                result.sort_by(|a, b| b.id.cmp(&a.id));
            }
        }
        result.into_iter().skip(offset).take(limit).collect()
    }

    /// 统计笔记总数（带 tag/category 过滤）
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
        let mut related: Vec<Note> = notes
            .iter()
            .filter(|n| n.id != id && n.tags.iter().any(|t| current.tags.contains(t)))
            .cloned()
            .collect();
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

    // ── Guestbook: CRUD ────────────────────────────

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
        gb.push(crate::models::GuestbookEntry {
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

    pub fn get_guestbook(&self, limit: usize, offset: usize) -> Vec<crate::models::GuestbookEntry> {
        let gb = self.guestbook.lock().unwrap();
        let result: Vec<crate::models::GuestbookEntry> = gb
            .iter()
            .filter(|g| g.is_approved == 1)
            .rev()
            .cloned()
            .collect();
        result.into_iter().skip(offset).take(limit).collect()
    }

    // ── Portfolio: CRUD ────────────────────────────

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
            created_at: now.clone(),
            updated_at: now,
        });
        drop(pf);
        self.save_portfolio();
        id
    }

    pub fn get_portfolio(&self) -> Vec<PortfolioItem> {
        let pf = self.portfolio.lock().unwrap();
        let mut result: Vec<PortfolioItem> = pf.clone();
        result.sort_by(|a, b| match a.sort_order.cmp(&b.sort_order) {
            std::cmp::Ordering::Equal => b.created_at.cmp(&a.created_at),
            other => other,
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
}

// 抑制未使用导入警告（Arc 在 future 扩展中可能用到）
#[allow(dead_code)]
fn _unused_import_marker() -> Arc<()> {
    Arc::new(())
}
