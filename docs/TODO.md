# z-switch 开发代办

> 开发进度跟进清单。完成的项打勾并在「已完成」区留档；新想法先进「待评估」。
> 约定：`[ ]` 待办 · `[~]` 进行中 · `[x]` 已完成。

## 🔴 高优先级

- [ ] **本地路由：热切换时同步改写模型名与 Codex `wire_api`**
      当前开启代理后，已落入 live 配置的模型名 / `wire_api` 不会随切换更新，导致跨模型 / 跨协议热切换不可用。
- [ ] **本地路由：自动重试 / 故障转移**
      目前上游失败直接透传，无重试、无 failover。
- [ ] **补充可持续验证的真实供应商用例**
      代理路径、鉴权头、流式响应兼容性仍需用真实中转站长期验证。

## 🟡 中优先级

- [ ] **全局 env / 公共配置合并**（详见「已知限制」1）
      让部分 env 变量（Claude）/ 公共段（Codex）可跨供应商共享，而不是 per-provider 记忆。
- [ ] **ztest.ai 验真功能接入**（设置页已预留后续版本提示）
- [ ] **首启引导 / 空状态优化**，降低新用户上手成本。

## 🟢 低优先级 / 打磨

- [ ] 供应商卡片批量操作（批量测速 / 批量删除）
- [ ] 深色主题细节走查
- [ ] i18n（英文 README 与界面文案）

## ⚠️ 已知限制 / 待观察

1. **`env` / `config.toml` 内的第三方配置是 per-provider 记忆的，不是全局共享。**
   - Claude：`write_claude_live` 只替换顶层 `env` 键，其它顶层字段（如 `statusLine`、`hooks`、`permissions`）**始终保留**；但 `env` 内部是整体替换，插件写进 `env` 的变量只跟随「切换离开时激活的那张卡片」（靠 `backfill` 捕获）。
   - Codex：`config.toml` 按供应商整份文本存储 / 回写，插件写进去的 `[mcp_servers]` 等同样是 per-provider。
   - 影响：切到「插件安装前建的卡片」时，这些第三方配置会暂时不在 live（切回原卡片会恢复）。
   - 可能的解法：见「全局 env / 公共配置合并」。
   - 相关代码：`src-tauri/src/live.rs`（`write_claude_live` / `write_codex_live` / `backfill`）。

2. **配置文件解析失败时会中止写入**（设计如此，防止覆盖用户配置）。
   需确保 UI 能清晰地把该错误提示给用户。

## ✅ 已完成

- [x] 重写 README（徽章、功能表格、工作原理、安全隐私、目录等）
- [x] 新增 MIT `LICENSE`，保留 cc-switch 原始版权声明并致谢
- [x] 新增完整使用教程 `docs/USAGE.md`，修复应用内「文档」按钮地址
- [x] 配置 GitHub Actions 三平台发布流水线，发布首个版本 **v0.1.0**
- [x] 测速改为 **HTTP 层**探测（弃用纯 TCP，规避 TUN / 透明代理就地应答的「<1ms」失真）
- [x] 修复：恢复 Codex 后切第三方（开路由）不触发第三方请求（`current=None` 也走写 localhost 分支）
- [x] Claude **1M 长上下文**：Sonnet / Opus / Fable 可勾选，模型名追加 `[1M]` 标记
- [x] **应用到 Claude Code 插件**（VS Code 扩展 `~/.claude/config.json` 的 `primaryApiKey`）
- [x] **跳过 Claude Code 初次安装确认**（`~/.claude.json` 的 `hasCompletedOnboarding`）
- [x] 设置页**配置目录**快捷入口（打开 `~/.claude` / `~/.codex` / `~/.z-switch`）
- [x] **浏览器一键添加供应商**：`zswitch://import` 深链加固（确认弹窗 + http(s) 校验 + 密钥脱敏 + 同名不覆盖），附 `docs/DEEPLINK.md` 规范与 `docs/deeplink.html` 生成页
- [x] **Claude 桌面版随切换生效**（独立聊天 App，写 3p 网关 profile；代理跟随 / 直连两模式；仅 macOS/Windows，未装 App 时 no-op；schema 已用真机 dump 核对） — *待真机验证激活*
- [x] 同步更新 README / USAGE 文档至以上能力
