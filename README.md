# 表情面板 EmoticonPanel — egui 功能完备版

> 仿微信表情面板的桌面小工具:**点一下表情,自动粘贴进你想发送的窗口**(微信/QQ/任意聊天输入框)。
> egui 纯 Rust 实现,无浏览器内核。

本分支 = **egui 功能完备版**(`egui-app/`)。

---

## ✨ 功能特性

| 能力 | 说明 |
|---|---|
| 🗂️ 表情包分组 | 每个文件夹 = 一个分组,png / gif / jpg / webp / bmp 混装,`cover.*` 作 Tab 封面 |
| 🖼️ 4 列网格 | 75px 预览 + 文件名,上下滚动,默认窗口 318×445 |
| 🎞️ GIF 原位动画 | 悬浮动图**原地播放**(全帧常驻,切换组后动画立即有,无需等待解码) |
| 📌 目标窗口附件 | ⚙ → 目标窗口 → 点选任意窗口(微信等),绑定后点击表情即插入 |
| 📋 原生剪贴板注入 | CF_HDROP 真实文件路径 + Ctrl+V,透明/动图原样保留 |
| 🗂️ Tab 横滑 | 底部分组栏鼠标滚轮横滑;右键删除分组;GIF 徽标 |
| ⚙ 设置面板 | 目标窗口选择 / 刷新表情包 / 任意文件夹作根目录(持久化) |
| 🌍 全局数据位置 | `EMOTICON_STICKERS_DIR` > 设置面板选择 > 默认 appdata |
| 🔀 多实现 | 仓库含三种实现,本分支为功能完备基准版 |

## 🧰 技术栈

- **Rust** + [egui/eframe](https://github.com/emilk/egui)
- [image](https://crates.io/crates/image):图片解码 + GIF 逐帧动画
- [winapi](https://crates.io/crates/winapi):窗口枚举 / 前台激活 / 剪贴板 / 模拟 Ctrl+V
- **GB2312 中文字体子集**(2MB 内嵌,替代 19MB simhei)

## 🏗️ 与 lite 版差异

| 维度 | 本版 (egui) | lite 版 (main) |
|---|---|---|
| 缩略图 | 96px(放大更清晰) | 72px |
| GIF 动画 | **切换组即全帧常驻**,动画零等待 | 悬浮才解码 + LRU 逐出 |
| worker | 6 | 3 + 双队列 |
| 内存(WS) | ~120MB | **~82MB** |

功能完全一致;**本版换内存换取动画即时性**,lite 版换内存效率。

## 🚀 快速开始

```bash
cd egui-app && cargo build --release
# → egui-app/target/release/emoticon-panel-egui.exe (单文件, 绿色运行)
```

1. 双击运行,把表情包文件夹放进 `%APPDATA%\EmoticonPanel-egui\stickers\`(每个子文件夹一个分组)
2. ⚙ → 目标窗口 → 点选微信/聊天窗口
3. 点击表情 → 自动粘贴(Ctrl+V 原生注入,透明/动图保留)

## 🧪 测试

```bash
cd egui-app && cargo test --release decode_smoke
```

## 🔀 分支说明

| 分支 | 实现 | 内存(WS) |
|---|---|---|
| `main` | egui-lite 内存优化版 | ~82MB |
| **`egui`(本分支)** | egui 功能完备版 | ~120MB |
| `tauri` | Tauri 2 + WebView2 | ~465MB |

```bash
git checkout main / git checkout tauri   # 切换(切换会清掉忽略的 target/, 需重新 cargo build --release)
```

exe 归档在仓库外 `H:\VibeCoding\EmoticonPanel-archives\`。

## 📄 License

MIT。
