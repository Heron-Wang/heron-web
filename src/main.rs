//! Heron Wang · 个人网站主服务 (Rust 版)
//! 纯标准库实现，零第三方依赖。
//! 监听 0.0.0.0:8080，多线程处理并发。

mod api;
mod config;
mod handler;
mod json;
mod mdnotes;
mod models;
mod redact;
mod routes;
mod service;
mod store;
mod utils;

use std::net::TcpListener;
use std::sync::Arc;
use std::thread;

use config::{DATA_DIR, HOST, PORT};
use handler::handle_request;
use routes::read_request;
use store::Store;

fn main() {
    let store = Arc::new(Store::new(DATA_DIR));

    let addr = format!("{}:{}", HOST, PORT);
    let listener = match TcpListener::bind(&addr) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("❌ 无法绑定 {} : {}", addr, e);
            std::process::exit(1);
        }
    };

    println!("🌐 主站已启动 (Rust): http://localhost:{}", PORT);
    println!("   监听: {}", addr);
    println!("   外网: https://heronwang.cn");
    println!("   API Token: 读取自环境变量 API_TOKEN");
    println!("{}", "-".repeat(50));

    for incoming in listener.incoming() {
        match incoming {
            Ok(mut stream) => {
                let store = Arc::clone(&store);
                thread::spawn(move || {
                    if let Some(req) = read_request(&mut stream) {
                        handle_request(&mut stream, &req, &store);
                    }
                });
            }
            Err(e) => {
                eprintln!("⚠️ 连接错误: {}", e);
            }
        }
    }
}
