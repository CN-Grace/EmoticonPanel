# 表情面板 — egui 功能完备版分支

本分支仅含 **egui 纯 Rust 功能完备版** (`egui-app/`)。

- 功能: 表情网格 / 分组 Tab / 目标窗口附件 / 剪贴板插入 / GIF 悬浮动画
- 内存: ~120MB WS / ~250MB 私有 (96px 缩略图 + 全帧 GIF 常驻)
- 构建: `cd egui-app && cargo build --release` → `egui-app/target/release/emoticon-panel-egui.exe`

> 其他实现见分支: `main` (egui-lite 内存优化版) / `tauri` (Tauri/WebView2 版)
