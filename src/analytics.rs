//! 访问分析模块 — 记录每次请求并聚合查询
//! 纯标准库实现，内存缓存 + 定期持久化到 analytics.json

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::json::{JsonParser, JsonValue};
use crate::utils::json_escape;

/// 单条访问记录
#[derive(Clone)]
pub struct AccessLog {
    pub ts: u64,       // epoch 秒
    pub ip: String,
    pub path: String,
    pub host: String,  // 子域名
    pub ua: String,
    pub status: u16,
}

pub struct Analytics {
    pub logs: Arc<Mutex<Vec<AccessLog>>>,
    pub data_dir: PathBuf,
    /// 上次持久化的时间
    pub last_save: Arc<Mutex<u64>>,
}

impl Analytics {
    pub fn new(data_dir: &str) -> Self {
        let dir = PathBuf::from(data_dir);
        let a = Analytics {
            logs: Arc::new(Mutex::new(Vec::new())),
            data_dir: dir,
            last_save: Arc::new(Mutex::new(0)),
        };
        a.load();
        a
    }

    fn analytics_path(&self) -> PathBuf {
        self.data_dir.join("analytics.json")
    }

    /// 从 JSON 文件加载历史日志（只加载最近7天）
    pub fn load(&self) {
        let path = self.analytics_path();
        if !path.exists() {
            return;
        }
        match fs::read_to_string(&path) {
            Ok(content) => {
                if let Ok(JsonValue::Array(arr)) = JsonParser::parse(&content) {
                    let now = now_secs();
                    let week_ago = now - 7 * 86400;
                    let mut logs = self.logs.lock().unwrap();
                    for item in &arr {
                        if let JsonValue::Object(obj) = item {
                            let mut log = AccessLog {
                                ts: 0,
                                ip: String::new(),
                                path: String::new(),
                                host: String::new(),
                                ua: String::new(),
                                status: 200,
                            };
                            for (k, v) in obj {
                                match k.as_str() {
                                    "ts" => {
                                        if let JsonValue::Number(n) = v {
                                            log.ts = *n as u64;
                                        }
                                    }
                                    "ip" => {
                                        if let JsonValue::String(s) = v {
                                            log.ip = s.clone();
                                        }
                                    }
                                    "path" => {
                                        if let JsonValue::String(s) = v {
                                            log.path = s.clone();
                                        }
                                    }
                                    "host" => {
                                        if let JsonValue::String(s) = v {
                                            log.host = s.clone();
                                        }
                                    }
                                    "ua" => {
                                        if let JsonValue::String(s) = v {
                                            log.ua = s.clone();
                                        }
                                    }
                                    "status" => {
                                        if let JsonValue::Number(n) = v {
                                            log.status = *n as u16;
                                        }
                                    }
                                    _ => {}
                                }
                            }
                            if log.ts >= week_ago {
                                logs.push(log);
                            }
                        }
                    }
                    println!("📊 加载 {} 条历史访问日志", logs.len());
                }
            }
            Err(e) => eprintln!("⚠️ 读取 analytics.json 失败: {}", e),
        }
    }

    /// 持久化到 JSON 文件（全量写入）
    pub fn save(&self) {
        let logs = self.logs.lock().unwrap();
        let parts: Vec<String> = logs
            .iter()
            .map(|l| {
                format!(
                    r#"{{"ts":{},"ip":"{}","path":"{}","host":"{}","ua":"{}","status":{}}}"#,
                    l.ts,
                    json_escape(&l.ip),
                    json_escape(&l.path),
                    json_escape(&l.host),
                    json_escape(&l.ua),
                    l.status
                )
            })
            .collect();
        let json = format!("[{}]", parts.join(","));
        if let Err(e) = fs::write(self.analytics_path(), &json) {
            eprintln!("⚠️ 保存 analytics.json 失败: {}", e);
        }
        *self.last_save.lock().unwrap() = now_secs();
    }

