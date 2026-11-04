# TransitLink

中转站用户客户端：登录 new-api 或 sub2api 搭建的中转站，管理 Key，一键配置 Claude Code 和 Codex。

站长可以在 [TransitHub](https://github.com/deviseo/transit-hub) 里定制品牌（logo、名称、主题色）、推荐配置和功能开关，用户的工单也由 TransitHub 处理。

## 下载安装

在 [Releases](https://github.com/deviseo/transitlink/releases/latest) 页面下载最新版本：

- **macOS**（Apple 芯片和 Intel 通用）：下载 `.dmg`，打开后把 TransitLink 拖进「应用程序」。
- **Windows 10 / 11**：下载 `-setup.exe`，双击安装。

安装包暂时没有正式的开发者签名，第一次打开时系统会拦一下：

- **macOS**：如果提示「无法打开」或「已损坏」，到「系统设置 → 隐私与安全性」点「仍要打开」；或者在终端运行 `xattr -dr com.apple.quarantine /Applications/TransitLink.app` 后再打开。
- **Windows**：如果出现「Windows 已保护你的电脑」，点「更多信息」→「仍要运行」。

## 当前进度（第一期）

- [x] 项目骨架：Tauri 2 + React + TypeScript；中英文界面；浅色、深色跟随系统
- [x] 检测本机的 Claude Code 和 Codex：安装位置、版本、配置文件位置
- [x] 注册 `transitlink://` 专属链接协议，单实例运行
- [x] 接入站点：输入网址后自动识别 new-api 和 sub2api，可以同时接入多个站点
- [x] 账号密码登录 new-api（新版和旧版）和 sub2api，支持两步验证；显示余额
- [x] Key 管理：查看、新建（可选分组）、一键复制
- [x] 一键配置 Claude Code 和 Codex：自动选分组、建专用 Key、测试连通后再写入；写前备份、可还原、可换模型
- [x] 托盘（macOS 是菜单栏）里切换站点：配置过的工具一起切过去，沿用在那个站点上选过的分组和模型；关掉窗口后留在托盘里运行
- [x] 开启了人机验证（Turnstile 等）的站点：在应用里打开站点自己的登录页登录，登录后自动接入
- [x] 专属链接一键接入：换上站长在 TransitHub 里设置的品牌（名称、logo、主题色、欢迎语），首页显示公告和客服，一键配置用站长推荐的分组和模型
- [x] GitHub Actions 自动构建安装包，发布到 GitHub Releases
- [x] 应用内自动更新（从第一个正式发布的版本开始生效）

## 一键配置会改哪些文件

- **Claude Code**：`~/.claude/settings.json` 的 `env` 里写入站点地址（`ANTHROPIC_BASE_URL`）和 Key（`ANTHROPIC_AUTH_TOKEN`），选了模型时还有 `ANTHROPIC_MODEL`；`~/.claude.json` 里标记已完成首次引导，免得第一次启动就要求登录 Anthropic 账号。
- **Codex**：`~/.codex/config.toml` 里加一个 `[model_providers.transitlink]`（站点地址和 Key），再把 `model_provider`、`model` 指向它。不动 `auth.json`，ChatGPT 账号登录不受影响。
- 会盖过上面这些设置的旧值（例如别的工具写的 `ANTHROPIC_API_KEY`、`apiKeyHelper`、`openai_base_url`）会先清掉，其他设置、注释和格式都保留。
- 写之前先用这个 Key 测试连通，测试不通过就什么都不改。每次修改前把原文件备份到 `~/.transitlink/backups/`，第一次修改前的原文件一直保留。
- 「还原」只把 TransitLink 改过的几项改回第一次配置之前的样子，你后来加的其他设置不受影响。
- 用 `CLAUDE_CONFIG_DIR`、`CODEX_HOME` 换过配置目录的，按换过的目录处理。

## 站长：给用户发专属链接

在 [TransitHub](https://github.com/deviseo/transit-hub) 的「客户端」页面设置品牌、推荐的分组和模型、公告和客服，然后把生成的 `transitlink://connect?config=…` 链接发给用户。用户点链接（或者把链接粘贴到「接入站点」的网址栏）就能接入，登录后自动打开一键配置。

- 链接里是 TransitHub 的公开配置接口 `…/api/client/v1/sites/<代码>`，用户的电脑要能访问它；TransitHub 只需要对外开放 `/api/client/` 开头的接口。
- 配置只接受 https 地址；配置里的站点地址必须和用户登录的站点一致，别的链接改不了这个站点的品牌。
- 已经用网址接入过的站点，把专属链接粘贴到「接入站点」里，点「直接使用」就能换上站长的品牌，不用重新登录。
- 站长改了配置后，TransitLink 启动时和运行中每半小时检查一次（超过 10 分钟的配置才重新读取）。

## 账号安全

- 站点密码只用于登录那一次，不会保存。
- 开了人机验证的站点在无痕登录窗口里登录，窗口关掉后什么都不留；TransitLink 只从窗口里取走登录结果，登录页本身不能调用应用的任何功能。
- 登录后得到的凭证存在系统钥匙串里（macOS 钥匙串、Windows 凭据管理器）。new-api 新版用的是只带必要权限的个人访问令牌，不能充值或改账号安全设置，可以随时在站点的个人设置里删除。
- 界面上的 Key 都是打码的；点复制时，桌面端直接把完整 Key 写进剪贴板。
- 写进 Claude Code、Codex 配置文件的 Key 是明文，这是这两个工具自己的要求；TransitLink 新建的配置文件和备份都只有你自己的账户能读。
- 所有请求都由桌面端直接发给你接入的站点，不经过其他服务器。

## 开发

需要的环境：

- Node.js 22+、Rust stable（1.94+）
- macOS：Xcode Command Line Tools
- Windows：MSVC 构建工具（WebView2 在 Windows 10 和 11 上已自带）

```bash
npm install
npm run tauri dev            # 运行桌面端（开发模式）
npm run dev                  # 只在浏览器里预览界面，显示示例数据
npm run build                # 前端类型检查和构建
(cd src-tauri && cargo test) # Rust 单元测试
npm run tauri build          # 打包安装包（需要更新签名密钥，见下面）
```

安装包里带自动更新，打包时要用更新签名密钥给更新包签名：设置环境变量 `TAURI_SIGNING_PRIVATE_KEY`（密钥文件路径或内容）和 `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`。GitHub Actions 从仓库的同名 secret 读取。只是运行开发模式（`npm run tauri dev`）不需要。

发布新版本：改 `src-tauri/tauri.conf.json`、`src-tauri/Cargo.toml`、`package.json` 里的版本号，推送 `v<版本号>` 标签（或在 Actions 里手动运行 Release）。工作流会生成 Release 草稿，里面有安装包、签过名的更新包和 `latest.json`；检查后在 GitHub 上发布。发布之后，已安装的应用会在启动时提示更新（草稿和预发布版不会推送给用户）。

国内网络下 npm 可以用镜像；拉取 Rust 依赖慢时，可以设置 `CARGO_HTTP_PROXY=http://127.0.0.1:<端口>`。

## 目录

```
src/                     React 前端
  components/            布局和通用组件
  components/sites/      接入站点、站点卡片、Key 管理
  components/tools/      一键配置窗口
  pages/                 首页、站点、工具、设置
  i18n/                  中英文文案
  lib/                   调用 Rust 命令；在浏览器预览时返回示例数据
  brand.ts               当前显示的品牌（默认 TransitLink，以后由站点配置替换）
src-tauri/               Rust 核心
  src/lib.rs             应用入口：插件、单实例、专属链接
  src/commands.rs        前端可调用的命令（含检查和安装更新）
  src/accounts.rs        站点账号：登录流程、凭证续期、Key
  src/sites/             new-api 和 sub2api 的接口、登录密码加密
  src/store.rs           站点列表（~/.transitlink/sites.json，不含凭证）
  src/secrets.rs         凭证存取（系统钥匙串）
  src/configure/         一键配置：Claude Code、Codex 的配置读写，备份、还原，连通测试
  src/tools.rs           本机编程工具检测
  src/tray.rs            托盘菜单：切换站点、打开、退出
  src/webauth.rs         开了人机验证的站点：登录窗口
  src/siteconfig.rs      专属链接和站长配置（品牌、推荐、公告、客服）
  src/settings.rs        应用设置（~/.transitlink/settings.json）
  src/paths.rs           本地目录（~/.transitlink）
```

## 致谢

决定 Claude Code、Codex 连到哪里、用什么鉴权的配置项清单，参考了 [cc-switch](https://github.com/farion1231/cc-switch)（MIT 许可）的整理。
