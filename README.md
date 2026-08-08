# Heron Web · 个人网站服务

> 用 Rust 标准库从零实现的个人网站后端——零第三方依赖、多线程并发、JSON 文件持久化，部署于 heronwang.cn。

---

## 项目背景

本项目是一个个人技术空间网站，包含技术笔记、访客留言、作品集展示等功能模块。项目的核心设计理念是 **"零第三方依赖"**——不依赖任何 Rust crate（无 serde、无 tokio、无 axum），全部使用 Rust 标准库手工实现：

- 手写 HTTP/1.1 请求解析与响应构建
- 手写 JSON 序列化/反序列化解析器
- 手写敏感信息脱敏引擎（私钥、JWT、API Key、数据库连接串等）
- 基于 `std::thread` 的多线程并发处理
- 基于 JSON 文件的持久化存储（`Mutex` 保证线程安全）

前端为嵌入二进制的单页应用（SPA），通过 Hash 路由切换页面，部署后只有一个可执行文件，无需额外静态资源。外网通过 Cloudflare Tunnel 暴露 `heronwang.cn → localhost:8080`。

---

## 项目结构

```
heron-web/
├── Cargo.toml              # 包配置（LTO 优化，单二进制输出）
├── Cargo.lock
├── heron-web.service       # systemd 服务文件（开机自启、自动重启）
├── src/
│   ├── main.rs             # 程序入口，绑定 0.0.0.0:8080，多线程 TCP 监听
│   ├── config.rs           # 配置常量（HOST/PORT/DATA_DIR）与环境变量读取（API_TOKEN）
│   ├── handler.rs          # HTTP 请求分发（GET/POST/PUT/DELETE/OPTIONS）与响应工具函数
│   ├── routes.rs           # GET 路由处理、HTTP 请求解析、Request 结构体定义
│   ├── api.rs              # POST/PUT/DELETE 路由处理（笔记/留言/作品的增删改）
│   ├── models.rs           # 数据模型定义（Note、GuestbookEntry、PortfolioItem）及序列化
│   ├── store.rs            # JSON 文件持久化层，Store 结构体与文件 I/O、Mutex 线程安全
│   ├── service.rs          # 业务逻辑层，Store 的 CRUD、搜索、排序、导航、推荐方法
│   ├── json.rs             # 手写 JSON 解析器（JsonValue 枚举 + JsonParser 递归下降解析）
│   ├── redact.rs           # 敏感信息脱敏（私钥/JWT/API Key/key=value/数据库连接串）
│   ├── utils.rs            # 工具函数（ISO 时间戳、JSON 转义、数组序列化）
│   └── index.html          # SPA 单页应用首页（编译时 include_str! 嵌入二进制）
├── static/
│   └── favicon.svg         # 鹭鸟主题 SVG favicon
└── data/                   # JSON 数据存储目录（运行时生成）
    ├── notes.json          # 技术笔记数据
    ├── guestbook.json      # 留言数据
    ├── portfolio.json      # 作品集数据
    └── visits.json         # 访问统计数据
```

---

## API 接口

所有写操作（POST/PUT/DELETE）需在请求头携带 `X-API-Token` 进行认证，Token 来自环境变量 `API_TOKEN`。留言创建无需 Token（公开接口）。

### GET 接口

| 方法 | 路径 | 说明 |
|------|------|------|
| GET | `/` | SPA 首页（HTML），记录访客访问 |
| GET | `/health` | 健康检查，返回 `{"status":"ok"}` |
| GET | `/api/stats` | 站点统计（在线人数、累计访问量） |
| GET | `/api/heartbeat` | 心跳上报，刷新在线状态 |
| GET | `/favicon.svg` | 站点 favicon（SVG） |
| GET | `/api/notes` | 笔记列表（支持 `limit`/`offset`/`tag`/`category`/`q`/`sort` 查询参数） |
| GET | `/api/notes/tags` | 获取所有笔记标签 |
| GET | `/api/notes/<id>` | 获取单条笔记详情 |
| GET | `/api/notes/<id>/prev` | 上一篇笔记 |
| GET | `/api/notes/<id>/next` | 下一篇笔记 |
| GET | `/api/notes/<id>/related` | 相关推荐笔记（同标签，最多 5 条） |
| GET | `/api/notes/export` | 导出全部笔记（按 id 升序，含全文） |
| GET | `/rss.xml` | RSS 2.0 订阅源（最近 20 条笔记） |
| GET | `/api/guestbook` | 留言列表（支持 `limit`/`offset` 分页） |
| GET | `/api/portfolio` | 作品集列表（按 sort_order 排序） |
| GET | `/api/documents` | 文档列表（预留接口，当前返回空数组） |