    /// 记录一次访问
    pub fn record(&self, ip: &str, path: &str, host: &str, ua: &str, status: u16) {
        let now = now_secs();
        let log = AccessLog {
            ts: now,
            ip: ip.to_string(),
            path: path.to_string(),
            host: host.to_string(),
            ua: ua.to_string(),
            status,
        };
        {
            let mut logs = self.logs.lock().unwrap();
            logs.push(log);
            // 保留最近7天 + 最多10000条
            let week_ago = now - 7 * 86400;
            logs.retain(|l| l.ts >= week_ago);
            if logs.len() > 10000 {
                let drain = logs.len() - 10000;
                logs.drain(..drain);
            }
        }
        // 每5分钟持久化一次
        let last = *self.last_save.lock().unwrap();
        if now - last > 300 {
            self.save();
        }
    }

    // ── 查询接口 ──────────────────────────────

    /// 当前在线 IP 列表（30秒内有活动）
    pub fn get_online_ips(&self) -> Vec<(String, u64)> {
        let now = now_secs();
        let logs = self.logs.lock().unwrap();
        let mut ip_last: HashMap<String, u64> = HashMap::new();
        for l in logs.iter().rev() {
            if now > l.ts && now - l.ts <= 30 {
                ip_last.entry(l.ip.clone()).or_insert(l.ts);
            }
        }
        let mut result: Vec<(String, u64)> =
            ip_last.into_iter().map(|(ip, ts)| (ip, ts)).collect();
        result.sort_by(|a, b| b.1.cmp(&a.1));
        result
    }

    /// 24小时按小时聚合
    pub fn get_hourly_stats(&self) -> Vec<(u32, u64)> {
        let now = now_secs();
        let day_ago = now - 86400;
        let logs = self.logs.lock().unwrap();
        let mut hours: [u64; 24] = [0; 24];
        for l in logs.iter() {
            if l.ts >= day_ago {
                let hour = ((l.ts / 3600) % 24) as usize;
                hours[hour] += 1;
            }
        }
        hours.iter().enumerate().map(|(i, &c)| (i as u32, c)).collect()
    }

    /// 30天按天聚合
    pub fn get_daily_stats(&self) -> Vec<(String, u64)> {
        let now = now_secs();
        let month_ago = now - 30 * 86400;
        let logs = self.logs.lock().unwrap();
        let mut days: HashMap<String, u64> = HashMap::new();
        for l in logs.iter() {
            if l.ts >= month_ago {
                let day = epoch_to_date(l.ts);
                *days.entry(day).or_insert(0) += 1;
            }
        }
        let mut result: Vec<(String, u64)> = days.into_iter().collect();
        result.sort_by(|a, b| a.0.cmp(&b.0));
        result
    }

    /// 按子域名聚合
    pub fn get_host_stats(&self) -> Vec<(String, u64)> {
        let logs = self.logs.lock().unwrap();
        let mut hosts: HashMap<String, u64> = HashMap::new();
        for l in logs.iter() {
            *hosts.entry(l.host.clone()).or_insert(0) += 1;
        }
        let mut result: Vec<(String, u64)> = hosts.into_iter().collect();
        result.sort_by(|a, b| b.1.cmp(&a.1));
        result
    }

    /// 按路径聚合 Top N
    pub fn get_top_paths(&self, n: usize) -> Vec<(String, u64)> {
        let logs = self.logs.lock().unwrap();
        let mut paths: HashMap<String, u64> = HashMap::new();
        for l in logs.iter() {
            *paths.entry(l.path.clone()).or_insert(0) += 1;
        }
        let mut result: Vec<(String, u64)> = paths.into_iter().collect();
        result.sort_by(|a, b| b.1.cmp(&a.1));
        result.truncate(n);
        result
    }

    /// 按 IP 聚合 Top N
    pub fn get_top_ips(&self, n: usize) -> Vec<(String, u64)> {
        let logs = self.logs.lock().unwrap();
        let mut ips: HashMap<String, u64> = HashMap::new();
        for l in logs.iter() {
            *ips.entry(l.ip.clone()).or_insert(0) += 1;
        }
        let mut result: Vec<(String, u64)> = ips.into_iter().collect();
        result.sort_by(|a, b| b.1.cmp(&a.1));
        result.truncate(n);
        result
    }

