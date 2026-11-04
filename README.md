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
- [ ] 一键配置 Claude Code 和 Codex（写前备份、可还原、连通测试）；托盘切换站点
- [ ] 开启了人机验证的站点：在登录窗口里完成验证
- [ ] 专属链接一键接入；读取 TransitHub 下发的站长品牌和推荐配置
- [x] GitHub Actions 自动构建安装包，发布到 GitHub Releases
- [ ] 应用内自动更新

## 账号安全

- 站点密码只用于登录那一次，不会保存。
- 登录后得到的凭证存在系统钥匙串里（macOS 钥匙串、Windows 凭据管理器）。new-api 新版用的是只带必要权限的个人访问令牌，不能充值或改账号安全设置，可以随时在站点的个人设置里删除。
- 界面上的 Key 都是打码的；点复制时，桌面端直接把完整 Key 写进剪贴板。
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
npm run tauri build          # 打包安装包
```

发布新版本：改 `src-tauri/tauri.conf.json`、`src-tauri/Cargo.toml`、`package.json` 里的版本号，推送 `v<版本号>` 标签（或在 Actions 里手动运行 Release）。工作流会生成 Release 草稿，检查后在 GitHub 上发布。

国内网络下 npm 可以用镜像；拉取 Rust 依赖慢时，可以设置 `CARGO_HTTP_PROXY=http://127.0.0.1:<端口>`。

## 目录

```
src/                     React 前端
  components/            布局和通用组件
  components/sites/      接入站点、站点卡片、Key 管理
  pages/                 首页、站点、工具、设置
  i18n/                  中英文文案
  lib/                   调用 Rust 命令；在浏览器预览时返回示例数据
  brand.ts               当前显示的品牌（默认 TransitLink，以后由站点配置替换）
src-tauri/               Rust 核心
  src/lib.rs             应用入口：插件、单实例、专属链接
  src/commands.rs        前端可调用的命令
  src/accounts.rs        站点账号：登录流程、凭证续期、Key
  src/sites/             new-api 和 sub2api 的接口、登录密码加密
  src/store.rs           站点列表（~/.transitlink/sites.json，不含凭证）
  src/secrets.rs         凭证存取（系统钥匙串）
  src/tools.rs           本机编程工具检测
  src/paths.rs           本地目录（~/.transitlink）
```
