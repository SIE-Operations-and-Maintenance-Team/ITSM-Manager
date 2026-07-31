# ITSM Manager

<p align="center">
  <img src="app-icon.png" width="120" alt="ITSM Manager" />
</p>

> SIE ITSM 工单管理桌面工具 —— 基于 Tauri 2 + Rust，内置本机 MCP server，让 AI agent 也能直接操作工单。

ITSM Manager 是面向 SIE 运维团队的 Windows 桌面应用，用于集中查看和处理 [ITSM](https://help.chinasie.com) 工单：登录后自动按配置的视图周期性拉取工单，支持在应用内完成回复、解决、暂挂/取消暂挂、补单、转派等操作；同时在本机暴露一个 MCP（Model Context Protocol）server，AI agent（如 Claude Code）可通过标准 MCP 协议查询与处理工单。

## 功能特性

- **单点登录集成**：内嵌 ITSM 登录页，登录后自动捕获 token，无需手动复制粘贴。
- **多视图工单列表**：按工单视图（我的工单、所有工单等）分类浏览，支持分页、按工单号/主题/客户组搜索。
- **后台定时刷新**：按视图白名单和自定义间隔自动拉取工单，新工单/异常通过系统通知提醒。
- **工单操作**：在应用内直接回复（公开/内部备注）、解决、暂挂、取消暂挂、补单、转派。
- **本地缓存**：工单列表按视图分文件缓存，断网也能查看上次拉取的结果。
- **系统托盘 + 开机自启**：最小化到托盘后台运行，可选开机启动。
- **本机 MCP server**：AI agent 通过 MCP 协议复用应用登录态，查询与处理工单（见下文）。

## 技术栈

| 层 | 技术 |
|---|---|
| 桌面框架 | [Tauri 2](https://tauri.app/)（Rust 后端 + WebView 前端） |
| 后端 | Rust、tokio、reqwest、serde |
| 前端 | 原生 HTML / CSS / JavaScript（无构建步骤，由 Tauri 直接分发） |
| MCP | [rmcp](https://crates.io/crates/rmcp) 2.2 + axum 0.8（Streamable HTTP） |
| 打包 | NSIS 安装包（`bundle.targets: ["nsis"]`） |

## 环境要求

- Windows 10/11（自带 WebView2）
- [Rust](https://www.rust-lang.org/) 工具链（stable）
- [Tauri CLI 2](https://v2.tauri.app/)：`cargo install tauri-cli --version "^2"`

> 应用连接的外部服务为 SIE 内部 ITSM（`api-itsm.chinasie.com`），需有有效账号才能使用。

## 开发与构建

```bash
# 开发模式（热重载窗口）
cargo tauri dev

# 发布构建，产出 NSIS 安装包（src-tauri/target/release/bundle/nsis/）
cargo tauri build

# 仅编译 Rust 库/二进制
cargo build --release
```

前端为静态文件（`src/`），无 `npm` 构建步骤；`package.json` 中的 `dev`/`build` 脚本仅为占位。

## MCP 集成

应用启动后默认在本机 `http://127.0.0.1:17540/mcp` 暴露一个 **stateless、Streamable HTTP** 的 MCP server，复用应用当前登录态。在 AI 客户端中将其配置为 HTTP 类型 MCP server 即可调用以下 10 个工单工具：

### 只读工具（6）

| 工具 | 说明 |
|---|---|
| `list_views` | 列出可用工单视图（含 `seachType` 编号） |
| `search_tickets_by_code` | 按工单号或主题关键字模糊搜索 |
| `search_tickets_by_customer_group` | 按客户组名称关键字模糊搜索 |
| `get_detail` | 按 `incidentId` 获取工单详情 |
| `get_ticket_by_code` | 按展示单号（如 `IM26070065`）定位工单 |
| `list_replies` | 列出指定工单的回复记录 |

### 写入工具（4）

| 工具 | 说明 |
|---|---|
| `reply` | 回复工单（可选内部备注 / 公开回复） |
| `resolve` | 解决工单（需填写解决方案） |
| `suspend` / `unhang` | 暂挂 / 取消暂挂工单 |

> 安全说明：MCP server 只监听 loopback（`127.0.0.1`），不对外网开放，不做独立鉴权，直接借用应用当前登录 token。

## 配置与本地数据

应用内「设置」对话框可配置：

- **视图白名单与刷新间隔**：选择要后台刷新的视图及其周期（秒；0 表示暂停）。
- **每视图分页大小**：50 / 100 / 200。
- **补单默认值**：补单操作的预设字段。
- **MCP**：开关、端口（默认 17540）、缺省视图（`search` / `get_ticket_by_code` 未指定视图时使用，默认 7=所有工单）。修改后重启应用生效。

本地持久化文件位于 Tauri `app_data_dir`：

| 文件 | 内容 |
|---|---|
| `credentials.json` | 登录凭证（token / tenant_id / user_name） |
| `config.json` | 用户配置（上述设置项） |
| `cache/tickets_{seachType}.json` | 各视图工单列表缓存 |
| `cache/views.json` | 视图列表缓存 |

所有文件以原子写（临时文件 + rename）落盘，防写一半崩溃。登出会清除凭证与缓存，但保留 `config.json`。

## 项目结构

```
src/                      静态前端（Tauri webview 加载）
├── index.html            单页面：登录屏 / 主屏 / 各操作弹窗
├── main.js               前端逻辑：IPC 调用、视图/分页、自动刷新
└── styles.css            样式

src-tauri/
├── Cargo.toml            Rust 依赖与 release profile
├── tauri.conf.json       Tauri 配置（窗口、bundle、identifier）
├── capabilities/         Tauri 2 权限声明
└── src/
    ├── main.rs           入口
    ├── lib.rs            Tauri Builder、IPC handler 注册、登录流程
    ├── commands.rs       IPC 命令薄封装
    ├── api.rs            纯 HTTP 层（ITSM REST API + 错误分类）
    ├── config.rs         用户配置读写
    ├── scheduler.rs      后台周期刷新调度
    ├── cache.rs          工单列表本地缓存
    ├── mcp.rs            本机 MCP server（10 个工单工具）
    └── state.rs          AppState、凭证、原子写工具
```

分层约定：`commands.rs` 是 IPC 边界，`api.rs` 是纯 HTTP 层（可独立单测），`mcp.rs` 是 agent 边界（与 `commands.rs` 平行，复用 `api.rs`，不引入新的 ITSM endpoint）。

## 测试

使用 Rust 内置 `#[test]`，无外部服务 mock：

```bash
cargo test                # 运行全部单测
```

已覆盖：HTTP 错误分类与响应解析（`api.rs`）、配置钳位与序列化往返（`config.rs`）、缓存读写与损坏文件处理（`cache.rs`）、调度告警阈值（`scheduler.rs`）。涉及真实 ITSM API 的端到端行为需用有效 token 人工验证。

## 说明

- 本项目为 SIE 内部工具，外部服务地址（ITSM API、登录页）在代码中硬编码，未提供切换配置。
- 默认远程仓库：`https://github.com/SIE-Operations-and-Maintenance-Team/ITSM-Manager.git`。
