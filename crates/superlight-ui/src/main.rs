#![cfg_attr(windows, windows_subsystem = "windows")]

use eframe::egui::{self, ColorImage, Sense, TextureHandle, TextureOptions, Vec2};
use serde_json::{Value, json};
use std::{sync::mpsc::{Receiver, SyncSender, sync_channel}, thread, time::{Duration, Instant}};
use superlight_core::{actions::ACTIONS, config};
use superlight_ipc::{DeviceStatus, Paths, Request, Response, Snapshot, PROTOCOL_VERSION};
use superlight_ui::Editor;

const LABELS: [&str; 12] = ["Middle", "Gesture button", "Back", "Forward", "Scroll left", "Scroll right", "Mode shift", "DPI button", "Gesture left", "Gesture right", "Gesture up", "Gesture down"];
const MOUSE: &[u8] = include_bytes!("../../../images/mouse.png");
const ANYWHERE: &[u8] = include_bytes!("../../../images/mouse_mx_anywhere_3s.png");
const VERTICAL: &[u8] = include_bytes!("../../../images/mx_vertical.png");

struct Job { request: Request, save: bool }
struct Reply { result: Result<Response, String>, save: bool }
struct Client { requests: SyncSender<Job>, replies: Receiver<Reply> }

impl Client {
    fn new(context: egui::Context) -> std::io::Result<Self> {
        let (requests, work) = sync_channel::<Job>(1);
        let (results, replies) = sync_channel(1);
        thread::Builder::new().name("superlight-ui-control".into()).spawn(move || {
            while let Ok(job) = work.recv() {
                let result = Paths::discover().and_then(|paths| superlight_ipc::call(&paths, &job.request)).map_err(|error| error.to_string());
                if results.send(Reply { result, save: job.save }).is_err() { break; }
                context.request_repaint();
            }
        })?;
        Ok(Self { requests, replies })
    }
}

struct Settings {
    editor: Editor,
    client: Option<Client>,
    busy: bool,
    connected: bool,
    demo: bool,
    message: Option<String>,
    last_poll: Instant,
    page: usize,
    layout: usize,
    textures: Vec<TextureHandle>,
    custom: String,
    profile_name: String,
    apps: String,
    dpi_presets: String,
    delete_profile: bool,
    reload_confirmation: bool,
    close_confirmation: bool,
    allow_close: bool,
    smoke_frames: Option<u8>,
}

impl Settings {
    fn new(context: &eframe::CreationContext<'_>, demo: bool, smoke: bool) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let mut textures = Vec::with_capacity(3);
        for (name, bytes) in [("master", MOUSE), ("anywhere", ANYWHERE), ("vertical", VERTICAL)] {
            let image = image::load_from_memory_with_format(bytes, image::ImageFormat::Png)?.into_rgba8();
            let size = [image.width() as usize, image.height() as usize];
            textures.push(context.egui_ctx.load_texture(name, ColorImage::from_rgba_unmultiplied(size, image.as_raw()), TextureOptions::LINEAR));
        }
        let client = if demo { None } else { Some(Client::new(context.egui_ctx.clone())?) };
        let mut editor = Editor::default();
        if demo {
            editor.receive(Snapshot {
                instance: "preview".into(), config: config::defaults(), active_profile: "default".into(),
                device: Some(DeviceStatus { name: "MX Master 3S".into(), model_key: "mx_master_3s".into(), layout_key: "mx_master_3s".into(), dpi_min: 200, dpi_max: 8000, dpi: Some(1000), battery: Some(80), supports_dpi: true, supports_smart_shift: true, supports_gesture: true, supports_mode_shift: true, ..DeviceStatus::default() }),
                ..Snapshot::default()
            });
        }
        Ok(Self {
            editor, client, busy: false, connected: false, demo, message: None, last_poll: Instant::now() - Duration::from_secs(10),
            page: 0, layout: 0, textures, custom: String::new(), profile_name: String::new(), apps: String::new(),
            dpi_presets: "800, 1200, 1600, 2400".into(), delete_profile: false, reload_confirmation: false,
            close_confirmation: false, allow_close: false, smoke_frames: smoke.then_some(0),
        })
    }

    fn send(&mut self, request: Request, save: bool) {
        if self.busy || self.demo { return; }
        let Some(client) = &self.client else { return; };
        match client.requests.try_send(Job { request, save }) {
            Ok(()) => { self.busy = true; self.last_poll = Instant::now(); }
            Err(error) => self.message = Some(error.to_string()),
        }
    }

