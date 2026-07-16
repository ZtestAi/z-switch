# z-switch

![z-switch Logo](./src/assets/zsw.png)

z-switch 是一个面向 Claude Code 和 Codex 的桌面供应商切换器，由真测 Ztest（ztest.ai）出品。

它默认直接修改客户端配置文件，也提供可选的本地路由代理，用于在运行期间热切换供应商。项目坚持开源、无广告、无返利链接，不包含账单、会话、MCP、Skills 等无关功能。

## 使用帮助

1. 首次启动时，z-switch 会先保存本机 Claude Code / Codex 原始配置，并为两个应用建立默认“官方账号”卡片；检测到现有中转配置时会自动导入并设为当前项。
2. 点击右上角“添加”，填写供应商名称、Base URL 和 API Key；随后可测试连通性、拉取模型并测速。
3. Claude 供应商可分别配置主模型以及 Haiku、Sonnet、Opus、Fable 四个模型档位；Codex 供应商可选择 `responses` 或 `chat` 协议。
4. 保存后点击供应商卡片上的“切换”，直连模式会写入客户端配置；真实测试图标可选择模型并发送一条最小流式 `Hi` 请求。
5. 如需运行期间热切换，可在设置中开启“本地路由代理”；高级设置提供超时、连接复用、请求体限制和错误日志。
6. 遇到问题时，可在设置中打开错误日志目录；需要退出 z-switch 管理时，可恢复首次保存的本机原始配置。

> 真实测试会产生极少量模型调用费用。本地路由目前不提供自动重试或故障转移。

## 当前功能

- Claude Code / Codex 分区管理与一键切换
- 默认 Claude / OpenAI 官方账号卡片，与 API 中转站直接来回切换
- 自定义供应商添加、复制、编辑、删除
- 表单与 JSON 两种编辑方式
- Base URL 智能推断、连通性测试、模型列表拉取、TCP 测速、真实流式模型调用测试
- JSON 导入导出、首次启动自动保留并导入现有 `~/.claude` / `~/.codex` 配置
- 独立原始配置快照、一键恢复、原子写入、写前备份、Codex 双文件回滚、切换前 backfill
- 系统托盘、深链 `zswitch://import`、窗口状态记忆、单实例
- 点击关闭或按 Alt+F4 时最小化到托盘；从托盘菜单选择“退出”才结束进程
- 浅色、深色、跟随系统主题
- 开机自启
- 可选 localhost 代理：原样转发请求和流式响应，运行期间热切换目标
- 设置页保留 ztest.ai 验真功能的后续版本提示

每个 API 中转站卡片都提供“真实测试”入口，使用已保存的地址、密钥、模型和协议，向供应商发送一条 `Hi`，最多输出 32 tokens，并在独立弹窗中实时显示回复、首字耗时与总耗时。测试结果仅在本次运行中回显到卡片；请求可能产生极少量模型调用费用，测试内容、回复与密钥均不写入日志或持久化文件。

## 工作方式

### 直连模式（默认）

- Claude：合并写入 `~/.claude/settings.json` 的 `env`，保留其他顶层字段。
- Codex：写入 `~/.codex/auth.json` 和 `~/.codex/config.toml`；第二个文件写入失败时回滚 auth。
- 官方账号卡片不保存 API Key：Claude 清除中转环境变量并使用客户端本机登录；Codex 切走前保存客户端刷新后的登录态，切回时恢复。
- 切换前会把当前 live 配置回填到旧供应商，避免用户手工修改丢失。
- 删除正在使用的供应商时，可选择恢复首次原始配置，或保留电脑当前配置并仅解除 z-switch 管理。

首次运行会把原始文件完整保存在 `~/.z-switch/original/`。该快照独立于供应商列表和普通 JSON 导出，可在设置页分别恢复 Claude Code 或 Codex；恢复前仍会保存一份时间戳备份。

Claude Code 通常在下一次请求时读取新配置；Codex CLI 可能需要重启。

### 本地路由模式（实验）

开启后，z-switch 监听 `127.0.0.1:8899`（可由配置覆盖），将 Claude 和 Codex 的 live Base URL 分别改为：

- `http://127.0.0.1:8899/claude`
- `http://127.0.0.1:8899/codex`

代理根据当前 API 中转站注入对应鉴权信息并转发请求，不修改请求体、不做协议转换、不记录用量。中转站之间切换时会立即更新上游 Base URL 和鉴权信息；官方账号始终保持客户端直连，另一个应用仍可继续使用本地代理。设置页可配置连接、首段、流静默和非流式超时，请求体硬上限、连接池与 TCP Keepalive；流式返回和连接复用始终开启。

失败请求会按设置写入 `~/.z-switch/logs/proxy-errors.jsonl`，仅记录上游状态、脱敏 URL、失败阶段和截断后的错误详情，不记录请求正文。日志自动脱敏当前供应商密钥并按文件大小轮转，可在设置页打开目录或清空。

当前不会同步改写开启代理时已经落入 live 配置的模型名和 Codex `wire_api`。因此热切换只适用于模型/协议兼容的供应商；跨模型或跨协议切换仍需关闭代理后直连切换。该模式仍需要用真实供应商持续验证路径、鉴权头和流式响应兼容性。

## 开发

要求：

- Node.js 20.19+ 或 22.12+
- Rust stable
- Windows 下安装 Tauri 所需的 WebView2 与 MSVC 构建工具

```powershell
npm install
npm run tauri dev
```

常用检查：

```powershell
npm run build
cd src-tauri
cargo clippy --all-targets --all-features
cargo test
```

构建可执行文件：

```powershell
npm run tauri build -- --no-bundle
```

## 目录

```text
src/
  App.tsx                 主界面、切换、深链和状态同步
  ProviderModal.tsx       供应商表单、连通性、密钥和模型选择
  SettingsModal.tsx       主题、自启、本地路由等设置
  providerFactory.ts      Provider 构建器和地址推断规则
  api.ts / types.ts       Tauri 命令封装与前端类型

src-tauri/src/
  lib.rs                  Tauri 命令、状态和切换流程
  config.rs               路径与原子写入
  store.rs                providers.json 数据模型
  live.rs                 Claude/Codex live 配置读写
  proxy.rs                本地代理和热切换目标
  connectivity.rs         HTTP 连通性检查
  model_fetch.rs          模型列表拉取
  speed.rs                TCP 测速
  tray.rs                 系统托盘
```
