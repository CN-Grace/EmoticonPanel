// 表情面板 — 纯 Rust egui 版 (1:1 复刻 Tauri 版微信风格视觉)
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod core;

use core::{Attach, Sticker};
use egui::{
    pos2, vec2, Align, Align2, Color32, FontId, Frame, Id, Layout, Margin, Pos2, Rect, RichText,
    Rounding, Sense, Stroke, TextureHandle, TextureOptions, Vec2, epaint::TextureId,
};
use std::collections::HashMap;
use std::io::Cursor;
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::Duration;

const THUMB_MAX: u32 = 96;
const PREVIEW_MAX: u32 = 400;
const CELL_W: f32 = 76.0;
const CELL_H: f32 = 97.0;
const IMG: f32 = 72.0;
const COLS: usize = 4;
const W: f32 = 352.0;
const H: f32 = 486.0;
const BOTTOM_H: f32 = 46.0;
const MAX_INFLIGHT: usize = 6;

// 微信风格配色
const C_WHITE: Color32 = Color32::WHITE;
const C_BAR: Color32 = Color32::from_rgb(247, 247, 247);
const C_LINE: Color32 = Color32::from_rgb(229, 229, 229);
const C_HOVER: Color32 = Color32::from_rgb(242, 242, 242);
const C_TEXT: Color32 = Color32::from_rgb(26, 26, 26);
const C_DIM: Color32 = Color32::from_rgb(138, 138, 138);
const C_GREEN: Color32 = Color32::from_rgb(7, 193, 96);
const C_ORANGE: Color32 = Color32::from_rgb(255, 149, 0);
const C_RED: Color32 = Color32::from_rgb(250, 81, 81);

fn fit_dim(w: u32, h: u32, max: u32) -> (u32, u32) {
    if w == 0 || h == 0 {
        return (1, 1);
    }
    let scale = (max as f32 / w.max(h) as f32).min(1.0);
    (((w as f32 * scale) as u32).max(1), ((h as f32 * scale) as u32).max(1))
}

fn trunc(s: &str, n: usize) -> String {
    let mut chars = s.chars();
    let mut out: String = chars.by_ref().take(n).collect();
    if chars.next().is_some() {
        out.push('…');
    }
    out
}

fn decode_frames(bytes: &[u8], ctx: &egui::Context, max: u32, tag: &str) -> Option<(Vec<TextureHandle>, Vec<u64>)> {
    let fmt = image::ImageReader::new(Cursor::new(bytes)).with_guessed_format().ok()?.format()?;
    let mut frames: Vec<egui::ColorImage> = Vec::new();
    let mut delays: Vec<u64> = Vec::new();
    if fmt == image::ImageFormat::Gif {
        use image::AnimationDecoder;
        let decoder = image::codecs::gif::GifDecoder::new(Cursor::new(bytes)).ok()?;
        let decoded = decoder.into_frames().collect_frames().ok()?;
        for (i, frame) in decoded.into_iter().enumerate() {
            if i >= 48 {
                break;
            }
            let (n, d) = frame.delay().numer_denom_ms();
            delays.push(if d == 0 { 100 } else { ((n as f64 / d as f64) * 1000.0).max(20.0) as u64 });
            let buf = frame.into_buffer();
            let (nw, nh) = fit_dim(buf.width(), buf.height(), max);
            let small = image::imageops::resize(&buf, nw, nh, image::imageops::FilterType::Triangle);
            frames.push(egui::ColorImage::from_rgba_unmultiplied([nw as usize, nh as usize], small.as_raw()));
        }
    } else {
        let buf = image::load_from_memory(bytes).ok()?.to_rgba8();
        let (nw, nh) = fit_dim(buf.width(), buf.height(), max);
        let small = image::imageops::resize(&buf, nw, nh, image::imageops::FilterType::Triangle);
        frames.push(egui::ColorImage::from_rgba_unmultiplied([nw as usize, nh as usize], small.as_raw()));
        delays.push(0);
    }
    if frames.is_empty() {
        return None;
    }
    let handles = frames
        .into_iter()
        .enumerate()
        .map(|(i, c)| ctx.load_texture(format!("{tag}#{i}"), c, TextureOptions::LINEAR))
        .collect();
    Some((handles, delays))
}