    fn sync_fields(&mut self) {
        self.apps = self.editor.applications();
        if let Some(values) = self.editor.draft["settings"]["dpi_presets"].as_array() {
            self.dpi_presets = values.iter().filter_map(Value::as_u64).map(|value| value.to_string()).collect::<Vec<_>>().join(", ");
        }
        self.sync_custom();
    }

    fn sync_custom(&mut self) {
        let key = config::BUTTONS[self.editor.selected_button];
        self.custom = self.editor.draft["profiles"][&self.editor.selected_profile]["mappings"][key].as_str().and_then(|value| value.strip_prefix("custom:")).unwrap_or_default().to_owned();
    }

    fn receive(&mut self) {
        let reply = self.client.as_ref().and_then(|client| client.replies.try_recv().ok());
        let Some(reply) = reply else { return; };
        self.busy = false;
        match reply.result {
            Ok(response) if response.protocol != PROTOCOL_VERSION => {
                self.connected = false;
                self.message = Some("The service and settings window use different protocol versions".into());
            }
            Ok(response) if response.ok => {
                if let Some(state) = response.snapshot {
                    let replace = reply.save || !self.editor.dirty();
                    if replace {
                        if let Some(device) = &state.device {
                            self.layout = if device.layout_key.contains("anywhere") { 1 } else if device.layout_key.contains("vertical") { 2 } else { 0 };
                        }
                    }
                    if reply.save { self.editor.saved(state); } else { self.editor.receive(state); }
                    self.connected = true;
                    if replace { self.sync_fields(); }
                    if reply.save { self.message = Some("Settings saved. Hardware changes may still be pending.".into()); }
                } else { self.message = Some("The service returned no configuration".into()); }
            }
            Ok(response) => self.message = response.error,
            Err(error) => {
                self.connected = false;
                self.message = Some(format!("Service unavailable: {error}"));
            }
        }
    }

