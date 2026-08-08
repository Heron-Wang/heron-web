//! 配置常量与环境变量读取

/// 监听地址
pub const HOST: &str = "0.0.0.0";

/// 监听端口
pub const PORT: u16 = 8080;

/// 数据存储目录
pub const DATA_DIR: &str = "data";

/// 从环境变量读取 API Token，避免硬编码泄露到源码
pub fn get_api_token() -> String {
    std::env::var("API_TOKEN").unwrap_or_else(|_| {
        eprintln!("⚠️  警告: 未设置 API_TOKEN 环境变量，管理接口将不可用");
        String::new()
    })
}