struct Avatar {
    frames: Vec<TextureHandle>,
    delays: Vec<u64>,
    current: usize,
    last_time: f64,
}

struct App {
    attach: Attach,
    root: PathBuf,
    packages: Vec<core::Package>,
    stickers: Vec<Sticker>,
    current: usize,
    thumbs: HashMap<PathBuf, Avatar>,
    previews: HashMap<PathBuf, TextureHandle>,
    tab_cover: Vec<Option<TextureHandle>>,
    fallback: TextureHandle,
    show_settings: bool,
    menu: Option<(Pos2, usize)>, // 右键菜单位置 + 分组索引
    hover_pv: Option<(Pos2, TextureId)>,
    toast: Option<(String, std::time::Instant)>,
    folder_rx: Option<mpsc::Receiver<Option<PathBuf>>>,
    // 后台解码
    pending: std::collections::VecDeque<(PathBuf, Vec<u8>)>,
    inflight: usize,
    done_rx: std::sync::mpsc::Receiver<(PathBuf, Vec<TextureHandle>, Vec<u64>)>,
    done_tx: std::sync::mpsc::Sender<(PathBuf, Vec<TextureHandle>, Vec<u64>)>,
    tabs_off: f32,
    tab_hover_last: Option<usize>,
}

impl App {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        install_fonts(&cc.egui_ctx);
        cc.egui_ctx.set_visuals(egui::Visuals::light());
        let fallback = cc
            .egui_ctx
            .load_texture("fb", egui::ColorImage::from_rgba_unmultiplied([4, 4], &[230; 64]), TextureOptions::LINEAR);
        let root = core::root_dir();
        let packages = core::list_packages(&root);
        let (done_tx, done_rx) = std::sync::mpsc::channel();
        let mut a = Self {
            attach: Attach::default(),
            root,
            packages,
            stickers: Vec::new(),
            current: 0,
            thumbs: HashMap::new(),
            previews: HashMap::new(),
            tab_cover: Vec::new(),
            fallback,
            show_settings: false,
            menu: None,
            hover_pv: None,
            toast: None,
            folder_rx: None,
            pending: std::collections::VecDeque::new(),
            inflight: 0,
            done_rx,
            done_tx,
            tabs_off: 0.0,
            tab_hover_last: None,
        };
        a.load_group(&cc.egui_ctx);
        a
    }

    fn load_group(&mut self, ctx: &egui::Context) {
        self.stickers = self
            .packages
            .get(self.current)
            .and_then(|p| core::list_stickers(&self.root, &p.name).ok())
            .unwrap_or_default();
        self.thumbs.clear();
        self.previews.clear();
        self.pending.clear();
        self.tabs_off = 0.0;
        // 读字节并入队后台解码 (最后一张先排, 保证可视区先出)
        let mut paths: Vec<PathBuf> = self.stickers.iter().map(|s| s.path.clone()).collect();
        paths.reverse();
        for p in paths {
            if let Ok(bytes) = std::fs::read(&p) {
                self.pending.push_front((p, bytes));
            }
        }
        let _ = ctx;
        self.rebuild_tab_covers(ctx);
    }

    /// 每帧: 启动 ≤MAX_INFLIGHT 个解码线程 + 收完成结果
    fn pump_decode(&mut self, ctx: &egui::Context) {
        while self.inflight < MAX_INFLIGHT {
            let Some((path, bytes)) = self.pending.pop_front() else { break };
            let tx = self.done_tx.clone();
            let ctx = ctx.clone();
            let tag = path.to_string_lossy().to_string();
            self.inflight += 1;
            std::thread::spawn(move || {
                let r = decode_frames(&bytes, &ctx, THUMB_MAX, &tag);
                let (frames, delays) = match r {
                    Some((f, d)) => (f, d),
                    None => (Vec::new(), Vec::new()),
                };
                let _ = tx.send((path, frames, delays));
            });
        }
        let mut got = 0;
        while let Ok((path, frames, delays)) = self.done_rx.try_recv() {
            self.inflight -= 1;
            got += 1;
            if !frames.is_empty() {
                // 若是某组封面, 同时更新 tab_cover
                for (i, p) in self.packages.iter().enumerate() {
                    if let Some(cover) = &self.tab_cover[i] {
                        if cover.id() == self.fallback.id() {
                            if let Ok(ss) = core::list_stickers(&self.root, &p.name) {
                                if ss.first().map(|f| f.path == path).unwrap_or(false) {
                                    self.tab_cover[i] = Some(frames[0].clone());
                                }
                            }
                        }
                    }
                }
                self.thumbs.insert(path, Avatar { frames, delays, current: 0, last_time: 0.0 });
            }
        }
        if got > 0 || !self.pending.is_empty() {
            ctx.request_repaint();
        }
    }

    fn rebuild_tab_covers(&mut self, ctx: &egui::Context) {
        self.tab_cover.clear();
        for _ in self.packages.iter() {
            self.tab_cover.push(Some(self.fallback.clone()));
        }
        ctx.request_repaint();
    }

    fn refresh(&mut self, ctx: &egui::Context) {
        self.root = core::root_dir();
        if self.current >= self.packages.len() {
            self.current = 0;
        }
        self.packages = core::list_packages(&self.root);
        self.load_group(ctx);
    }

    fn ensure_thumb(&mut self, ctx: &egui::Context, path: &std::path::Path) {
        if !self.thumbs.contains_key(path) {
            if let Ok(bytes) = std::fs::read(path) {
                let tag = path.to_string_lossy().to_string();
                if let Some((frames, delays)) = decode_frames(&bytes, ctx, THUMB_MAX, &tag) {
                    self.thumbs.insert(path.to_path_buf(), Avatar { frames, delays, current: 0, last_time: 0.0 });
                }
            }
        }
    }

    fn ensure_preview(&mut self, ctx: &egui::Context, st: &Sticker) {
        if !self.previews.contains_key(&st.path) {
            if let Ok(bytes) = std::fs::read(&st.path) {
                if let Ok(img) = image::load_from_memory(&bytes) {
                    let buf = img.to_rgba8();
                    let (nw, nh) = fit_dim(buf.width(), buf.height(), PREVIEW_MAX);
                    let small = image::imageops::resize(&buf, nw, nh, image::imageops::FilterType::Triangle);
                    let color = egui::ColorImage::from_rgba_unmultiplied([nw as usize, nh as usize], small.as_raw());
                    let h = ctx.load_texture(format!("pv_{}", st.path.to_string_lossy()), color, TextureOptions::LINEAR);
                    self.previews.insert(st.path.clone(), h);
                }
            }
        }
    }

    fn advance_animations(&mut self, ctx: &egui::Context) {
        let now = ctx.input(|i| i.time);
        let mut need = false;
        let mut min_delay = u64::MAX;
        let paths: Vec<PathBuf> = self.stickers.iter().map(|s| s.path.clone()).collect();
        for p in paths {
            if let Some(at) = self.thumbs.get_mut(&p) {
                if at.delays.len() > 1 {
                    let dt_ms = ((now - at.last_time) * 1000.0).max(0.0);
                    let mut acc = dt_ms as u64;
                    let mut guard = 0usize;
                    while acc >= at.delays[at.current] && guard < at.delays.len() {
                        acc -= at.delays[at.current];
                        at.current = (at.current + 1) % at.delays.len();
                        guard += 1;
                    }
                    at.last_time = now;
                    min_delay = min_delay.min(at.delays[at.current].max(1));
                    need = true;
                } else {
                    at.last_time = now;
                }
            }
        }
        if need {
            ctx.request_repaint_after(Duration::from_millis(min_delay.min(100)));
        }
    }

    fn sticker_cell(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        st: &Sticker,
    ) -> egui::Response {
        self.ensure_thumb(ctx, &st.path);
        let tex = self
            .thumbs
            .get(&st.path)
            .map(|at| at.frames[at.current].id())
            .unwrap_or(self.fallback.id());
        let (rect, resp) = ui.allocate_exact_size(vec2(CELL_W, CELL_H), Sense::click());
        let painter = ui.painter();
        if resp.hovered() {
            painter.rect(rect, Rounding::same(6.0), C_HOVER, Stroke::NONE);
        }
        // 75x75 图片
        let img_rect = Rect::from_center_size(rect.center_top() + vec2(0.0, IMG / 2.0 + 3.0), vec2(IMG, IMG));
        painter.image(tex, img_rect, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), C_WHITE);
        // 文件名
        painter.text(
            pos2(rect.center().x, rect.bottom() - 4.0),
            Align2::CENTER_BOTTOM,
            trunc(&st.name, 8),
            FontId::proportional(9.5),
            C_DIM,
        );
        // hover 大预览
        if resp.hovered() {
            self.hover_pv = Some((resp.hover_pos().unwrap_or(rect.left_top()), tex));
        }
        resp.on_hover_cursor(egui::CursorIcon::PointingHand)
    }

    fn draw_tab_chip(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        index: usize,
        name: &str,
        gif_count: usize,
        active: bool,
    ) -> egui::Response {
        let (rect, resp) = ui.allocate_exact_size(vec2(32.0, 32.0), Sense::click());
        let painter = ui.painter();
        let r = Rounding::same(6.0);
        let (fill, stroke) = if active {
            (C_WHITE, Stroke::new(1.0, C_LINE))
        } else if resp.hovered() {
            (Color32::from_rgb(236, 236, 236), Stroke::NONE)
        } else {
            (Color32::TRANSPARENT, Stroke::NONE)
        };
        painter.rect(rect, r, fill, stroke);
        // 封面
        let img_id = self.tab_cover.get(index).and_then(|c| c.as_ref()).map(|h| h.id()).unwrap_or(self.fallback.id());
        let ir = Rect::from_center_size(rect.center(), vec2(26.0, 26.0));
        painter.image(img_id, ir, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), C_WHITE);
        // GIF 徽标
        if gif_count > 0 {
            let b = Rect::from_min_size(pos2(rect.right() - 18.0, rect.bottom() - 12.0), vec2(17.0, 11.0));
            painter.rect(b, Rounding::same(3.0), C_ORANGE, Stroke::NONE);
            painter.text(b.center(), Align2::CENTER_CENTER, "GIF", FontId::proportional(7.0), C_WHITE);
        }
        let _ = ctx;
        resp
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.pump_decode(ctx);
        self.advance_animations(ctx);
        if self.attach.picking.load(std::sync::atomic::Ordering::SeqCst) {
            ctx.request_repaint_after(Duration::from_millis(120));
        }

        if let Some(rx) = &self.folder_rx {
            if let Ok(Some(dir)) = rx.try_recv() {
                if core::set_stickers_dir(&dir.to_string_lossy()).is_ok() {
                    self.toast = Some(("表情包位置已切换".into(), std::time::Instant::now()));
                    self.refresh(ctx);
                }
                self.folder_rx = None;
            }
        }

        // ---------- 底部栏 (微信风格灰底 + 上边框) ----------
        egui::TopBottomPanel::bottom("bottom")
            .exact_height(BOTTOM_H)
            .frame(Frame::none().fill(C_BAR).inner_margin(Margin::symmetric(8.0, 6.0)).stroke(Stroke::new(1.0, C_LINE)))
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    let mut names: Vec<String> = Vec::new();
                    let mut gif_counts: Vec<usize> = Vec::new();
                    for p in self.packages.iter() {
                        names.push(p.name.clone());
                        gif_counts.push(p.gif_count);
                    }
                    // ---- Tab 自绘 + 滚轮横向滚动 ----
                    let avail_w = ui.available_width() - 40.0; // 留 ⚙ 位置
                    let per = 36.0;
                    let total_w = names.len() as f32 * per - 4.0;
                    let max_off = (total_w - avail_w).max(0.0);
                    if max_off > 0.0 {
                        let dy = ui.input(|i| i.raw_scroll_delta.y);
                        if dy != 0.0 {
                            self.tabs_off = (self.tabs_off + dy).clamp(0.0, max_off);
                            // 平滑滚轮用 raw; 触控板平滑滚动同样生效
                        }
                    } else {
                        self.tabs_off = 0.0;
                    }
                    let (row_rect, row_resp) = ui.allocate_exact_size(vec2(avail_w, 32.0), Sense::hover());
                    let painter = ui.painter_at(row_rect);
                    let mut clicked = None;
                    let mut right_clicked = None;
                    let mut hovered_idx = None;
                    for (i, n) in names.iter().enumerate() {
                        let x = 2.0 + i as f32 * per - self.tabs_off;
                        let chip_rect = Rect::from_min_size(pos2(row_rect.min.x + x, row_rect.min.y), vec2(32.0, 32.0));
                        if !chip_rect.intersects(row_rect) {
                            continue;
                        }
                        let resp = ui.interact(chip_rect, Id::new(("tab", i)), Sense::click());
                        let active = self.current == i;
                        let (fill, stroke) = if active {
                            (C_WHITE, Stroke::new(1.0, C_LINE))
                        } else if resp.hovered() {
                            (Color32::from_rgb(236, 236, 236), Stroke::NONE)
                        } else {
                            (Color32::TRANSPARENT, Stroke::NONE)
                        };
                        painter.rect(chip_rect, Rounding::same(6.0), fill, stroke);
                        let img_id = self
                            .tab_cover
                            .get(i)
                            .and_then(|c| c.as_ref())
                            .map(|h| h.id())
                            .unwrap_or(self.fallback.id());
                        let ir = Rect::from_center_size(chip_rect.center(), vec2(26.0, 26.0));
                        painter.image(img_id, ir, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), C_WHITE);
                        if gif_counts[i] > 0 {
                            let b = Rect::from_min_size(pos2(chip_rect.right() - 18.0, chip_rect.bottom() - 12.0), vec2(17.0, 11.0));
                            painter.rect(b, Rounding::same(3.0), C_ORANGE, Stroke::NONE);
                            painter.text(b.center(), Align2::CENTER_CENTER, "GIF", FontId::proportional(7.0), C_WHITE);
                        }
                        if resp.clicked() {
                            clicked = Some(i);
                        }
                        if resp.secondary_clicked() {
                            right_clicked = Some((ctx.pointer_latest_pos().unwrap_or(row_rect.left_top()), i));
                        }
                        if resp.hovered() {
                            hovered_idx = Some(i);
                        }
                    }
                    if hovered_idx != self.tab_hover_last {
                        self.tab_hover_last = hovered_idx;
                        if let Some(i) = hovered_idx {
                            ui.output_mut(|o| o.cursor_icon = egui::CursorIcon::PointingHand);
                        }
                    }
                    ui.allocate_space(vec2(0.0, 0.0));
                    // ⚙ 按钮
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let (g, g_resp) = ui.allocate_exact_size(vec2(32.0, 32.0), Sense::click());
                        let p = ui.painter();
                        p.rect(g, Rounding::same(6.0), if g_resp.hovered() { Color32::from_rgb(236, 236, 236) } else { Color32::TRANSPARENT }, Stroke::NONE);
                        p.text(g.center(), Align2::CENTER_CENTER, "⚙", FontId::proportional(16.0), C_TEXT);
                        if g_resp.clicked() {
                            self.show_settings = !self.show_settings;
                        }
                    });
                    if let Some(i) = clicked {
                        self.current = i;
                        self.load_group(ctx);
                    }
                    if let Some((pos, i)) = right_clicked {
                        self.menu = Some((pos, i));
                    }
                    let _ = row_resp;
                });
            });

        // ---------- 右键菜单 (自绘) ----------
        if let Some((pos, idx)) = self.menu.clone() {
            let pkg = self.packages.get(idx).cloned();
            if let Some(pkg) = pkg {
                let mw = 130.0;
                let mh = 34.0;
                let mut open = true;
                egui::Area::new(Id::new("ctxmenu"))
                    .fixed_pos(pos)
                    .order(egui::Order::Foreground)
                    .show(ctx, |ui| {
                        let inner = Frame::none()
                            .fill(C_WHITE)
                            .rounding(6.0)
                            .stroke(Stroke::new(1.0, C_LINE))
                            .inner_margin(Margin::symmetric(12.0, 0.0))
                            .shadow(egui::epaint::Shadow { offset: vec2(0.0, 2.0), blur: 12.0, spread: 0.0, color: Color32::from_black_alpha(40) });
                        inner.show(ui, |ui| {
                            let (r, resp) = ui.allocate_exact_size(vec2(100.0, mh), Sense::click());
                            ui.painter().text(r.center(), Align2::CENTER_CENTER, format!("删除「{}」", trunc(&pkg.name, 6)), FontId::proportional(13.0), C_RED);
                            if resp.clicked() {
                                open = false;
                                if core::delete_package(&self.root, &pkg.name).is_ok() {
                                    self.toast = Some((format!("已删除「{}」", pkg.name), std::time::Instant::now()));
                                }
                                self.refresh(ctx);
                            }
                        });
                    });
                if !open {
                    self.menu = None;
                }
            } else {
                self.menu = None;
            }
        }

        // ---------- 设置面板 (自绘浮层, 行区块背景) ----------
        if self.show_settings {
            egui::Area::new(Id::new("settings"))
                .anchor(Align2::RIGHT_BOTTOM, vec2(-8.0, -(BOTTOM_H + 8.0)))
                .order(egui::Order::Foreground)
                .show(ctx, |ui| {
                    Frame::none()
                        .fill(C_WHITE)
                        .rounding(10.0)
                        .stroke(Stroke::new(1.0, C_LINE))
                        .inner_margin(Margin::same(10.0))
                        .shadow(egui::epaint::Shadow { offset: vec2(0.0, 4.0), blur: 18.0, spread: 0.0, color: Color32::from_black_alpha(46) })
                        .show(ui, |ui| {
                            ui.set_width(316.0);
                            // 行区块: 浅灰圆角背景, 突出按钮
                            let row = |ui: &mut egui::Ui, label: &str, value: String, btn: &str, filled: bool, need_val: bool, cap: usize| {
                                let mut clicked = false;
                                Frame::none()
                                    .fill(Color32::from_rgb(245, 245, 245))
                                    .rounding(8.0)
                                    .inner_margin(Margin::symmetric(10.0, 7.0))
                                    .show(ui, |ui| {
                                        ui.horizontal(|ui| {
                                            ui.label(RichText::new(label).size(13.0).color(C_TEXT));
                                            let val = trunc(&value, cap);
                                            ui.add_sized([130.0, 22.0], egui::Label::new(RichText::new(if need_val { &val } else { "" }).size(12.0).color(if need_val { C_TEXT } else { C_DIM })).truncate());
                                            if chip(ui, vec2(62.0, 28.0), btn, filled).clicked() {
                                                clicked = true;
                                            }
                                        });
                                    });
                                clicked
                            };

                            // 目标窗口
                            let t = self.attach.target.lock().unwrap().clone();
                            let picking = self.attach.picking.load(std::sync::atomic::Ordering::SeqCst);
                            let status = if picking {
                                "正在选择…点击目标窗口".to_string()
                            } else if let Some(t) = &t {
                                format!("{} · {}", t.process, trunc(&t.title, 8))
                            } else {
                                "未选择".to_string()
                            };
                            if row(ui, "目标窗口", status, if picking { "取消" } else { if t.is_some() { "重选" } else { "选择" } }, !picking, false, 16) {
                                if picking {
                                    core::cancel_pick(&self.attach);
                                } else {
                                    core::begin_pick(&self.attach);
                                    self.toast = Some(("请在 15 秒内点击目标窗口".into(), std::time::Instant::now()));
                                }
                            }
                            ui.add_space(8.0);
                            // 刷新
                            if row(ui, "刷新表情包", "重新扫描分组".to_string(), "刷新", false, false, 10) {
                                self.refresh(ctx);
                                self.toast = Some(("已刷新表情包".into(), std::time::Instant::now()));
                            }
                            ui.add_space(8.0);
                            // 位置
                            if row(ui, "表情包位置", self.root.to_string_lossy().to_string(), "选文件夹", false, true, 16) {
                                let (tx, rx) = mpsc::channel();
                                self.folder_rx = Some(rx);
                                std::thread::spawn(move || {
                                    let dir = rfd::FileDialog::new().set_title("选择表情包文件夹").pick_folder();
                                    let _ = tx.send(dir);
                                });
                            }
                        });
                    ui.allocate_space(vec2(0.0, 6.0));
                });
        }

        // ---------- 网格 (Grid 固定列宽对齐, 隐藏滚动条) ----------
        egui::CentralPanel::default()
            .frame(Frame::none().fill(C_WHITE))
            .show(ctx, |ui| {
                egui::ScrollArea::vertical()
                    .id_salt("grid")
                    .auto_shrink([false, false])
                    .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
                    .show(ui, |ui| {
                        ui.add_space(6.0);
                        let stickers = self.stickers.clone();
                        let mut insert_err: Option<String> = None;
                        egui::Grid::new("stkgrid")
                            .num_columns(COLS)
                            .min_col_width(CELL_W)
                            .max_col_width(CELL_W)
                            .spacing(vec2(5.0, 0.0))
                            .show(ui, |ui| {
                                for (i, st) in stickers.iter().enumerate() {
                                    let resp = self.sticker_cell(ui, ctx, st);
                                    if resp.clicked() {
                                        match core::insert_sticker(&self.attach, &self.root, &st.path) {
                                            Ok(()) => {
                                                let proc = self.attach.target.lock().unwrap().clone().map(|t| t.process).unwrap_or_default();
                                                self.toast = Some((format!("已插入 → {proc}"), std::time::Instant::now()));
                                            }
                                            Err(e) => {
                                                insert_err = Some(e);
                                            }
                                        }
                                    }
                                    if (i + 1) % COLS == 0 {
                                        ui.end_row();
                                    }
                                }
                            });
                        if let Some(e) = insert_err {
                            self.toast = Some((e, std::time::Instant::now()));
                        }
                        if self.stickers.is_empty() {
                            ui.centered_and_justified(|ui| {
                                ui.label(RichText_13("还没有表情包 — 在 ⚙ 设置里选择位置或刷新").colored_opt(C_DIM));
                            });
                        }
                        ui.add_space(10.0);
                    });
            });

        // ---------- hover 大预览 ----------
        if let Some((pos, tid)) = self.hover_pv.clone() {
            let size = vec2(150.0, 150.0);
            let mut r = Rect::from_min_size(pos + vec2(16.0, 10.0), size);
            let max_rect = ctx.screen_rect();
            if r.right() > max_rect.right() {
                r = r.translate(vec2(-(r.width() + 32.0), 0.0));
            }
            egui::Area::new(Id::new("hpv"))
                .fixed_pos(r.min)
                .order(egui::Order::Foreground)
                .show(ctx, |ui| {
                    Frame::none()
                        .fill(C_WHITE)
                        .rounding(8.0)
                        .inner_margin(Margin::same(6.0))
                        .shadow(egui::epaint::Shadow { offset: vec2(0.0, 3.0), blur: 16.0, spread: 0.0, color: Color32::from_black_alpha(50) })
                        .show(ui, |ui| {
                            ui.add(egui::Image::new(egui::load::SizedTexture::new(tid, size)).fit_to_exact_size(size));
                        });
                });
            self.hover_pv = None;
        }

        // ---------- toast ----------
        if let Some((msg, t0)) = &self.toast {
            if t0.elapsed() < Duration::from_millis(1800) {
                egui::Area::new(Id::new("toast"))
                    .anchor(Align2::CENTER_BOTTOM, vec2(0.0, -(BOTTOM_H + 14.0)))
                    .order(egui::Order::Foreground)
                    .show(ctx, |ui| {
                        Frame::none()
                            .fill(Color32::from_rgba_unmultiplied(0, 0, 0, 200))
                            .rounding(6.0)
                            .inner_margin(Margin::symmetric(12.0, 7.0))
                            .show(ui, |ui| {
                                ui.label(RichText_12(msg).colored(C_WHITE));
                            });
                    });
            } else {
                self.toast = None;
            }
        }
    }
}

