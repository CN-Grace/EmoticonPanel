# 表情面板 (EmoticonPanel)

仿微信表情面板的桌面应用 — **Tauri 2 + Rust + vanilla TypeScript**。支持自定义表情包(文件夹形式,gif/png 混装),分组 Tab、7×3 网格翻页、圆点指示、悬停大预览、表情商店(下载/删除分组)、发送/删除输入。

## 运行

```bash
# 开发模式 (热更新)
npm run tauri dev

# 正式构建 (产物在 src-tauri/target/release/emoticon-panel.exe)
npm run tauri build -- --no-bundle
```

## 表情包目录

默认根目录: `%APPDATA%\com.grace.emoticonpanel\stickers`
- 每个**子文件夹 = 一个表情包分组**(文件夹名即分组名)
- 支持扩展名: `png / gif / jpg / jpeg / webp / bmp`
- `cover.png`(或 cover.gif 等)作为分组封面,不计入表情网格
- 首次启动自动从内置示例 (`src-tauri/stickers/`) 播种示例包 + 商店
- 也可用环境变量指定目录: `EMOTICON_STICKERS_DIR=D:\我的表情` (指定后不自动播种)
- 放入自己的包后,点面板上的 **⭮ 刷新** 即可;右键分组 Tab 可删除该分组

商店包目录: `<表情根目录>/shop`,在面板点 **＋** 打开表情商店,「下载」即安装到分组。

## 测试

```bash
# Rust 后端单元测试 (注意: 请用 --release 跑, 见下方已知问题)
cd src-tauri && cargo test --release

# 前端 UI 驱动测试 (Playwright + 本机 Edge, mock Tauri 桥, 26 项断言)
node scripts/uitest.cjs        # 配合 npm run build 后的 dist

# 重新生成示例素材 (PIL 依赖: python -m pip install pillow)
python scripts/gen_stickers.py
```

## 技术要点

| 项 | 说明 |
|---|---|
| 工具链 | rustup 1.98.0 (GNU host `x86_64-pc-windows-gnu`) 由 **mise** 管理; 链接器 mingw-w64 16.2 (D:/dev/mingw64) |
| 图片传输 | Rust 命令读取后 base64 data-URL 返回前端,前端按路径缓存 |
| 安全 | 读取路径 canonicalize 后必须位于表情根目录内; 包名校验拒绝 `..`/`/`/`\`/特殊字符 |
| 首次播种 | `tauri build` 时资源随 exe 放置 (`target/release/stickers`), 纯 `cargo build` 时回退 `CARGO_MANIFEST_DIR` |

## 已知问题

- 本机 `cargo test`(debug 模式)链接出的测试二进制在 mingw 下加载报 `0xc0000139`(GNU debug 链接怪癖,应用本体 debug/release 均正常); 请用 **`cargo test --release`**。
- `cargo build` 有 `.rsrc merge failure: multiple non-default manifests` 链接警告,不影响产物运行。

## 目录结构

```
src/                 前端 (index.html / main.ts / styles.css)
src-tauri/src/lib.rs Rust 后端命令: list_packages, list_stickers, read_sticker,
                     shop_list, install_package, delete_package, get_root, reveal_root
src-tauri/stickers/  内置示例表情包 (samples/) 与商店 (shop/)
scripts/             gen_stickers.py 素材生成, uitest.cjs UI 驱动测试
```

Tauri 命令均为路径/包名强校验;删除与安装直接操作表情目录。