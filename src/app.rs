use crate::{
    config::{AppConfig, CaptureRegion},
    monitor::{MonitorCommand, MonitorEvent, MonitorHandle},
    platform::{CapturedImage, NativePlatform, Platform, spawn_region_hotkey},
    print_log,
};
use eframe::egui::{self, Color32, Pos2, Rect, Sense, Stroke, Vec2, ViewportCommand, WindowLevel};
use std::sync::{Arc, mpsc::Receiver};

type SelectionCapture = Result<(CaptureRegion, CapturedImage), String>;

pub const MAIN_WINDOW_SIZE: Vec2 = Vec2::new(620.0, 470.0);

pub struct PigeonApp {
    config: AppConfig,
    platform: Arc<dyn Platform>,
    monitor: MonitorHandle,
    printers: Vec<String>,
    active: bool,
    status: String,
    last_ocr: String,
    selector: Option<RegionSelector>,
    hotkey: Option<Receiver<()>>,
    hotkey_requested: bool,
    selection_capture: Option<Receiver<SelectionCapture>>,
}

struct RegionSelector {
    screen: CaptureRegion,
    screenshot: CapturedImage,
    texture: Option<egui::TextureHandle>,
    start: Option<Pos2>,
    current: Option<Pos2>,
}

impl PigeonApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        cc.egui_ctx.set_zoom_factor(1.1);
        install_system_cjk_font(&cc.egui_ctx);
        let config = AppConfig::load();
        let platform: Arc<dyn Platform> = Arc::new(NativePlatform);
        let printers = platform.printers().unwrap_or_default();
        let monitor = MonitorHandle::spawn(Arc::clone(&platform), config.clone());
        let active = config.region.is_some() && !config.printer_name.trim().is_empty();
        let hotkey = spawn_region_hotkey().ok();
        if active {
            let _ = monitor.commands.send(MonitorCommand::SetActive(true));
        }

        Self {
            config,
            platform,
            monitor,
            printers,
            active,
            status: if active {
                "监控中".to_owned()
            } else {
                "请设定区域并选择打印机".to_owned()
            },
            last_ocr: String::new(),
            selector: None,
            hotkey,
            hotkey_requested: false,
            selection_capture: None,
        }
    }

    fn begin_selection(&mut self, ctx: &egui::Context) {
        if self.selection_capture.is_some() || self.selector.is_some() {
            return;
        }

        let _ = self.monitor.commands.send(MonitorCommand::SetActive(false));
        ctx.send_viewport_cmd(ViewportCommand::Minimized(true));
        let platform = Arc::clone(&self.platform);
        let (sender, receiver) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(300));
            let result = platform
                .virtual_screen()
                .and_then(|screen| platform.capture(screen).map(|image| (screen, image)));
            let _ = sender.send(result);
        });
        self.selection_capture = Some(receiver);
        self.status = "正在准备区域框选……".to_owned();
        ctx.request_repaint_after(std::time::Duration::from_millis(350));
    }

    fn show_selector(&mut self, ctx: &egui::Context, result: SelectionCapture) {
        match result {
            Ok((screen, screenshot)) => {
                let window_scale = ctx.pixels_per_point().max(1.0);
                self.selector = Some(RegionSelector {
                    screen,
                    screenshot,
                    texture: None,
                    start: None,
                    current: None,
                });
                ctx.send_viewport_cmd(ViewportCommand::Visible(true));
                ctx.send_viewport_cmd(ViewportCommand::Minimized(false));
                ctx.send_viewport_cmd(ViewportCommand::Decorations(false));
                ctx.send_viewport_cmd(ViewportCommand::WindowLevel(WindowLevel::AlwaysOnTop));
                ctx.send_viewport_cmd(ViewportCommand::Focus);
                ctx.send_viewport_cmd(ViewportCommand::OuterPosition(Pos2::new(
                    screen.x as f32 / window_scale,
                    screen.y as f32 / window_scale,
                )));
                ctx.send_viewport_cmd(ViewportCommand::InnerSize(Vec2::new(
                    screen.width as f32 / window_scale,
                    screen.height as f32 / window_scale,
                )));
                self.status = "拖动鼠标框选识别区域，Esc 取消".to_owned();
            }
            Err(error) => {
                ctx.send_viewport_cmd(ViewportCommand::Minimized(false));
                self.status = error;
                if self.active {
                    let _ = self.monitor.commands.send(MonitorCommand::SetActive(true));
                }
            }
        }
    }

    fn finish_selection(&mut self, ctx: &egui::Context, region: Option<CaptureRegion>) {
        let selection_screen = self.selector.as_ref().map(|selector| selector.screen);
        self.selector = None;
        ctx.send_viewport_cmd(ViewportCommand::Decorations(true));
        ctx.send_viewport_cmd(ViewportCommand::WindowLevel(WindowLevel::Normal));
        ctx.send_viewport_cmd(ViewportCommand::Resizable(false));
        ctx.send_viewport_cmd(ViewportCommand::InnerSize(MAIN_WINDOW_SIZE));
        ctx.send_viewport_cmd(ViewportCommand::OuterPosition(Pos2::new(100.0, 100.0)));

        if let Some(region) = region.filter(|value| value.is_valid()) {
            self.config.region = Some(region);
            self.config.selection_screen = selection_screen;
            self.status = format!(
                "已设定区域：({}, {}) {}×{}",
                region.x, region.y, region.width, region.height
            );
            self.apply_config();
            if !self.config.printer_name.is_empty() {
                self.set_active(true);
            }
        } else {
            self.status = "已取消框选".to_owned();
            if self.active {
                let _ = self.monitor.commands.send(MonitorCommand::SetActive(true));
            }
        }
    }

    fn apply_config(&mut self) {
        if let Err(error) = self.config.save() {
            self.status = format!("保存配置失败：{error}");
        }
        let _ = self
            .monitor
            .commands
            .send(MonitorCommand::Configure(self.config.clone()));
    }

    fn set_active(&mut self, active: bool) {
        if active && self.config.region.is_none() {
            self.status = "请先设定捕捉区域".to_owned();
            return;
        }
        if active && self.config.printer_name.trim().is_empty() {
            self.status = "请先选择打印机".to_owned();
            return;
        }
        self.active = active;
        let _ = self
            .monitor
            .commands
            .send(MonitorCommand::SetActive(active));
    }

    fn drain_events(&mut self) {
        while let Ok(event) = self.monitor.events.try_recv() {
            match event {
                MonitorEvent::Status(value) => self.status = value,
                MonitorEvent::OcrText(value) => self.last_ocr = value,
                MonitorEvent::Printed(value) => {
                    self.config.last_printed = Some(value.clone());
                    let _ = self.config.save();
                    self.status = format!("已打印：{value}");
                }
                MonitorEvent::Reprinted(value) => {
                    self.status = format!("已再次打印：{value}");
                }
                MonitorEvent::DisplayChanged(message) => {
                    self.active = false;
                    let _ = self.monitor.commands.send(MonitorCommand::SetActive(false));
                    self.status = message;
                }
                MonitorEvent::Error(error) => self.status = format!("错误：{error}"),
            }
        }
    }

    fn selector_ui(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        let escape = ctx.input(|input| input.key_pressed(egui::Key::Escape));
        if escape {
            self.finish_selection(&ctx, None);
            return;
        }

        let mut completed = None;
        let selector = self.selector.as_mut().expect("selector state");
        let texture = selector.texture.get_or_insert_with(|| {
            let size = [
                selector.screenshot.width as usize,
                selector.screenshot.height as usize,
            ];
            let image = egui::ColorImage::from_rgba_unmultiplied(size, &selector.screenshot.rgba);
            ctx.load_texture("virtual-screen", image, egui::TextureOptions::LINEAR)
        });

        let rect = ui.max_rect();
        ui.painter().image(
            texture.id(),
            rect,
            Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
            Color32::WHITE,
        );
        ui.painter()
            .rect_filled(rect, 0.0, Color32::from_black_alpha(80));
        let response = ui.allocate_rect(rect, Sense::drag());

        if response.drag_started() {
            selector.start = response.interact_pointer_pos();
            selector.current = selector.start;
        }
        if response.dragged() {
            selector.current = response.interact_pointer_pos();
        }

        if let (Some(start), Some(current)) = (selector.start, selector.current) {
            let selected = Rect::from_two_pos(start, current);
            ui.painter().rect_stroke(
                selected,
                0.0,
                Stroke::new(2.0, Color32::from_rgb(0, 200, 255)),
                egui::StrokeKind::Inside,
            );

            if response.drag_stopped() {
                let local = selected.translate(-rect.min.to_vec2());
                let scale_x = selector.screen.width as f32 / rect.width().max(1.0);
                let scale_y = selector.screen.height as f32 / rect.height().max(1.0);
                completed = Some(CaptureRegion {
                    x: selector.screen.x + (local.min.x * scale_x).round() as i32,
                    y: selector.screen.y + (local.min.y * scale_y).round() as i32,
                    width: (local.width() * scale_x).round().max(0.0) as u32,
                    height: (local.height() * scale_y).round().max(0.0) as u32,
                });
            }
        }

        ui.painter().text(
            rect.left_top() + Vec2::new(20.0, 20.0),
            egui::Align2::LEFT_TOP,
            "拖动框选足环号区域 · Esc 取消",
            egui::FontId::proportional(20.0),
            Color32::WHITE,
        );

        if completed.is_some() {
            self.finish_selection(&ctx, completed);
        }
    }

    fn settings_ui(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        egui::Frame::central_panel(ui.style()).show(ui, |ui| {
            ui.heading("信鸽足环自动识别打印");
            ui.add_space(8.0);

            ui.horizontal(|ui| {
                ui.label("状态：");
                ui.colored_label(
                    if self.active {
                        Color32::from_rgb(30, 150, 80)
                    } else {
                        Color32::GRAY
                    },
                    &self.status,
                );
            });

            ui.separator();
            ui.label("捕捉区域");
            ui.horizontal(|ui| {
                let region_text = self
                    .config
                    .region
                    .map(|r| format!("({}, {}) {}×{}", r.x, r.y, r.width, r.height))
                    .unwrap_or_else(|| "尚未设置".to_owned());
                ui.monospace(region_text);
                if ui.button("重新框选").clicked() {
                    self.begin_selection(&ctx);
                }
            });

            ui.add_space(8.0);
            ui.horizontal(|ui| {
                ui.label("佳博打印机");
                let before = self.config.printer_name.clone();
                egui::ComboBox::from_id_salt("printer")
                    .selected_text(if self.config.printer_name.is_empty() {
                        "请选择"
                    } else {
                        &self.config.printer_name
                    })
                    .show_ui(ui, |ui| {
                        for printer in &self.printers {
                            ui.selectable_value(
                                &mut self.config.printer_name,
                                printer.clone(),
                                printer,
                            );
                        }
                    });
                if before != self.config.printer_name {
                    self.apply_config();
                }
                if ui.button("刷新").clicked() {
                    match self.platform.printers() {
                        Ok(printers) => self.printers = printers,
                        Err(error) => self.status = error,
                    }
                }
            });

            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui
                    .add_enabled(!self.active, egui::Button::new("开始监控"))
                    .clicked()
                {
                    self.apply_config();
                    self.set_active(true);
                }
                if ui
                    .add_enabled(self.active, egui::Button::new("暂停监控"))
                    .clicked()
                {
                    self.set_active(false);
                }
                if ui.button("测试打印并切纸").clicked() {
                    if self.config.printer_name.is_empty() {
                        self.status = "请先选择打印机".to_owned();
                    } else {
                        let _ = self.monitor.commands.send(MonitorCommand::TestPrint {
                            printer_name: self.config.printer_name.clone(),
                        });
                    }
                }
            });

            ui.horizontal(|ui| {
                let last_printed = self.config.last_printed.clone();
                let can_reprint =
                    last_printed.is_some() && !self.config.printer_name.trim().is_empty();
                if ui
                    .add_enabled(can_reprint, egui::Button::new("再次打印"))
                    .clicked()
                    && let Some(value) = last_printed
                {
                    let _ = self.monitor.commands.send(MonitorCommand::Reprint {
                        printer_name: self.config.printer_name.clone(),
                        value,
                    });
                }
                if ui.button("打开日志目录").clicked() {
                    match print_log::open_directory() {
                        Ok(()) => self.status = "已打开日志目录".to_owned(),
                        Err(error) => self.status = error,
                    }
                }
            });

            ui.separator();
            ui.label("全局热键：Ctrl + Shift + R（重新框选并立即继续监控）");
            ui.label("识别规则：正式地区码 01–33 或 JN；流水号固定为 7 位数字。");
            ui.monospace("2026-10-0178948  |  03-0482758  |  0237849");
            ui.add_space(8.0);
            ui.label("最近 OCR 原文");
            ui.add(
                egui::TextEdit::multiline(&mut self.last_ocr)
                    .desired_rows(4)
                    .interactive(false),
            );
            if let Some(last) = &self.config.last_printed {
                ui.label(format!("最近成功打印：{last}"));
            }
        });
    }
}