fn RichText_13(s: &str) -> egui::RichText {
    egui::RichText::new(s).size(13.0).color(C_TEXT)
}
fn RichText_12(s: &str) -> egui::RichText {
    egui::RichText::new(s).size(12.0).color(C_TEXT)
}
fn RichText_11(s: &str) -> egui::RichText {
    egui::RichText::new(s).size(11.0).color(C_DIM)
}

trait ColoredOpt {
    fn colored_opt(self, c: Color32) -> egui::RichText;
}
impl ColoredOpt for egui::RichText {
    fn colored_opt(self, _c: Color32) -> egui::RichText {
        self
    }
}
trait RichTextExt {
    fn colored(self, c: Color32) -> egui::RichText;
}
impl RichTextExt for egui::RichText {
    fn colored(self, c: Color32) -> egui::RichText {
        self.color(c)
    }
}

fn chip(ui: &mut egui::Ui, size: Vec2, text: &str, filled: bool) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let painter = ui.painter();
    let r = Rounding::same(6.0);
    let (fill, fg, stroke) = if filled {
        (C_GREEN, C_WHITE, Stroke::new(1.0, C_GREEN))
    } else {
        let fill = if resp.hovered() { Color32::from_rgb(236, 236, 236) } else { C_WHITE };
        (fill, C_GREEN, Stroke::new(1.0, C_GREEN))
    };
    painter.rect(rect, r, fill, stroke);
    painter.text(rect.center(), Align2::CENTER_CENTER, text, FontId::proportional(12.0), fg);
    resp
}

fn install_fonts(ctx: &egui::Context) {
    use egui::{FontData, FontDefinitions, FontFamily};
    let mut fonts = FontDefinitions::default();
    for c in [
        r"C:\Windows\Fonts\simhei.ttf",
        r"C:\Windows\Fonts\Deng.ttf",
        r"C:\Windows\Fonts\Dengb.ttf",
        r"C:\Windows\Fonts\msyh.ttc",
        r"C:\Windows\Fonts\simsun.ttc",
    ] {
        if let Ok(bytes) = std::fs::read(c) {
            fonts.font_data.insert("cjk".into(), FontData::from_owned(bytes));
            for f in [FontFamily::Proportional, FontFamily::Monospace] {
                fonts.families.entry(f).or_default().insert(0, "cjk".into());
            }
            break;
        }
    }
    ctx.set_fonts(fonts);
}

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([W, H])
            .with_min_inner_size([300.0, 450.0])
            .with_title("表情面板"),
        ..Default::default()
    };
    eframe::run_native("表情面板", options, Box::new(|cc| Ok(Box::new(App::new(cc)))))
}