    /// 按 UA 聚合 Top N
    pub fn get_top_uas(&self, n: usize) -> Vec<(String, u64)> {
        let logs = self.logs.lock().unwrap();
        let mut uas: HashMap<String, u64> = HashMap::new();
        for l in logs.iter() {
            let ua = simplify_ua(&l.ua);
            *uas.entry(ua).or_insert(0) += 1;
        }
        let mut result: Vec<(String, u64)> = uas.into_iter().collect();
        result.sort_by(|a, b| b.1.cmp(&a.1));
        result.truncate(n);
        result
    }

    /// 星期x小时热力图 (7x24)
    pub fn get_heatmap(&self) -> Vec<Vec<u64>> {
        let logs = self.logs.lock().unwrap();
        let mut grid = vec![vec![0u64; 24]; 7];
        for l in logs.iter() {
            let dow = epoch_to_dow(l.ts); // 0=Sunday
            let hour = ((l.ts / 3600) % 24) as usize;
            grid[dow][hour] += 1;
        }
        grid
    }

    /// IP 地域分布（简单按 IP 前缀分组，离线近似）
    pub fn get_geo_stats(&self) -> Vec<(String, u64)> {
        let logs = self.logs.lock().unwrap();
        let mut geo: HashMap<String, u64> = HashMap::new();
        for l in logs.iter() {
            let region = ip_to_region(&l.ip);
            *geo.entry(region).or_insert(0) += 1;
        }
        let mut result: Vec<(String, u64)> = geo.into_iter().collect();
        result.sort_by(|a, b| b.1.cmp(&a.1));
        result
    }

    /// 总访问数（日志条数）
    pub fn get_total_logs(&self) -> usize {
        self.logs.lock().unwrap().len()
    }
}

// ── 辅助函数 ──────────────────────────────────

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// epoch 秒 → "YYYY-MM-DD"（UTC）
fn epoch_to_date(secs: u64) -> String {
    let days = secs / 86400;
    let (y, m, d) = days_to_ymd(days as i64);
    format!("{:04}-{:02}-{:02}", y, m, d)
}

fn days_to_ymd(days: i64) -> (i64, u32, u32) {
    let mut days = days;
    let mut year = 1970i64;
    loop {
        let diy = if is_leap(year) { 366 } else { 365 };
        if days < diy {
            break;
        }
        days -= diy;
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

/// epoch 秒 → 星期几 (0=Sunday)
fn epoch_to_dow(secs: u64) -> usize {
    let days = secs / 86400;
    // 1970-01-01 是星期四 (4)
    ((days + 4) % 7) as usize
}

/// UA 简化归类
fn simplify_ua(ua: &str) -> String {
    let ua_lower = ua.to_lowercase();
    if ua_lower.contains("bot") || ua_lower.contains("crawler") || ua_lower.contains("spider") {
        return "🤖 爬虫".to_string();
    }
    if ua_lower.contains("mobile") || ua_lower.contains("android") || ua_lower.contains("iphone") {
        if ua_lower.contains("android") {
            return "📱 Android".to_string();
        }
        return "📱 iPhone".to_string();
    }
    if ua_lower.contains("windows") {
        return "💻 Windows".to_string();
    }
    if ua_lower.contains("mac") || ua_lower.contains("darwin") {
        return "💻 macOS".to_string();
    }
    if ua_lower.contains("linux") {
        return "💻 Linux".to_string();
    }
    if ua.is_empty() {
        return "❓ 未知".to_string();
    }
    "🌐 其他".to_string()
}

/// IP → 地域（简单近似：私有网络/本地/外网）
fn ip_to_region(ip: &str) -> String {
    if ip.starts_with("127.") || ip == "::1" {
        return "本地".to_string();
    }
    if ip.starts_with("192.168.") || ip.starts_with("10.") || ip.starts_with("172.") {
        return "局域网".to_string();
    }
    // 外网 IP 无法离线精确归属，标注为外网
    // 后续可接入纯真IP库或在线API
    format!("外网 {}", ip_to_prefix(ip))
}
/// 取 IP 前两段作为粗粒度标识
fn ip_to_prefix(ip: &str) -> String {
    let parts: Vec<&str> = ip.split('.').collect();
    if parts.len() >= 2 { format!("{}.{}.*", parts[0], parts[1]) } else { ip.to_string() }
}
