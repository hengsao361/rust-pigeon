use crate::{
    config::AppConfig,
    ocr::{OcrEngine, TesseractCli},
    platform::Platform,
    print_log::{self, PrintAction},
    receipt::PrintTicket,
    ring::extract_ring_numbers,
};
use std::{
    sync::{
        Arc,
        mpsc::{self, Receiver, Sender, TryRecvError},
    },
    thread,
    time::Duration,
};

const FAST_CONFIRMATION_INTERVAL_MS: u64 = 100;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GateDecision {
    Waiting { seen: u8, required: u8 },
    Invalid,
    Duplicate,
    Ready(String),
}

#[derive(Debug, Clone)]
pub enum MonitorCommand {
    Configure(AppConfig),
    SetActive(bool),
    TestPrint { printer_name: String },
    Reprint { printer_name: String, value: String },
    Shutdown,
}

#[derive(Debug, Clone)]
pub enum MonitorEvent {
    Status(String),
    OcrText(String),
    Printed(String),
    Reprinted(String),
    DisplayChanged(String),
    Error(String),
}

pub struct MonitorHandle {
    pub commands: Sender<MonitorCommand>,
    pub events: Receiver<MonitorEvent>,
    worker: Option<thread::JoinHandle<()>>,
}

impl MonitorHandle {
    pub fn spawn(platform: Arc<dyn Platform>, initial: AppConfig) -> Self {
        let (command_tx, command_rx) = mpsc::channel();
        let (event_tx, event_rx) = mpsc::channel();

        let worker = thread::Builder::new()
            .name("pigeon-monitor".to_owned())
            .spawn(move || run_monitor(platform, initial, command_rx, event_tx))
            .expect("failed to start monitor thread");

        Self {
            commands: command_tx,
            events: event_rx,
            worker: Some(worker),
        }
    }