fn install_system_cjk_font(ctx: &egui::Context) {
    #[cfg(windows)]
    {
        use std::sync::Arc;

        for path in [
            r"C:\Windows\Fonts\msyh.ttc",
            r"C:\Windows\Fonts\msyh.ttf",
            r"C:\Windows\Fonts\simhei.ttf",
        ] {
            let Ok(bytes) = std::fs::read(path) else {
                continue;
            };
            let mut fonts = egui::FontDefinitions::default();
            let name = "system-cjk".to_owned();
            fonts
                .font_data
                .insert(name.clone(), Arc::new(egui::FontData::from_owned(bytes)));
            for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
                fonts
                    .families
                    .entry(family)
                    .or_default()
                    .insert(0, name.clone());
            }
            ctx.set_fonts(fonts);
            break;
        }
    }

    #[cfg(not(windows))]
    let _ = ctx;
}

impl eframe::App for PigeonApp {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.drain_events();
        if self
            .hotkey
            .as_ref()
            .is_some_and(|receiver| receiver.try_recv().is_ok())
        {
            self.hotkey_requested = true;
        }
        let selection_result = self
            .selection_capture
            .as_ref()
            .and_then(|receiver| receiver.try_recv().ok());
        if let Some(result) = selection_result {
            self.selection_capture = None;
            self.show_selector(ctx, result);
        }
        ctx.request_repaint_after(std::time::Duration::from_millis(100));
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        if self.hotkey_requested && self.selector.is_none() {
            self.hotkey_requested = false;
            self.begin_selection(ui.ctx());
        }
        if self.selector.is_some() {
            self.selector_ui(ui);
        } else {
            self.settings_ui(ui);
        }
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.monitor.shutdown();
        let _ = print_log::upload_on_exit();
    }

    fn clear_color(&self, visuals: &egui::Visuals) -> [f32; 4] {
        visuals.panel_fill.to_normalized_gamma_f32()
    }
}
