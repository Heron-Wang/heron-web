//! Markdown 笔记解析 — 从 .md 文件提取元信息与正文

use std::path::PathBuf;

/// 解析 markdown 笔记文件，提取元信息和正文。
///
/// 文件格式:
/// ```md
/// # 标题
///
/// > **日期**: 2026-08-06
/// > **分类**: 踩坑记录
/// > **标签**: a, b, c
/// > **来源**: hermes
///
/// ---
///
/// 正文内容...
/// ```
pub fn parse_markdown_note(
    content: &str,
    path: &PathBuf,
) -> (String, Vec<String>, String, String, String, String) {
    let mut title = String::new();
    let mut tags: Vec<String> = Vec::new();
    let mut category = String::from("经验总结");
    let mut source = String::from("hermes");
    let mut created_at = String::new();
    let mut body_start = 0;

    let lines: Vec<&str> = content.lines().collect();

    // 第一行: # 标题
    for (i, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if let Some(t) = trimmed.strip_prefix("# ") {
            title = t.trim().to_string();
            body_start = i + 1;
        }
        break;
    }

    // 解析元信息块 (> **key**: value)
    let mut meta_end = body_start;
    for (i, line) in lines.iter().enumerate().skip(body_start) {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if trimmed.starts_with('>') {
            let inner = trimmed.trim_start_matches('>').trim();
            if let Some(rest) = inner.strip_prefix("**日期**:") {
                created_at = rest.trim().to_string();
            } else if let Some(rest) = inner.strip_prefix("**分类**:") {
                let c = rest.trim();
                if !c.is_empty() {
                    category = c.to_string();
                }
            } else if let Some(rest) = inner.strip_prefix("**标签**:") {
                let t = rest.trim();
                if t != "无" && !t.is_empty() {
                    tags = t.split(',').map(|s| s.trim().to_string()).collect();
                }
            } else if let Some(rest) = inner.strip_prefix("**来源**:") {
                let s = rest.trim();
                if !s.is_empty() {
                    source = s.to_string();
                }
            }
            meta_end = i + 1;
        } else if trimmed == "---" {
            meta_end = i + 1;
            break;
        } else {
            break;
        }
    }

    // 正文从 meta_end 之后开始
    let body = lines
        .iter()
        .skip(meta_end)
        .cloned()
        .collect::<Vec<&str>>()
        .join("\n")
        .trim()
        .to_string();

    // 从文件名提取日期作为回退 (YYMMDD_NNN_标题.md → 20YY-MM-DD)
    if created_at.is_empty() {
        if let Some(fname) = path.file_name().and_then(|n| n.to_str()) {
            if fname.len() >= 6 {
                let yy = &fname[..2];
                let mm = &fname[2..4];
                let dd = &fname[4..6];
                created_at = format!("20{}-{}-{}", yy, mm, dd);
            }
        }
    }

    (title, tags, category, source, created_at, body)
}
