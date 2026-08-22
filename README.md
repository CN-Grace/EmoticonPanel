# 表情面板 (EmoticonPanel)

**点表情 → 粘贴进目标窗口输入框**。仿微信表情面板的浏览交互(分组 Tab、4 列网格、悬浮动图预览)。

现行版本为 **egui 纯 Rust 版**,分两个变体:
- `egui-app/` — 功能完备用例(完整缓存,内存 ~145MB WS / ~270MB 私有)
- `egui-app-lite/` — **内存优化版**(默认推荐):缩略图 72px、GIF 动画帧按 hover 按需解码 + LRU 逐出(默认仅常驻首帧)、bounded 任务队列、3 worker → **稳态 WS ~104MB(-28%) / 私有 ~123MB(-54%)**

早期 **Tauri 版**已归档(见下)。

## egui 版(现行)

```bash
# 推荐(内存优化版)
cd egui-app-lite && cargo build --release
# → egui-app-lite/target/release/emoticon-panel-lite.exe (单文件, 绿色运行)

# 功能完备原版
cd egui-app && cargo build --release
# → egui-app/target/release/emoticon-panel-egui.exe
```

- 表情包目录: `%APPDATA%\EmoticonPanel-egui\stickers`(也可 ⚙→选文件夹 指定并持久化 `settings.json`; 环境变量 `EMOTICON_STICKERS_DIR` 优先)
- 每个子文件夹 = 一个分组;`cover.*` 作 Tab 封面;支持 png/gif/jpg/webp/bmp 混装
- 交互: 4 列网格(75px 图 + 文件名)上下滚动 · 底部 Tab(滚轮横滑/右键删除) · ⚙ 设置(目标窗口选择/刷新/位置) · **悬浮 GIF 原位实时播放动图**(按帧 delay) · 点击表情 **CF_HDROP 剪贴板 + Ctrl+V 插入原文件**(透明/动图保留)

## 归档与版本

| 版本 | tag | 可执行文件 |
|---|---|---|
| Tauri 版(完整历史) | `tauri-v1` | `archives/emoticon-panel-tauri-v1.exe` (29MB, 内存 ~480MB) |
| egui 版(功能完备) | `egui-v1` | `archives/emoticon-panel-egui-v1.exe` (4.8MB) |
| egui lite 版(内存优化) | 当前默认 | `egui-app-lite/target/release/emoticon-panel-lite.exe` (4.8MB) |

Tauri 源码仍在仓库根目录(`src-tauri/`,完整可用);egui 源码在 `egui-app/`。

## 测试(egui 版)

```bash
cd egui-app && cargo test --release decode_smoke   # 解码冒烟 (须先停运行中程序)
```
诊断脚本:`scripts/shot.ps1`(按 PID 枚举窗口截图)。