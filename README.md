# TransitLink

中转站用户客户端：登录 new-api 或 sub2api 搭建的中转站，管理 Key，一键配置 Claude Code 和 Codex。

站长可以在 [TransitHub](https://github.com/deviseo/transit-hub) 里定制品牌（logo、名称、主题色）、推荐配置和功能开关，用户的工单也由 TransitHub 处理。

## 当前进度（第一期）

- [x] 项目骨架：Tauri 2 + React + TypeScript；中英文界面；浅色、深色跟随系统
- [x] 检测本机的 Claude Code 和 Codex：安装位置、版本、配置文件位置
- [x] 注册 `transitlink://` 专属链接协议，单实例运行
- [ ] 接入站点：专属链接、输入网址后自动识别
- [ ] 登录 new-api 和 sub2api（两步验证、人机验证窗口）、凭证存入系统钥匙串、显示余额
- [ ] Key 管理；一键配置 Claude Code 和 Codex（写前备份、可还原、连通测试）；托盘切换站点
- [ ] 读取 TransitHub 下发的站长品牌和推荐配置
- [ ] 自动更新和安装包发布

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

国内网络下 npm 可以用镜像；拉取 Rust 依赖慢时，可以设置 `CARGO_HTTP_PROXY=http://127.0.0.1:<端口>`。

## 目录

```
src/                     React 前端
  components/            布局和通用组件
  pages/                 首页、站点、工具、设置
  i18n/                  中英文文案
  lib/                   调用 Rust 命令；在浏览器预览时返回示例数据
  brand.ts               当前显示的品牌（默认 TransitLink，以后由站点配置替换）
src-tauri/               Rust 核心
  src/lib.rs             应用入口：插件、单实例、专属链接
  src/commands.rs        前端可调用的命令
  src/tools.rs           本机编程工具检测
  src/paths.rs           本地目录（~/.transitlink）
```
