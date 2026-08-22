// 表情面板 — 纯 Rust egui 版 (1:1 复刻 Tauri 版微信风格视觉)
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod core;

use core::{Attach, Sticker};
use egui::{
    pos2, vec2, Align, Align2, Color32, FontId, Frame, Id, Layout, Margin, Pos2, Rect,
    Rounding, Sense, Stroke, TextureHandle, TextureOptions, Vec2, epaint::TextureId,
};
use std::collections::HashMap;
use std::io::Cursor;
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::Duration;

const THUMB_MAX: u32 = 96;
const PREVIEW_MAX: u32 = 400;
const CELL_W: f32 = 79.0;
const CELL_H: f32 = 98.0;
const IMG: f32 = 75.0;
const COLS: usize = 4;
const W: f32 = 350.0;
const H: f32 = 470.0;
const BOTTOM_H: f32 = 46.0;

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
            let small = image::imageops::resize(&buf, nw, nh, image::imageops::FilterType::Lanczos3);
            frames.push(egui::ColorImage::from_rgba_unmultiplied([nw as usize, nh as usize], small.as_raw()));
        }
    } else {
        let buf = image::load_from_memory(bytes).ok()?.to_rgba8();
        let (nw, nh) = fit_dim(buf.width(), buf.height(), max);
        let small = image::imageops::resize(&buf, nw, nh, image::imageops::FilterType::Lanczos3);
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
        let paths: Vec<PathBuf> = self.stickers.iter().map(|s| s.path.clone()).collect();
        for p in paths {
            self.ensure_thumb(ctx, &p);
        }
        self.rebuild_tab_covers(ctx);
    }

    fn rebuild_tab_covers(&mut self, ctx: &egui::Context) {
        self.tab_cover.clear();
        let names: Vec<String> = self.packages.iter().map(|p| p.name.clone()).collect();
        for name in names {
            let cover: Option<TextureHandle> = if let Ok(ss) = core::list_stickers(&self.root, &name) {
                ss.first().and_then(|st| {
                    self.ensure_thumb(ctx, &st.path);
                    self.thumbs.get(&st.path).map(|at| at.frames[at.current].clone())
                })
            } else {
                None
            };
            self.tab_cover.push(cover);
        }
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
                    let small = image::imageops::resize(&buf, nw, nh, image::imageops::FilterType::Lanczos3);
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

    // ---------- 绘制辅助 ----------

    fn chip(&self, ui: &mut egui::Ui, size: Vec2, text: &str, filled: bool) -> egui::Response {
        let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
        let painter = ui.painter();
        let rounding = Rounding::same(6.0);
        let bg = if resp.hovered() {
            C_HOVER
        } else if filled {
            C_GREEN
        } else {
            C_WHITE
        };
        let fg = if filled { C_WHITE } else { C_GREEN };
        painter.rect(rect, rounding, if filled { bg } else if resp.hovered() { C_HOVER } else { C_WHITE }, Stroke::new(1.0, C_GREEN));
        painter.text(rect.center(), Align2::CENTER_CENTER, text, FontId::proportional(12.0), fg);
        resp
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
                    let mut clicked = None;
                    let mut del = None;
                    let mut delname = String::new();
                    let mut names: Vec<String> = Vec::new();
                    let mut gif_counts: Vec<usize> = Vec::new();
                    for p in self.packages.iter() {
                        names.push(p.name.clone());
                        gif_counts.push(p.gif_count);
                    }
                    egui::ScrollArea::horizontal().id_salt("tabs").show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.add_space(2.0);
                            let mut right_clicked: Option<(Pos2, usize)> = None;
                            for (i, n) in names.iter().enumerate() {
                                let resp = self.draw_tab_chip(ui, ctx, i, n, gif_counts[i], self.current == i);
                                if resp.clicked() {
                                    clicked = Some(i);
                                }
                                if resp.secondary_clicked() {
                                    right_clicked = Some((ctx.pointer_latest_pos().unwrap_or(ui.min_rect().left_top()), i));
                                }
                            }
                            if let Some((pos, i)) = right_clicked {
                                del = Some(i);
                                delname = names[i].clone();
                                self.menu = Some((pos, i));
                            }
                        });
                    });
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
                    let _ = del;
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

        // ---------- 设置面板 (自绘浮层) ----------
        if self.show_settings {
            egui::Area::new(Id::new("settings"))
                .anchor(Align2::RIGHT_BOTTOM, vec2(-8.0, -(BOTTOM_H + 8.0)))
                .order(egui::Order::Foreground)
                .show(ctx, |ui| {
                    Frame::none()
                        .fill(C_WHITE)
                        .rounding(10.0)
                        .stroke(Stroke::new(1.0, C_LINE))
                        .inner_margin(Margin::same(8.0))
                        .shadow(egui::epaint::Shadow { offset: vec2(0.0, 4.0), blur: 18.0, spread: 0.0, color: Color32::from_black_alpha(46) })
                        .show(ui, |ui| {
                            ui.set_width(296.0);
                            // 目标窗口
                            ui.horizontal(|ui| {
                                ui.add_sized([62.0, 20.0], egui::Label::new(RichText_13("目标窗口")).truncate());
                                let t = self.attach.target.lock().unwrap().clone();
                                let picking = self.attach.picking.load(std::sync::atomic::Ordering::SeqCst);
                                let status = if picking {
                                    "正在选择…点击目标窗口".to_string()
                                } else if let Some(t) = &t {
                                    format!("{} · {}", t.process, trunc(&t.title, 10))
                                } else {
                                    "未选择".to_string()
                                };
                                ui.add_sized([150.0, 20.0], egui::Label::new(RichText_11(&status)).truncate());
                                if picking {
                                    if self.chip(ui, vec2(56.0, 26.0), "取消", false).clicked() {
                                        core::cancel_pick(&self.attach);
                                    }
                                } else if self.chip(ui, vec2(56.0, 26.0), if t.is_some() { "重选" } else { "选择" }, true).clicked() {
                                    core::begin_pick(&self.attach);
                                    self.toast = Some(("请在 15 秒内点击目标窗口".into(), std::time::Instant::now()));
                                }
                            });
                            let mut n = 12.0;
                            ui.add_space(n);
                            // 刷新
                            ui.horizontal(|ui| {
                                ui.add_sized([62.0, 20.0], egui::Label::new(RichText_13("刷新表情包")).truncate());
                                ui.add_sized([150.0, 20.0], egui::Label::new(RichText_11("重新扫描分组")));
                                if self.chip(ui, vec2(56.0, 26.0), "刷新", false).clicked() {
                                    self.refresh(ctx);
                                    self.toast = Some(("已刷新表情包".into(), std::time::Instant::now()));
                                }
                            });
                            ui.add_space(n);
                            // 位置
                            ui.horizontal(|ui| {
                                ui.add_sized([62.0, 20.0], egui::Label::new(RichText_13("表情包位置")).truncate());
                                ui.add_sized([150.0, 20.0], egui::Label::new(RichText_11(&trunc(&self.root.to_string_lossy(), 16))).truncate());
                                if self.chip(ui, vec2(76.0, 26.0), "选择文件夹", false).clicked() {
                                    let (tx, rx) = mpsc::channel();
                                    self.folder_rx = Some(rx);
                                    std::thread::spawn(move || {
                                        let dir = rfd::FileDialog::new().set_title("选择表情包文件夹").pick_folder();
                                        let _ = tx.send(dir);
                                    });
                                }
                            });
                        });
                    ui.allocate_space(vec2(0.0, 6.0));
                });
        }

        // ---------- 网格 ----------
        egui::CentralPanel::default()
            .frame(Frame::none().fill(C_WHITE))
            .show(ctx, |ui| {
                egui::ScrollArea::vertical().id_salt("grid").auto_shrink([false, false]).show(ui, |ui| {
                    ui.add_space(6.0);
                    let stickers = self.stickers.clone();
                    let mut insert_err: Option<String> = None;
                    for chunk in stickers.chunks(COLS) {
                        ui.horizontal(|ui| {
                            let total_w = (COLS as f32) * CELL_W;
                            ui.add_space((ui.available_width().max(0.0) - total_w) / 2.0);
                            for st in chunk {
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
                            }
                        });
                    }
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