### POST / PUT / DELETE 接口

| 方法 | 路径 | 认证 | 说明 |
|------|------|------|------|
| POST | `/api/notes` | 🔒 Token | 创建笔记（自动脱敏敏感信息） |
| POST | `/api/notes/import` | 🔒 Token | 批量导入笔记 |
| POST | `/api/notes/<id>/view` | 公开 | 阅读次数 +1 |
| POST | `/api/guestbook` | 公开 | 提交留言（字数限制：昵称 30、留言 500、联系方式 50） |
| POST | `/api/portfolio` | 🔒 Token | 创建作品 |
| PUT | `/api/portfolio/<id>` | 🔒 Token | 更新作品（支持部分字段更新） |
| DELETE | `/api/notes/<id>` | 🔒 Token | 删除笔记 |
| DELETE | `/api/portfolio/<id>` | 🔒 Token | 删除作品 |

---

## 快速启动

### 环境要求

- Rust 工具链（`rustc` + `cargo`，edition 2021）

### 构建

```bash
# 进入项目目录
cd /home/heron/workspace/heron-web

# Release 构建（启用 LTO 优化，codegen-units=1）
cargo build --release

# 构建产物
# → target/release/heron-web
```

### 直接运行

```bash
# 设置 API Token（管理接口认证用）
export API_TOKEN="your-secret-token"

# 启动服务
./target/release/heron-web

# 服务监听 http://localhost:8080
```

### systemd 部署

项目自带 `heron-web.service` 文件，配置如下：

```ini
[Unit]
Description=Heron Web (Rust) - Personal Website Service
After=network.target

[Service]
Type=simple
User=heron
WorkingDirectory=/home/heron/workspace/heron-web
ExecStart=/home/heron/workspace/heron-web/target/release/heron-web
Environment=API_TOKEN=hermes-a3f7b2e9c1d4
Restart=always
RestartSec=5

# 安全限制
NoNewPrivileges=yes
ProtectSystem=full
ProtectHome=read-only
PrivateTmp=yes
ReadWritePaths=/home/heron/workspace/heron-web/data

[Install]
WantedBy=multi-user.target
```

部署步骤：

```bash
# 1. 复制服务文件到 systemd 目录
sudo cp heron-web.service /etc/systemd/system/

# 2. 重新加载 systemd 配置
sudo systemctl daemon-reload

# 3. 启用并启动服务（开机自启）
sudo systemctl enable --now heron-web

# 4. 查看运行状态
sudo systemctl status heron-web

# 5. 查看日志
journalctl -u heron-web -f
```

### Cloudflare Tunnel

外网通过 Cloudflare Tunnel 将 `heronwang.cn` 反向代理到 `localhost:8080`，无需开放公网端口。

---

## 演示效果

网站为暗色主题 SPA 单页应用（支持亮/暗主题切换），导航栏含 Logo、搜索框、页面链接及主题切换按钮。包含以下页面：

- **首页** — 渐变色 Hero 标题「Heron Wang · 个人空间」，副标题「技术探索 · 踩坑记录 · 作品分享」；下方三张功能卡片（技术笔记、访客留言、作品集）；顶部实时统计栏（在线人数脉冲指示灯、累计访问量）。
- **笔记** — 笔记列表页，支持标签筛选、排序（时间/标题/阅读量）、关键词搜索；每条笔记卡片展示标题、分类、阅读量、标签、创建时间；点击进入详情页（含 Markdown 渲染、上下篇导航、相关推荐）。
- **留言** — 访客留言板，顶部留言表单（昵称 + 留言内容 + 联系方式），下方展示已 approved 的留言列表。
- **作品集** — 个人项目展示，每个作品含标题、描述、技术栈标签、在线链接和源码仓库链接。
- **时间轴** — 笔记与项目的时间线视图，按时间从早到近排列。
- **关于** — 个人简介页面，展示站点统计数据（笔记数、留言数、作品数等）。

站点同时提供 `/rss.xml` RSS 订阅源和 `/health` 健康检查端点，favicon 为鹭鸟主题 SVG。