    pub fn shutdown(&mut self) {
        let _ = self.commands.send(MonitorCommand::Shutdown);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn run_monitor(
    platform: Arc<dyn Platform>,
    mut config: AppConfig,
    commands: Receiver<MonitorCommand>,
    events: Sender<MonitorEvent>,
) {
    let mut active = false;
    let mut gate = StabilityGate::new(config.stable_observations, config.last_printed.clone());
    let mut ocr: Box<dyn OcrEngine> = Box::new(TesseractCli::new(&config.tesseract_path));
    let mut last_fingerprint = None;
    let mut last_candidate: Option<String> = None;

    loop {
        let delay = monitor_delay(&config, &gate);
        match commands.recv_timeout(if active {
            delay
        } else {
            Duration::from_secs(60)
        }) {
            Ok(MonitorCommand::Configure(next)) => {
                let ocr_changed = next.tesseract_path != config.tesseract_path;
                config = next;
                gate = StabilityGate::new(config.stable_observations, config.last_printed.clone());
                if ocr_changed {
                    ocr = Box::new(TesseractCli::new(&config.tesseract_path));
                }
                last_fingerprint = None;
                last_candidate = None;
                let _ = events.send(MonitorEvent::Status("配置已更新".to_owned()));
            }
            Ok(MonitorCommand::SetActive(value)) => {
                active = value;
                last_fingerprint = None;
                last_candidate = None;
                let text = if active { "监控中" } else { "已暂停" };
                let _ = events.send(MonitorEvent::Status(text.to_owned()));
            }
            Ok(MonitorCommand::TestPrint { printer_name }) => {
                let value = "2025-05-2025050";
                let ticket = print_ticket(&config, value);
                match platform.print_ring(&printer_name, &ticket) {
                    Ok(()) => {
                        let log_error =
                            log_print_success(value, &config.channel, PrintAction::Test);
                        let _ = events.send(MonitorEvent::Status("测试打印已发送".to_owned()));
                        send_log_error(&events, log_error);
                    }
                    Err(error) => {
                        let _ = events.send(MonitorEvent::Error(error));
                    }
                }
            }
            Ok(MonitorCommand::Reprint {
                printer_name,
                value,
            }) => match platform.print_ring(&printer_name, &print_ticket(&config, &value)) {
                Ok(()) => {
                    let log_error =
                        log_print_success(&value, &config.channel, PrintAction::Reprint);
                    let _ = events.send(MonitorEvent::Reprinted(value));
                    send_log_error(&events, log_error);
                }
                Err(error) => {
                    let _ = events.send(MonitorEvent::Error(error));
                }
            },
            Ok(MonitorCommand::Shutdown) => break,
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
            Err(mpsc::RecvTimeoutError::Timeout) => {
                if active {
                    monitor_once(
                        platform.as_ref(),
                        &config,
                        ocr.as_mut(),
                        &mut gate,
                        &mut last_fingerprint,
                        &mut last_candidate,
                        &events,
                    );
                }
            }
        }

        // Coalesce bursts from UI controls before the next capture.
        loop {
            match commands.try_recv() {
                Ok(MonitorCommand::Configure(next)) => {
                    let ocr_changed = next.tesseract_path != config.tesseract_path;
                    config = next;
                    gate =
                        StabilityGate::new(config.stable_observations, config.last_printed.clone());
                    if ocr_changed {
                        ocr = Box::new(TesseractCli::new(&config.tesseract_path));
                    }
                    last_fingerprint = None;
                    last_candidate = None;
                }
                Ok(MonitorCommand::SetActive(value)) => active = value,
                Ok(MonitorCommand::TestPrint { printer_name }) => {
                    let value = "2026-10-0178948";
                    let ticket = print_ticket(&config, value);
                    match platform.print_ring(&printer_name, &ticket) {
                        Ok(()) => {
                            let log_error =
                                log_print_success(value, &config.channel, PrintAction::Test);
                            let _ = events.send(MonitorEvent::Status("测试打印已发送".to_owned()));
                            send_log_error(&events, log_error);
                        }
                        Err(error) => {
                            let _ = events.send(MonitorEvent::Error(error));
                        }
                    }
                }
                Ok(MonitorCommand::Reprint {
                    printer_name,
                    value,
                }) => match platform.print_ring(&printer_name, &print_ticket(&config, &value)) {
                    Ok(()) => {
                        let log_error =
                            log_print_success(&value, &config.channel, PrintAction::Reprint);
                        let _ = events.send(MonitorEvent::Reprinted(value));
                        send_log_error(&events, log_error);
                    }
                    Err(error) => {
                        let _ = events.send(MonitorEvent::Error(error));
                    }
                },
                Ok(MonitorCommand::Shutdown) | Err(TryRecvError::Disconnected) => return,
                Err(TryRecvError::Empty) => break,
            }
        }
    }
}

fn monitor_delay(config: &AppConfig, gate: &StabilityGate) -> Duration {
    let normal_ms = config.poll_interval_ms.clamp(100, 10_000);
    let delay_ms = if gate.is_confirming() {
        normal_ms.min(FAST_CONFIRMATION_INTERVAL_MS)
    } else {
        normal_ms
    };
    Duration::from_millis(delay_ms)
}

fn should_repeat_ocr(config: &AppConfig, gate: &StabilityGate) -> bool {
    config.repeat_ocr_during_stability && gate.is_confirming()
}

#[allow(clippy::too_many_arguments)]
fn monitor_once(
    platform: &dyn Platform,
    config: &AppConfig,
    ocr: &mut dyn OcrEngine,
    gate: &mut StabilityGate,
    last_fingerprint: &mut Option<u64>,
    last_candidate: &mut Option<String>,
    events: &Sender<MonitorEvent>,
) {
    let Some(region) = config.region else {
        let _ = events.send(MonitorEvent::Error("请先设定捕捉区域".to_owned()));
        return;
    };
    if config.printer_name.trim().is_empty() {
        let _ = events.send(MonitorEvent::Error("请先选择打印机".to_owned()));
        return;
    }

    let current_screen = match platform.virtual_screen() {
        Ok(screen) => screen,
        Err(error) => {
            let _ = events.send(MonitorEvent::Error(error));
            return;
        }
    };
    let layout_changed = config
        .selection_screen
        .is_some_and(|selected_screen| selected_screen != current_screen);
    if layout_changed || !current_screen.contains(region) {
        gate.observe(None);
        let _ = events.send(MonitorEvent::DisplayChanged(
            "显示器布局、分辨率或缩放已变化，请重新框选区域".to_owned(),
        ));
        return;
    }

    let image = match platform.capture(region) {
        Ok(image) => image,
        Err(error) => {
            let _ = events.send(MonitorEvent::Error(error));
            return;
        }
    };

    let fingerprint = image.fingerprint();
    let repeat_ocr = should_repeat_ocr(config, gate);
    if *last_fingerprint != Some(fingerprint) || repeat_ocr {
        *last_fingerprint = Some(fingerprint);
        let raw = match ocr.recognize(&image) {
            Ok(text) => text,
            Err(error) => {
                let _ = events.send(MonitorEvent::Error(error));
                if repeat_ocr {
                    *last_fingerprint = None;
                }
                *last_candidate = None;
                gate.observe(None);
                return;
            }
        };
        let _ = events.send(MonitorEvent::OcrText(raw.clone()));
        let matches = extract_ring_numbers(&raw, config.ocr_format);
        *last_candidate = if matches.len() == 1 {
            Some(matches[0].as_str().to_owned())
        } else {
            if matches.len() > 1 {
                let _ = events.send(MonitorEvent::Status(
                    "识别到多个合法足环号，本次不打印".to_owned(),
                ));
            }
            None
        };
        if repeat_ocr && last_candidate.is_none() {
            *last_fingerprint = None;
        }
    }

    match gate.observe(last_candidate.as_deref()) {
        GateDecision::Ready(value) => {
            match platform.print_ring(&config.printer_name, &print_ticket(config, &value)) {
                Ok(()) => {
                    gate.mark_printed(value.clone());
                    let log_error =
                        log_print_success(&value, &config.channel, PrintAction::Automatic);
                    let _ = events.send(MonitorEvent::Printed(value));
                    send_log_error(events, log_error);
                }
                Err(error) => {
                    gate.mark_print_failed();
                    let _ = events.send(MonitorEvent::Error(error));
                }
            }
        }
        GateDecision::Waiting { seen, required } => {
            let _ = events.send(MonitorEvent::Status(format!(
                "结果稳定中 {seen}/{required}"
            )));
        }
        GateDecision::Duplicate => {
            let _ = events.send(MonitorEvent::Status("内容未变化，不重复打印".to_owned()));
        }
        GateDecision::Invalid => {
            let _ = events.send(MonitorEvent::Status("未识别到唯一合法足环号".to_owned()));
        }
    }
}

fn print_ticket(config: &AppConfig, value: &str) -> PrintTicket {
    PrintTicket::new(value, &config.business_content, &config.channel)
}

fn log_print_success(value: &str, channel: &str, action: PrintAction) -> Option<String> {
    print_log::append_success(value, channel, action)
        .err()
        .map(|error| format!("打印成功，但写入本地日志失败：{error}"))
}

fn send_log_error(events: &Sender<MonitorEvent>, error: Option<String>) {
    if let Some(error) = error {
        let _ = events.send(MonitorEvent::Error(error));
    }
}

#[derive(Debug, Clone)]
pub struct StabilityGate {
    required: u8,
    current: Option<String>,
    seen: u8,
    last_printed: Option<String>,
}

impl StabilityGate {
    pub fn new(required: u8, last_printed: Option<String>) -> Self {
        Self {
            required: required.max(1),
            current: None,
            seen: 0,
            last_printed,
        }
    }

    pub fn observe(&mut self, candidate: Option<&str>) -> GateDecision {
        let Some(candidate) = candidate else {
            self.reset_candidate();
            return GateDecision::Invalid;
        };

        if self.last_printed.as_deref() == Some(candidate) {
            self.reset_candidate();
            return GateDecision::Duplicate;
        }

        if self.current.as_deref() == Some(candidate) {
            self.seen = self.seen.saturating_add(1);
        } else {
            self.current = Some(candidate.to_owned());
            self.seen = 1;
        }

        if self.seen == self.required {
            GateDecision::Ready(candidate.to_owned())
        } else {
            GateDecision::Waiting {
                seen: self.seen.min(self.required),
                required: self.required,
            }
        }
    }

    pub fn mark_printed(&mut self, value: String) {
        self.last_printed = Some(value);
        self.reset_candidate();
    }

    pub fn mark_print_failed(&mut self) {
        self.reset_candidate();
    }

    fn is_confirming(&self) -> bool {
        self.current.is_some() && self.seen > 0 && self.seen < self.required
    }

    fn reset_candidate(&mut self) {
        self.current = None;
        self.seen = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requires_three_equal_observations() {
        let mut gate = StabilityGate::new(3, None);
        assert_eq!(
            gate.observe(Some("0237849")),
            GateDecision::Waiting {
                seen: 1,
                required: 3
            }
        );
        assert_eq!(
            gate.observe(Some("0237849")),
            GateDecision::Waiting {
                seen: 2,
                required: 3
            }
        );
        assert_eq!(
            gate.observe(Some("0237849")),
            GateDecision::Ready("0237849".to_owned())
        );
    }

    #[test]
    fn invalid_or_changed_observation_restarts_stability() {
        let mut gate = StabilityGate::new(3, None);
        gate.observe(Some("0237849"));
        gate.observe(None);
        assert_eq!(
            gate.observe(Some("0237849")),
            GateDecision::Waiting {
                seen: 1,
                required: 3
            }
        );
        assert_eq!(
            gate.observe(Some("03-0482758")),
            GateDecision::Waiting {
                seen: 1,
                required: 3
            }
        );
    }

    #[test]
    fn successful_print_is_deduplicated() {
        let mut gate = StabilityGate::new(1, None);
        assert!(matches!(
            gate.observe(Some("0237849")),
            GateDecision::Ready(_)
        ));
        gate.mark_printed("0237849".to_owned());
        assert_eq!(gate.observe(Some("0237849")), GateDecision::Duplicate);
        assert!(matches!(
            gate.observe(Some("03-0482758")),
            GateDecision::Ready(_)
        ));
    }

    #[test]
    fn accelerates_only_while_confirming_a_new_candidate() {
        let config = AppConfig::default();
        let mut gate = StabilityGate::new(3, Some("0237849".to_owned()));

        assert_eq!(monitor_delay(&config, &gate), Duration::from_millis(500));

        assert!(matches!(
            gate.observe(Some("03-0482758")),
            GateDecision::Waiting { seen: 1, .. }
        ));
        assert_eq!(monitor_delay(&config, &gate), Duration::from_millis(100));

        assert!(matches!(
            gate.observe(Some("03-0482758")),
            GateDecision::Waiting { seen: 2, .. }
        ));
        assert_eq!(monitor_delay(&config, &gate), Duration::from_millis(100));

        assert!(matches!(
            gate.observe(Some("03-0482758")),
            GateDecision::Ready(_)
        ));
        gate.mark_printed("03-0482758".to_owned());
        assert_eq!(monitor_delay(&config, &gate), Duration::from_millis(500));

        assert_eq!(gate.observe(Some("03-0482758")), GateDecision::Duplicate);
        assert_eq!(monitor_delay(&config, &gate), Duration::from_millis(500));
    }

    #[test]
    fn repeated_ocr_switch_only_applies_during_stability_confirmation() {
        let mut config = AppConfig::default();
        let mut gate = StabilityGate::new(3, None);

        assert!(!config.repeat_ocr_during_stability);
        assert!(!gate.is_confirming());
        assert!(!should_repeat_ocr(&config, &gate));

        gate.observe(Some("03-0482758"));
        assert!(gate.is_confirming());
        assert!(!should_repeat_ocr(&config, &gate));

        config.repeat_ocr_during_stability = true;
        assert!(should_repeat_ocr(&config, &gate));

        gate.observe(None);
        assert!(!should_repeat_ocr(&config, &gate));
    }
}
