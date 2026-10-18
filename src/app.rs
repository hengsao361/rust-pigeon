use crate::{
    config::AppConfig,
    monitor::{MonitorCommand, MonitorEvent, MonitorHandle},
    platform::{
        NativePlatform, Platform, RegionOverlay, RegionSelection, RegionSelectionReceiver,
        spawn_region_hotkey, spawn_region_overlay, spawn_region_selector,
    },
    print_log,
    receipt::sanitize_channel_input,
};
use eframe::egui::{self, Align, Color32, Vec2, ViewportCommand};
use std::sync::{Arc, mpsc::Receiver};

pub const MAIN_WINDOW_SIZE: Vec2 = Vec2::new(620.0, 540.0);
pub const APP_VERSION: &str = match option_env!("RUST_PIGEON_BUILD_VERSION") {
    Some(version) => version,
    None => env!("CARGO_PKG_VERSION"),
};

pub struct PigeonApp {
    config: AppConfig,
    platform: Arc<dyn Platform>,
    monitor: MonitorHandle,
    printers: Vec<String>,
    active: bool,
    status: String,
    last_ocr: String,
    hotkey: Option<Receiver<()>>,
    hotkey_requested: bool,
    selection_capture: Option<RegionSelectionReceiver>,
    region_overlay: Option<RegionOverlay>,
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
        let region_overlay = spawn_region_overlay().ok();
        if active {
            let _ = monitor.commands.send(MonitorCommand::SetActive(true));
        }

        let app = Self {
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
            hotkey,
            hotkey_requested: false,
            selection_capture: None,
            region_overlay,
        };
        app.sync_region_overlay();
        app
    }

    fn begin_selection(&mut self, ctx: &egui::Context) {
        if self.selection_capture.is_some() {
            return;
        }

        let _ = self.monitor.commands.send(MonitorCommand::SetActive(false));
        if let Some(overlay) = &self.region_overlay {
            overlay.set_region(None);
        }
        ctx.send_viewport_cmd(ViewportCommand::Minimized(true));
        match spawn_region_selector() {
            Ok(receiver) => {
                self.selection_capture = Some(receiver);
                self.status = "拖动鼠标框选识别区域，Esc 取消".to_owned();
                ctx.request_repaint_after(std::time::Duration::from_millis(50));
            }
            Err(error) => {
                ctx.send_viewport_cmd(ViewportCommand::Minimized(false));
                self.status = error;
                if self.active {
                    let _ = self.monitor.commands.send(MonitorCommand::SetActive(true));
                    self.sync_region_overlay();
                }
            }
        }
    }

    fn finish_selection(&mut self, ctx: &egui::Context, selection: Option<RegionSelection>) {
        ctx.send_viewport_cmd(ViewportCommand::Visible(true));
        ctx.send_viewport_cmd(ViewportCommand::Minimized(false));
        ctx.send_viewport_cmd(ViewportCommand::Focus);
        ctx.send_viewport_cmd(ViewportCommand::Resizable(false));
        ctx.send_viewport_cmd(ViewportCommand::InnerSize(MAIN_WINDOW_SIZE));

        if let Some(selection) = selection.filter(|value| value.region.is_valid()) {
            self.config.region = Some(selection.region);
            self.config.selection_screen = Some(selection.screen);
            self.status = format!(
                "已设定区域：({}, {}) {}×{}",
                selection.region.x,
                selection.region.y,
                selection.region.width,
                selection.region.height
            );
            self.apply_config();
            if !self.config.printer_name.is_empty() {
                self.set_active(true);
            }
        } else {
            self.status = "已取消框选".to_owned();
            if self.active {
                let _ = self.monitor.commands.send(MonitorCommand::SetActive(true));
                self.sync_region_overlay();
            }
        }
    }

    fn handle_selection_result(
        &mut self,
        ctx: &egui::Context,
        result: Result<Option<RegionSelection>, String>,
    ) {
        match result {
            Ok(selection) => self.finish_selection(ctx, selection),
            Err(error) => {
                ctx.send_viewport_cmd(ViewportCommand::Visible(true));
                ctx.send_viewport_cmd(ViewportCommand::Minimized(false));
                ctx.send_viewport_cmd(ViewportCommand::Focus);
                self.status = error;
                if self.active {
                    let _ = self.monitor.commands.send(MonitorCommand::SetActive(true));
                    self.sync_region_overlay();
                }
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
        self.sync_region_overlay();
    }

    fn sync_region_overlay(&self) {
        let region = self.active.then_some(self.config.region).flatten();
        if let Some(overlay) = &self.region_overlay {
            overlay.set_region(region);
        }
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
                    self.sync_region_overlay();
                    self.status = message;
                }
                MonitorEvent::Error(error) => self.status = format!("错误：{error}"),
            }
        }
    }

    fn settings_ui(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        egui::Frame::central_panel(ui.style()).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.heading("信鸽足环自动识别打印");
                ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                    ui.weak(format!("v{APP_VERSION}"));
                });
            });
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
                ui.label("鸽钟通道");
                let channel_response = ui
                    .add(egui::TextEdit::singleline(&mut self.config.channel).desired_width(50.0));
                if channel_response.changed() {
                    self.config.channel = sanitize_channel_input(&self.config.channel);
                    self.apply_config();
                }
            });

            ui.horizontal(|ui| {
                ui.label("业务内容");
                let response = ui.add(
                    egui::TextEdit::singleline(&mut self.config.business_content)
                        .desired_width(260.0)
                        .hint_text("请输入票据顶部内容"),
                );
                if response.changed() {
                    self.config.business_content =
                        self.config.business_content.chars().take(16).collect();
                    self.apply_config();
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
                let repeated_ocr = ui
                    .checkbox(
                        &mut self.config.repeat_ocr_during_stability,
                        "稳定确认时重复 OCR",
                    )
                    .on_hover_text("开启后，新号码打印前会实际执行 3 次 OCR；默认关闭");
                if repeated_ocr.changed() {
                    self.apply_config();
                }
            });

            ui.horizontal(|ui| {
                if ui.button("打开日志目录").clicked() {
                    match print_log::open_directory() {
                        Ok(()) => self.status = "已打开日志目录".to_owned(),
                        Err(error) => self.status = error,
                    }
                }
                if ui.button("测试打印").clicked() {
                    if self.config.printer_name.is_empty() {
                        self.status = "请先选择打印机".to_owned();
                    } else {
                        let _ = self.monitor.commands.send(MonitorCommand::TestPrint {
                            printer_name: self.config.printer_name.clone(),
                        });
                    }
                }
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
            });

            ui.separator();
            ui.label("全局热键：Ctrl + Shift + R（重新框选并立即继续监控）");
            ui.label("识别规则：正式地区码 01–33 或 JN；足环号固定为 7 位数字。");
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
            self.handle_selection_result(ctx, result);
        }
        ctx.request_repaint_after(std::time::Duration::from_millis(100));
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        if self.hotkey_requested && self.selection_capture.is_none() {
            self.hotkey_requested = false;
            self.begin_selection(ui.ctx());
        }
        self.settings_ui(ui);
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.monitor.shutdown();
        let _ = print_log::upload_on_exit();
    }

    fn clear_color(&self, visuals: &egui::Visuals) -> [f32; 4] {
        visuals.panel_fill.to_normalized_gamma_f32()
    }
}
