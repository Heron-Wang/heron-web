//! Analytics HTTP 路由层 — 处理 /api/analytics/* 请求

use std::net::TcpStream;
use std::sync::Arc;

use crate::analytics::Analytics;
use crate::handler::send_json;
use crate::routes::Request;
use crate::utils::json_escape;

/// 处理 /api/analytics/* GET 请求
pub fn handle_analytics_get(stream: &mut TcpStream, req: &Request, analytics: &Arc<Analytics>) {
    let path = req.path.as_str();

    if path == "/api/analytics/overview" {
        if !check_auth(req) {
            send_json(stream, r#"{"error":"unauthorized"}"#, 401, Some("no-cache"));
            return;
        }
        let online = analytics.get_online_ips();
        let total = analytics.get_total_logs();
        let hourly = analytics.get_hourly_stats();
        let hosts = analytics.get_host_stats();
        let top_ips = analytics.get_top_ips(10);
        let top_uas = analytics.get_top_uas(5);
        let json = build_overview_json(&online, total, &hourly, &hosts, &top_ips, &top_uas);
        send_json(stream, &json, 200, Some("no-cache"));
        return;
    }

    if path == "/api/analytics/daily" {
        if !check_auth(req) {
            send_json(stream, r#"{"error":"unauthorized"}"#, 401, Some("no-cache"));
            return;
        }
        let daily = analytics.get_daily_stats();
        let parts: Vec<String> = daily.iter().map(|(d, c)| format!(r#"["{}",{}]"#, d, c)).collect();
        send_json(stream, &format!("[{}]", parts.join(",")), 200, Some("no-cache"));
        return;
    }

    if path == "/api/analytics/heatmap" {
        if !check_auth(req) {
            send_json(stream, r#"{"error":"unauthorized"}"#, 401, Some("no-cache"));
            return;
        }
        let grid = analytics.get_heatmap();
        let rows: Vec<String> = grid.iter().map(|row| {
            let cells: Vec<String> = row.iter().map(|c| c.to_string()).collect();
            format!("[{}]", cells.join(","))
        }).collect();
        send_json(stream, &format!("[{}]", rows.join(",")), 200, Some("no-cache"));
        return;
    }

    if path == "/api/analytics/geo" {
        if !check_auth(req) {
            send_json(stream, r#"{"error":"unauthorized"}"#, 401, Some("no-cache"));
            return;
        }
        let geo = analytics.get_geo_stats();
        let parts: Vec<String> = geo.iter().map(|(r, c)| format!(r#"["{}",{}]"#, json_escape(r), c)).collect();
        send_json(stream, &format!("[{}]", parts.join(",")), 200, Some("no-cache"));
        return;
    }

    if path == "/api/analytics/paths" {
        if !check_auth(req) {
            send_json(stream, r#"{"error":"unauthorized"}"#, 401, Some("no-cache"));
            return;
        }
        let paths = analytics.get_top_paths(15);
        let parts: Vec<String> = paths.iter().map(|(p, c)| format!(r#"["{}",{}]"#, json_escape(p), c)).collect();
        send_json(stream, &format!("[{}]", parts.join(",")), 200, Some("no-cache"));
        return;
    }

    if path.starts_with("/api/analytics/verify") {
        let pw = req.query_param("pw").unwrap_or_default();
        let correct = std::env::var("ANALYTICS_PASSWORD").unwrap_or_default();
        if !correct.is_empty() && pw == correct {
            send_json(stream, r#"{"ok":true}"#, 200, Some("no-cache"));
        } else {
            send_json(stream, r#"{"ok":false}"#, 200, Some("no-cache"));
        }
        return;
    }

    send_json(stream, r#"{"error":"not found"}"#, 404, Some("no-cache"));
}

/// 检查 analytics 查询权限
fn check_auth(req: &Request) -> bool {
    let correct = std::env::var("ANALYTICS_PASSWORD").unwrap_or_default();
    if correct.is_empty() {
        return false;
    }
    let pw = req.query_param("pw").unwrap_or_default();
    pw == correct
}

/// 构建 overview JSON
fn build_overview_json(
    online: &[(String, u64)],
    total: usize,
    hourly: &[(u32, u64)],
    hosts: &[(String, u64)],
    top_ips: &[(String, u64)],
    top_uas: &[(String, u64)],
) -> String {
    let online_parts: Vec<String> = online.iter().map(|(ip, ts)| {
        format!(r#"{{"ip":"{}","last":{}}}"#, json_escape(ip), ts)
    }).collect();
    let hourly_parts: Vec<String> = hourly.iter().map(|(h, c)| format!("[{},{}]", h, c)).collect();
    let host_parts: Vec<String> = hosts.iter().map(|(h, c)| format!(r#"["{}",{}]"#, json_escape(h), c)).collect();
    let ip_parts: Vec<String> = top_ips.iter().map(|(ip, c)| format!(r#"["{}",{}]"#, json_escape(ip), c)).collect();
    let ua_parts: Vec<String> = top_uas.iter().map(|(ua, c)| format!(r#"["{}",{}]"#, json_escape(ua), c)).collect();
    format!(
        r#"{{"total":{},"online_count":{},"online_ips":[{}],"hourly":[{}],"hosts":[{}],"top_ips":[{}],"top_uas":[{}]}}"#,
        total, online.len(), online_parts.join(","),
        hourly_parts.join(","), host_parts.join(","),
        ip_parts.join(","), ua_parts.join(",")
    )
}