    fn top(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.heading("SuperLight");
            ui.label(env!("CARGO_PKG_VERSION"));
            if self.demo { ui.label("Preview only. No hardware connection."); }
            else if self.busy { ui.spinner(); }
            else if self.connected { ui.label("Connected"); }
            else { ui.label("Service not connected"); }
        });
        if let Some(state) = &self.editor.latest {
            let status = state.device.as_ref().map(|device| {
                let battery = device.battery.map(|level| format!(" | Battery {level}%")).unwrap_or_default();
                format!("{} | {}{}", device.name, device.transport, battery)
            }).unwrap_or_else(|| "No Logitech mouse connected".into());
            ui.label(status);
            if !self.demo && !state.permissions.description.is_empty() { ui.label(&state.permissions.description); }
        }
        ui.horizontal(|ui| {
            for (index, name) in ["Mouse", "Point and scroll", "Profiles", "Diagnostics"].into_iter().enumerate() {
                ui.selectable_value(&mut self.page, index, name);
            }
        });
        ui.separator();
    }

    fn profile_selector(&mut self, ui: &mut egui::Ui) {
        let profiles = self.editor.profiles();
        let old = self.editor.selected_profile.clone();
        let label = profiles.iter().find(|(key, _)| key == &old).map_or(old.as_str(), |(_, label)| label.as_str());
        egui::ComboBox::from_id_salt("profile").selected_text(label).show_ui(ui, |ui| {
            for (key, label) in profiles { ui.selectable_value(&mut self.editor.selected_profile, key, label); }
        });
        if old != self.editor.selected_profile { self.sync_fields(); self.delete_profile = false; }
    }

    fn mouse_view(&mut self, ui: &mut egui::Ui) {
        self.profile_selector(ui);
        ui.horizontal(|ui| {
            ui.label("Mouse view");
            egui::ComboBox::from_id_salt("layout").selected_text(["MX Master", "MX Anywhere", "MX Vertical"][self.layout]).show_ui(ui, |ui| {
                for (index, label) in ["MX Master", "MX Anywhere", "MX Vertical"].into_iter().enumerate() { ui.selectable_value(&mut self.layout, index, label); }
            });
        });
        let texture = &self.textures[self.layout];
        let original = texture.size_vec2();
        let height = 300.0_f32.min(ui.available_height().max(180.0) * 0.55);
        let width = (height * original.x / original.y).min(ui.available_width());
        let size = Vec2::new(width, width * original.y / original.x);
        let response = ui.add(egui::Image::new((texture.id(), size)).fit_to_exact_size(size));
        if self.layout == 0 {
            for (index, x, y) in [(0, 0.33, 0.45), (1, 0.70, 0.63), (2, 0.65, 0.40), (3, 0.60, 0.48), (4, 0.60, 0.375), (6, 0.43, 0.25)] {
                let point = response.rect.min + Vec2::new(x * response.rect.width(), y * response.rect.height());
                let rect = egui::Rect::from_center_size(point, Vec2::splat(26.0));
                let response = ui.interact(rect, ui.id().with(("mouse", index)), Sense::click()).on_hover_text(LABELS[index]);
                ui.painter().circle_stroke(point, if self.editor.selected_button == index { 12.0 } else { 9.0 }, ui.visuals().widgets.active.fg_stroke);
                if response.clicked() { self.editor.selected_button = index; self.sync_custom(); }
            }
        }
        ui.horizontal_wrapped(|ui| {
            for (index, label) in LABELS.iter().enumerate() {
                if ui.selectable_label(self.editor.selected_button == index, *label).clicked() { self.editor.selected_button = index; self.sync_custom(); }
            }
        });
        ui.separator();
        let index = self.editor.selected_button;
        let key = config::BUTTONS[index];
        let mut action = self.editor.draft["profiles"][&self.editor.selected_profile]["mappings"][key].as_str().unwrap_or("none").to_owned();
        let before = action.clone();
        let label = ACTIONS.iter().find(|(id, _)| *id == action).map_or(action.as_str(), |(_, label)| *label).to_owned();
        ui.label(format!("{} action", LABELS[index]));
        egui::ComboBox::from_id_salt("action").selected_text(label).show_ui(ui, |ui| {
            for &(id, label) in ACTIONS { ui.selectable_value(&mut action, id.into(), label); }
        });
        if action != before {
            if let Err(error) = self.editor.set_action(index, &action) { self.message = Some(error); }
            self.sync_custom();
        }
        ui.horizontal(|ui| {
            ui.label("Custom shortcut");
            ui.text_edit_singleline(&mut self.custom);
            if ui.button("Set shortcut").clicked() {
                if let Err(error) = self.editor.set_action(index, &format!("custom:{}", self.custom.trim())) { self.message = Some(error); }
            }
        });
        ui.label("Examples: ctrl+shift+t, cmd+shift+3, alt+left. Use super for the Windows key.");
    }

    fn settings(&mut self, ui: &mut egui::Ui) {
        let bounds = self.editor.latest.as_ref().and_then(|state| state.device.as_ref()).map_or((200, 8000), |device| (device.dpi_min.max(200), device.dpi_max.max(200)));
        let settings = &mut self.editor.draft["settings"];
        integer(ui, settings, "dpi", "Pointer DPI", i64::from(bounds.0)..=i64::from(bounds.1.max(bounds.0)));
        ui.horizontal(|ui| {
            ui.label("DPI presets");
            ui.text_edit_singleline(&mut self.dpi_presets);
            if ui.button("Set presets").clicked() {
                let values: Result<Vec<u16>, _> = self.dpi_presets.split(',').map(str::trim).filter(|value| !value.is_empty()).map(str::parse).collect();
                match values {
                    Ok(values) if !values.is_empty() && values.len() <= 64 && values.iter().all(|value| (bounds.0..=bounds.1).contains(value)) => settings["dpi_presets"] = json!(values),
                    _ => self.message = Some(format!("Enter 1 to 64 comma-separated DPI values between {} and {}", bounds.0, bounds.1)),
                }
            }
        });
        ui.separator();
        checkbox(ui, settings, "smart_shift_enabled", "Enable SmartShift");
        integer(ui, settings, "smart_shift_threshold", "SmartShift threshold", 1..=50);
        let mut mode = settings["smart_shift_mode"].as_str().unwrap_or("ratchet").to_owned();
        ui.horizontal(|ui| {
            ui.label("Fixed scroll mode / SmartShift fallback");
            ui.selectable_value(&mut mode, "ratchet".into(), "Ratchet");
            ui.selectable_value(&mut mode, "freespin".into(), "Free spin");
        });
        settings["smart_shift_mode"] = json!(mode);
        checkbox(ui, settings, "invert_vscroll", "Invert vertical scrolling");
        checkbox(ui, settings, "invert_hscroll", "Invert horizontal scrolling");
        checkbox(ui, settings, "ignore_trackpad", "Ignore trackpad and continuous scrolling on macOS");
        integer(ui, settings, "hscroll_threshold", "Horizontal action threshold", 1..=100);
        ui.separator();
        integer(ui, settings, "gesture_threshold", "Gesture distance", 5..=500);
        integer(ui, settings, "gesture_deadzone", "Gesture deadzone", 0..=500);
        integer(ui, settings, "gesture_timeout_ms", "Gesture timeout (ms)", 250..=10000);
        integer(ui, settings, "gesture_cooldown_ms", "Gesture cooldown (ms)", 0..=5000);
        ui.separator();
        checkbox(ui, settings, "start_at_login", "Start at login");
        checkbox(ui, settings, "start_minimized", "Start with settings closed");
        ui.label("Closing this settings window does not stop the separate mouse service.");
    }

    fn profiles(&mut self, ui: &mut egui::Ui) {
        self.profile_selector(ui);
        let key = self.editor.selected_profile.clone();
        let mut label = self.editor.draft["profiles"][&key]["label"].as_str().unwrap_or(&key).to_owned();
        ui.horizontal(|ui| { ui.label("Profile name"); if ui.text_edit_singleline(&mut label).changed() { self.editor.draft["profiles"][&key]["label"] = json!(label); } });
        if key == "default" { ui.label("This profile applies when no application-specific profile matches."); }
        else {
            ui.label("Application names, executable names or bundle identifiers. One per line.");
            if ui.add(egui::TextEdit::multiline(&mut self.apps).desired_rows(5).desired_width(f32::INFINITY)).changed() {
                if let Err(error) = self.editor.set_applications(&self.apps) { self.message = Some(error); }
            }
            if ui.button("Add current foreground application").clicked() {
                if let Some(state) = &self.editor.latest {
                    let name = if !state.foreground.id.is_empty() { state.foreground.id.clone() } else { state.foreground.name.clone() };
                    if !name.is_empty() {
                        if !self.apps.is_empty() { self.apps.push('\n'); }
                        self.apps.push_str(&name);
                        if let Err(error) = self.editor.set_applications(&self.apps) { self.message = Some(error); }
                        self.apps = self.editor.applications();
                    }
                }
            }
            if ui.button("Remove profile...").clicked() { self.delete_profile = true; }
            if self.delete_profile {
                ui.horizontal(|ui| {
                    ui.label("Remove this profile from the draft?");
                    if ui.button("Remove").clicked() {
                        if let Err(error) = self.editor.remove_profile(&key) { self.message = Some(error); }
                        self.delete_profile = false;
                        self.sync_fields();
                    }
                    if ui.button("Cancel").clicked() { self.delete_profile = false; }
                });
            }
        }
        ui.separator();
        ui.horizontal(|ui| {
            ui.text_edit_singleline(&mut self.profile_name);
            if ui.button("Add profile").clicked() {
                match self.editor.add_profile(&self.profile_name) {
                    Ok(_) => { self.profile_name.clear(); self.sync_fields(); }
                    Err(error) => self.message = Some(error),
                }
            }
        });
    }

    fn diagnostics(&mut self, ui: &mut egui::Ui) {
        if let Some(state) = &self.editor.latest {
            ui.label(format!("Service instance: {}", state.instance));
            ui.label(format!("Configuration revision: {}", state.revision));
            ui.label(format!("Active profile: {}", state.active_profile));
            ui.label(format!("Foreground application: {}", state.foreground.name));
            ui.label(format!("Native input ready: {}", state.native_ready));
            ui.label(format!("Suspended: {} | Paused: {}", state.suspended, state.paused));
            ui.label(format!("Hardware changes pending: {}", state.hardware_pending));
            ui.label(format!("Dropped queue events: {}", state.dropped_events));
            if let Some(device) = &state.device { ui.label(format!("Backend: {} | Receiver slot: {} | Raw XY: {}", device.backend, device.receiver_slot, device.raw_xy)); }
            if let Some(notice) = &state.notice { ui.label(notice); }
            for error in &state.errors { ui.label(error); }
        }
        ui.horizontal(|ui| {
            if ui.button("Retry connection").clicked() { self.send(Request::Get, false); }
            if ui.add_enabled(self.connected, egui::Button::new("Reconnect mouse")).clicked() { self.send(Request::Reconnect, false); }
            if ui.add_enabled(self.connected, egui::Button::new("Read hardware settings")).clicked() { self.send(Request::RefreshHardware, false); }
            if ui.add_enabled(self.connected, egui::Button::new("Input permissions")).clicked() { self.send(Request::RequestPermissions, false); }
        });
    }

    fn bottom(&mut self, ui: &mut egui::Ui) {
        ui.separator();
        if self.editor.conflicted() { ui.label("Settings changed in another session. Your draft is preserved. Reload before saving."); }
        if let Some(message) = &self.message { ui.label(message); }
        ui.horizontal(|ui| {
            let save = self.connected && self.editor.dirty() && !self.editor.conflicted() && !self.busy;
            if ui.add_enabled(save, egui::Button::new("Save")).clicked() {
                match self.editor.apply_request() { Ok(request) => self.send(request, true), Err(error) => self.message = Some(error) }
            }
            if ui.add_enabled(!self.busy && self.editor.latest.is_some(), egui::Button::new("Reload...")).clicked() {
                if self.editor.dirty() { self.reload_confirmation = true; }
                else { self.send(Request::Get, false); }
            }
            let paused = self.editor.latest.as_ref().is_some_and(|state| state.paused);
            if ui.add_enabled(self.connected && !self.busy, egui::Button::new(if paused { "Resume remapping" } else { "Pause remapping" })).clicked() { self.send(Request::SetPaused { value: !paused }, false); }
            if self.editor.dirty() { ui.label("Unsaved changes"); }
        });
        if self.reload_confirmation {
            ui.horizontal(|ui| {
                ui.label("Discard the local draft and reload the last service state?");
                if ui.button("Discard draft").clicked() {
                    if let Err(error) = self.editor.reload() { self.message = Some(error); }
                    self.reload_confirmation = false;
                    self.sync_fields();
                    self.send(Request::Get, false);
                }
                if ui.button("Keep editing").clicked() { self.reload_confirmation = false; }
            });
        }
        if self.close_confirmation {
            ui.horizontal(|ui| {
                ui.label("Close without saving? A submitted save cannot be cancelled.");
                if ui.button("Close settings").clicked() { self.allow_close = true; ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close); }
                if ui.button("Keep open").clicked() { self.close_confirmation = false; }
            });
        }
    }
}

fn checkbox(ui: &mut egui::Ui, settings: &mut Value, key: &str, label: &str) {
    let mut value = settings[key].as_bool().unwrap_or(false);
    if ui.checkbox(&mut value, label).changed() { settings[key] = json!(value); }
}

fn integer(ui: &mut egui::Ui, settings: &mut Value, key: &str, label: &str, range: std::ops::RangeInclusive<i64>) {
    let mut value = settings[key].as_i64().unwrap_or(*range.start());
    if ui.add(egui::Slider::new(&mut value, range).text(label)).changed() { settings[key] = json!(value); }
}

impl eframe::App for Settings {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.receive();
        let minimized = ui.ctx().input(|input| input.viewport().minimized.unwrap_or(false));
        if !minimized && !self.demo && !self.busy && self.last_poll.elapsed() >= Duration::from_secs(2) { self.send(Request::Get, false); }
        if !minimized { ui.ctx().request_repaint_after(Duration::from_secs(2)); }
        if ui.ctx().input(|input| input.viewport().close_requested()) && (self.editor.dirty() || self.busy) && !self.allow_close {
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.close_confirmation = true;
        }
        egui::CentralPanel::default().show(ui, |ui| {
            self.top(ui);
            egui::ScrollArea::vertical().max_height((ui.available_height() - 125.0).max(150.0)).show(ui, |ui| {
                ui.add_enabled_ui(!self.busy, |ui| match self.page {
                    0 => self.mouse_view(ui), 1 => self.settings(ui), 2 => self.profiles(ui), _ => self.diagnostics(ui),
                });
            });
            self.bottom(ui);
        });
        if let Some(frames) = &mut self.smoke_frames {
            *frames += 1;
            if *frames >= 4 { self.allow_close = true; ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close); }
            else { ui.ctx().request_repaint(); }
        }
    }
}

fn main() -> eframe::Result {
    let mut demo = false;
    let mut smoke = false;
    for argument in std::env::args().skip(1) {
        match argument.as_str() {
            "--demo" => demo = true,
            "--smoke-test" => { demo = true; smoke = true; }
            "--service-running" => {}
            "--help" | "-h" => { println!("SuperLight settings\n--demo         Preview without connecting to the service\n--smoke-test   Render four preview frames and exit"); return Ok(()); }
            _ => { eprintln!("Unknown argument: {argument}"); std::process::exit(2); }
        }
    }
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([980.0, 760.0]).with_min_inner_size([720.0, 600.0]),
        renderer: eframe::Renderer::Glow,
        ..Default::default()
    };
    eframe::run_native("SuperLight settings", options, Box::new(move |context| Ok(Box::new(Settings::new(context, demo, smoke)?))))
}
