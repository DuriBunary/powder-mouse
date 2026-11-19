use eframe::egui::{self, Color32, RichText, Stroke};
use std::time::{Duration, Instant};

use crate::device::{
    BatteryState, ConnectionProbeState, DeviceMonitor, DeviceSnapshot, DpiAndColorApplyState,
    DpiAndColorSettings, MouseProfileSwitchState, ReceiverState,
};
use crate::settings::{
    AppSettings, MAX_MACRO_STEPS, MAX_MACROS, MacroRepeatMode, MacroStep, MacroStepKind,
    ProfileSettings, SettingsStore,
};
use crate::theme::{self, ACCENT};
#[cfg(target_os = "windows")]
use crate::windows_pointer::{self, WheelScroll, WindowsPointerSettings};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Page {
    Overview,
    Buttons,
    Performance,
    Lighting,
    Macros,
    Profiles,
    WindowsInput,
}

#[derive(PartialEq)]
enum SettingsSnapshot {
    ActiveProfile {
        index: usize,
        profile_count: usize,
        profile: ProfileSettings,
    },
    Macro {
        macro_count: usize,
        selected_id: Option<u64>,
        edit_revision: u64,
    },
}

impl SettingsSnapshot {
    fn capture(
        settings: &AppSettings,
        page: Page,
        selected_macro: Option<u64>,
        macro_edit_revision: u64,
    ) -> Self {
        if page == Page::Macros {
            Self::Macro {
                macro_count: settings.macros.len(),
                selected_id: selected_macro,
                edit_revision: macro_edit_revision,
            }
        } else {
            Self::ActiveProfile {
                index: settings.active_profile,
                profile_count: settings.profiles.len(),
                profile: settings.active().clone(),
            }
        }
    }
}

pub struct DaxaApp {
    monitor: Option<DeviceMonitor>,
    #[cfg(target_os = "windows")]
    tray: Option<crate::tray::Tray>,
    #[cfg(target_os = "windows")]
    exiting: bool,
    snapshot: DeviceSnapshot,
    page: Page,
    configured: bool,
    mouse_texture: Option<egui::TextureHandle>,
    window_status: Option<(bool, Option<u8>, String)>,
    settings: SettingsStore,
    save_at: Option<Instant>,
    macro_name: String,
    selected_macro: Option<u64>,
    macro_edit_revision: u64,
    deleting_macro: bool,
    macro_message: String,
    #[cfg(target_os = "windows")]
    recorder: crate::macro_recorder::MacroRecorder,
    #[cfg(target_os = "windows")]
    recording_macro: Option<u64>,
    #[cfg(target_os = "windows")]
    recording_limit: usize,
    #[cfg(target_os = "windows")]
    recording_count: usize,
    profile_message: String,
    profile_name: String,
    deleting_profile: bool,
    mouse_profile_target: u8,
    mouse_profile_message: String,
    apply_message: String,
    #[cfg(target_os = "windows")]
    windows_input: Option<WindowsPointerSettings>,
    #[cfg(target_os = "windows")]
    windows_input_loaded: bool,
    #[cfg(target_os = "windows")]
    windows_input_message: String,
}

impl Default for DaxaApp {
    fn default() -> Self {
        Self {
            monitor: None,
            #[cfg(target_os = "windows")]
            tray: None,
            #[cfg(target_os = "windows")]
            exiting: false,
            snapshot: DeviceSnapshot::default(),
            page: Page::Overview,
            configured: false,
            mouse_texture: None,
            window_status: None,
            settings: SettingsStore::load(),
            save_at: None,
            macro_name: String::new(),
            selected_macro: None,
            macro_edit_revision: 0,
            deleting_macro: false,
            macro_message: String::new(),
            #[cfg(target_os = "windows")]
            recorder: crate::macro_recorder::MacroRecorder::default(),
            #[cfg(target_os = "windows")]
            recording_macro: None,
            #[cfg(target_os = "windows")]
            recording_limit: 0,
            #[cfg(target_os = "windows")]
            recording_count: 0,
            profile_message: String::new(),
            profile_name: String::new(),
            deleting_profile: false,
            mouse_profile_target: 0,
            mouse_profile_message: String::new(),
            apply_message: String::new(),
            #[cfg(target_os = "windows")]
            windows_input: None,
            #[cfg(target_os = "windows")]
            windows_input_loaded: false,
            #[cfg(target_os = "windows")]
            windows_input_message: String::new(),
        }
    }
}

fn muted(text: impl Into<String>) -> RichText {
    RichText::new(text).weak().size(14.0)
}
fn heading(ui: &mut egui::Ui, title: &str, description: &str) {
    ui.add_space(2.0);
    ui.label(RichText::new(title).size(27.0).strong());
    ui.label(muted(description));
    ui.add_space(14.0);
}
fn section(ui: &mut egui::Ui, title: &str, description: &str) {
    ui.add_space(7.0);
    ui.label(RichText::new(title).size(17.0).strong());
    ui.label(muted(description));
    ui.add_space(7.0);
}
fn divider(ui: &mut egui::Ui) {
    ui.add_space(10.0);
    ui.separator();
    ui.add_space(10.0);
}

fn load_mouse_texture(ctx: &egui::Context) -> Option<egui::TextureHandle> {
    let data = include_bytes!("../assets/daxa-air-mini.png");
    let mut decoder = png::Decoder::new(std::io::Cursor::new(data.as_slice()));
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder.read_info().ok()?;
    let mut bytes = vec![0; reader.output_buffer_size()?];
    let info = reader.next_frame(&mut bytes).ok()?;
    if info.color_type != png::ColorType::Rgba || info.bit_depth != png::BitDepth::Eight {
        return None;
    }
    let image = egui::ColorImage::from_rgba_unmultiplied(
        [info.width as usize, info.height as usize],
        &bytes[..info.buffer_size()],
    );
    Some(ctx.load_texture("daxa-air-mini", image, egui::TextureOptions::LINEAR))
}

impl DaxaApp {
    fn mark_macro_edited(&mut self) {
        self.macro_edit_revision = self.macro_edit_revision.wrapping_add(1);
    }

    #[cfg(target_os = "windows")]
    fn stop_recording(&mut self, reason: &str, dropped: usize) {
        if !self.recorder.is_recording() {
            return;
        }
        let steps = self.recorder.stop();
        let dropped = dropped + self.recorder.take_dropped_events();
        let total = steps.len();
        let mut appended = 0;
        if let Some(item) = self
            .recording_macro
            .take()
            .and_then(|id| self.settings.settings.macro_by_id_mut(id))
        {
            let available = MAX_MACRO_STEPS.saturating_sub(item.steps.len());
            appended = total.min(available);
            item.steps.extend(steps.into_iter().take(available));
        }
        let omitted = total.saturating_sub(appended);
        self.macro_message = format!("{reason} {appended} action(s) added locally.");
        if dropped > 0 || omitted > 0 {
            self.macro_message.push_str(&format!(
                " {} action(s) were not saved; review the sequence.",
                dropped + omitted
            ));
        }
        if appended > 0 {
            self.mark_macro_edited();
            let _ = self.settings.save_if_dirty();
            self.save_at = None;
        }
        self.recording_count = 0;
    }

    #[cfg(target_os = "windows")]
    fn update_recording(&mut self, ctx: &egui::Context) {
        if !self.recorder.is_recording() {
            return;
        }
        self.recording_limit = self
            .recording_macro
            .and_then(|id| self.settings.settings.macro_by_id(id))
            .map_or(0, |item| MAX_MACRO_STEPS.saturating_sub(item.steps.len()));
        self.recording_count = self.recorder.step_count();
        let dropped = self.recorder.take_dropped_events();
        if dropped > 0 {
            self.stop_recording("Recording stopped because input was missed.", dropped);
        } else if self.recording_count >= self.recording_limit {
            self.stop_recording("Recording stopped at the step limit.", 0);
        } else {
            ctx.request_repaint_after(Duration::from_millis(100));
        }
    }
    pub fn new(ctx: egui::Context) -> Self {
        let mouse_texture = load_mouse_texture(&ctx);
        let mut app = Self {
            #[cfg(target_os = "windows")]
            tray: crate::tray::Tray::new(ctx.clone()).ok(),
            monitor: Some(DeviceMonitor::start(ctx)),
            mouse_texture,
            ..Self::default()
        };
        app.profile_name = app.settings.settings.active().name.clone();
        app
    }

    fn overview(&mut self, ui: &mut egui::Ui, receiver: &str, battery: &str) {
        ui.add_space(16.0);
        ui.label(muted("YOUR MOUSE").strong());
        ui.add_space(4.0);
        ui.label(RichText::new("Daxa Air Mini Wireless").size(30.0).strong());
        ui.label(muted("Your mouse, at a glance."));
        ui.add_space(18.0);
        ui.horizontal_wrapped(|ui| {
            ui.label(receiver);
            ui.label("·");
            ui.label(battery);
            if let Some(id) = self.snapshot.reported_profile_id {
                ui.label("·");
                ui.label(format!("Mouse profile ID {id}"));
            }
        });
        ui.add_space(20.0);
        if let Some(texture) = &self.mouse_texture {
            let width = ui.available_width().min(300.0);
            let size = egui::vec2(
                width,
                width * texture.size()[1] as f32 / texture.size()[0] as f32,
            );
            ui.add(egui::Image::new((texture.id(), size)).alt_text("Daxa Air Mini Wireless mouse"));
        }
        ui.add_space(20.0);
        ui.label(RichText::new("Local profile").strong());
        ui.label(&self.settings.settings.active().name);
        ui.label(muted("Settings are saved on this PC. Device Apply is paused while the remaining controls are verified."));
        if ui.button("Open profiles").clicked() {
            self.page = Page::Profiles;
        }
    }

    fn commit_profile_name(&mut self) {
        let name = self.profile_name.trim();
        if !name.is_empty() {
            self.settings.settings.active_mut().name = name.to_owned();
        }
        self.profile_name = self.settings.settings.active().name.clone();
    }

    fn commit_macro_name(&mut self) {
        if let Some(id) = self.selected_macro {
            let previous_name = self
                .settings
                .settings
                .macro_by_id(id)
                .map(|item| item.name.clone());
            let renamed = self.settings.settings.rename_macro(id, &self.macro_name);
            let current_name = self
                .settings
                .settings
                .macro_by_id(id)
                .map(|item| item.name.clone());
            if renamed && previous_name != current_name {
                self.mark_macro_edited();
            }
            if let Some(item) = self.settings.settings.macro_by_id(id) {
                self.macro_name = item.name.clone();
            }
        }
    }

    fn navigation(&mut self, root: &mut egui::Ui, receiver: &str, battery: &str) {
        egui::Panel::left("navigation")
            .exact_size(198.0)
            .frame(
                egui::Frame::new()
                    .fill(theme::sidebar(root))
                    .inner_margin(egui::Margin::symmetric(16, 20)),
            )
            .show(root, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    ui.label(RichText::new("Daxa Control").strong().size(17.0));
                    ui.label(muted("Air Mini Wireless"));
                    ui.add_space(16.0);
                    ui.label(muted(receiver));
                    ui.label(muted(battery));
                    ui.add_space(16.0);
                    for (page, title) in [
                        (Page::Overview, "Overview"),
                        (Page::Buttons, "Buttons"),
                        (Page::Performance, "DPI & Polling"),
                        (Page::Lighting, "Lighting"),
                        (Page::Macros, "Macros"),
                        (Page::Profiles, "Profiles"),
                        (Page::WindowsInput, "Windows input"),
                    ] {
                        let active = self.page == page;
                        let text = if active {
                            RichText::new(title).strong().color(theme::accent(ui))
                        } else {
                            RichText::new(title)
                        };
                        let button = egui::Button::new(text)
                            .fill(if active {
                                theme::soft(ui)
                            } else {
                                Color32::TRANSPARENT
                            })
                            .stroke(Stroke::NONE)
                            .corner_radius(8)
                            .min_size(egui::vec2(ui.available_width(), 34.0));
                        if ui.add(button).clicked() {
                            if self.page == Page::Profiles && page != Page::Profiles {
                                self.commit_profile_name();
                            }
                            if self.page == Page::Macros && page != Page::Macros {
                                #[cfg(target_os = "windows")]
                                self.stop_recording("Recording stopped when leaving Macros.", 0);
                                self.commit_macro_name();
                            }
                            self.page = page;
                        }
                    }
                    ui.add_space(12.0);
                    if ui
                        .checkbox(&mut self.settings.settings.dark_mode, "Dark mode")
                        .changed()
                    {
                        theme::install(ui.ctx(), self.settings.settings.dark_mode);
                        let _ = self.settings.save_if_dirty();
                    }
                    let save_status = if self.save_at.is_some() {
                        "Saving changes…"
                    } else {
                        self.settings.status()
                    };
                    ui.label(muted(save_status));
                    #[cfg(target_os = "windows")]
                    if self.tray.is_some() {
                        ui.label(muted("Close to keep running in tray"));
                    } else {
                        ui.label(muted("Tray unavailable · closing exits"));
                    }
                });
            });
    }

    fn buttons(&mut self, ui: &mut egui::Ui) {
        heading(ui, "Buttons", "Choose what each mouse button does.");
        section(
            ui,
            "Button assignments",
            "Choose assignments for the active profile.",
        );
        let names = [
            "Left click",
            "Right click",
            "Middle click",
            "Forward",
            "Back",
            "DPI cycle",
        ];
        let options = [
            "Left click",
            "Right click",
            "Middle click",
            "Forward",
            "Back",
            "DPI cycle",
            "Disabled",
            "Scroll up",
            "Scroll down",
            "Double click",
            "Right double click",
            "Triple click",
            "Fire key",
            "Single key",
            "Key combination",
            "Multimedia",
            "DPI up",
            "DPI down",
            "DPI lock",
            "Polling rate switch",
            "Profile switch",
            "RGB on/off",
            "RGB switch",
            "Macro",
            "Play/pause",
            "Previous track",
            "Next track",
            "Volume up",
            "Volume down",
            "Mute",
            "Open browser",
            "Open email",
            "Open calculator",
            "Stop play",
            "Stop web",
            "Search",
            "Home page",
            "Explorer",
            "Refresh",
            "Favorites",
            "Media Player",
        ];
        let macro_count = self.settings.settings.macros.len();
        ui.horizontal_top(|ui| {
            let rows_width = (ui.available_width() - 200.0).max(340.0);
            ui.allocate_ui_with_layout(
                egui::vec2(rows_width, 0.0),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    for (i, name) in names.iter().enumerate() {
                        ui.horizontal_wrapped(|ui| {
                            ui.label(
                                RichText::new(format!("{:02}", i + 1))
                                    .color(theme::accent(ui))
                                    .strong()
                                    .monospace(),
                            );
                            let name_label = ui
                                .allocate_ui_with_layout(
                                    egui::vec2(135.0, 28.0),
                                    egui::Layout::left_to_right(egui::Align::Center),
                                    |ui| {
                                        ui.set_min_width(135.0);
                                        ui.label(*name)
                                    },
                                )
                                .inner;
                            let previous_binding =
                                self.settings.settings.active().button_bindings[i];
                            egui::ComboBox::from_id_salt(("binding", i))
                                .selected_text(
                                    options[self.settings.settings.active_mut().button_bindings[i]],
                                )
                                .width(165.0)
                                .show_ui(ui, |ui| {
                                    for (j, option) in options.iter().enumerate() {
                                        ui.selectable_value(
                                            &mut self
                                                .settings
                                                .settings
                                                .active_mut()
                                                .button_bindings[i],
                                            j,
                                            *option,
                                        );
                                    }
                                })
                                .response
                                .labelled_by(name_label.id);
                            let binding = self.settings.settings.active().button_bindings[i];
                            if binding == 23 && previous_binding != 23 {
                                let profile = self.settings.settings.active_mut();
                                if profile.button_macro_ids[i].is_none() {
                                    profile.button_details[i].clear();
                                }
                            }
                            if binding == 23 {
                                let detail =
                                    self.settings.settings.active().button_details[i].clone();
                                let selected_id =
                                    self.settings.settings.active().button_macro_ids[i];
                                let selected_name = selected_id.and_then(|id| {
                                    self.settings
                                        .settings
                                        .macro_by_id(id)
                                        .map(|macro_def| macro_def.name.clone())
                                });

                                if macro_count == 0 {
                                    ui.label(muted("No macros available."));
                                    if selected_id.is_some() || !detail.is_empty() {
                                        ui.label(muted("The saved macro is unavailable."));
                                        if ui.button("Clear assignment").clicked() {
                                            let profile = self.settings.settings.active_mut();
                                            profile.button_macro_ids[i] = None;
                                            profile.button_details[i].clear();
                                        }
                                    }
                                    if ui.button("Open Macros").clicked() {
                                        self.page = Page::Macros;
                                    }
                                } else {
                                    let label = ui.label("Macro");
                                    let selected_text = match selected_name.as_deref() {
                                        Some(name) if name.chars().count() > 18 => {
                                            format!(
                                                "{}…",
                                                name.chars().take(17).collect::<String>()
                                            )
                                        }
                                        Some(name) => name.to_owned(),
                                        None if detail.is_empty() && selected_id.is_none() => {
                                            "Select macro".to_owned()
                                        }
                                        None => "Macro unavailable".to_owned(),
                                    };
                                    let mut choice = selected_id.filter(|id| {
                                        self.settings.settings.macro_by_id(*id).is_some()
                                    });
                                    let mut changed = false;
                                    let response =
                                        egui::ComboBox::from_id_salt(("macro_binding", i))
                                            .selected_text(selected_text)
                                            .width(150.0)
                                            .show_ui(ui, |ui| {
                                                changed |= ui
                                                    .selectable_value(
                                                        &mut choice,
                                                        None,
                                                        "No macro assigned",
                                                    )
                                                    .changed();
                                                for macro_def in &self.settings.settings.macros {
                                                    changed |= ui
                                                        .selectable_value(
                                                            &mut choice,
                                                            Some(macro_def.id),
                                                            &macro_def.name,
                                                        )
                                                        .changed();
                                                }
                                            })
                                            .response
                                            .labelled_by(label.id);
                                    if let Some(name) = selected_name {
                                        response.on_hover_text(name);
                                    } else if !detail.is_empty() {
                                        response.on_hover_text(format!(
                                            "Saved macro not found: {detail}"
                                        ));
                                    }
                                    if changed {
                                        let macro_name = choice
                                            .and_then(|id| self.settings.settings.macro_by_id(id))
                                            .map(|macro_def| macro_def.name.clone())
                                            .unwrap_or_default();
                                        let profile = self.settings.settings.active_mut();
                                        profile.button_macro_ids[i] = choice;
                                        profile.button_details[i] = macro_name;
                                    }
                                }
                            } else if [12, 13, 14, 15].contains(&binding) {
                                let label = match binding {
                                    12 => "Fire key details",
                                    13 => "Key",
                                    14 => "Key combination",
                                    15 => "Multimedia action",
                                    _ => "Multimedia action",
                                };
                                ui.label(label);
                                ui.add(
                                    egui::TextEdit::singleline(
                                        &mut self.settings.settings.active_mut().button_details[i],
                                    )
                                    .char_limit(64)
                                    .desired_width(150.0)
                                    .hint_text(label),
                                );
                            }
                        });
                    }
                },
            );
            button_map(ui);
        });
        if self
            .settings
            .settings
            .active()
            .button_bindings
            .contains(&23)
        {
            ui.label(muted(
                "Macro assignments are saved on this PC and are not yet applied to the mouse.",
            ));
        }
        divider(ui);
        section(ui, "Debounce", "Set the delay for button input.");
        ui.add(
            egui::Slider::new(&mut self.settings.settings.active_mut().debounce_ms, 0..=30)
                .suffix(" ms"),
        );
        ui.label(muted(
            "Lower values may cause double clicks. Increase the delay if that happens.",
        ));
    }

    fn performance(&mut self, ui: &mut egui::Ui) {
        heading(
            ui,
            "DPI & Polling",
            "Tune pointer sensitivity and response.",
        );
        section(
            ui,
            "DPI stages",
            "Adjust sensitivity and color for each stage.",
        );
        let profile = self.settings.settings.active_mut();
        if ui
            .checkbox(
                &mut profile.dpi_independent,
                "Adjust horizontal and vertical DPI separately",
            )
            .changed()
            && profile.dpi_independent
            && profile.dpi_y_stages.is_none()
        {
            profile.dpi_y_stages = Some(profile.dpi_stages);
        }
        let independent = profile.dpi_independent;
        for i in 0..profile.dpi_stages.len() {
            ui.horizontal_wrapped(|ui| {
                ui.label(
                    RichText::new(format!("0{}", i + 1))
                        .color(theme::accent(ui))
                        .strong()
                        .monospace(),
                );
                ui.add_sized(
                    [300.0, 24.0],
                    egui::Slider::new(&mut profile.dpi_stages[i], 100..=26000)
                        .step_by(100.0)
                        .suffix(if independent { " X DPI" } else { " DPI" }),
                );
                ui.label(format!("Stage {} color", i + 1));
                ui.color_edit_button_srgb(&mut profile.dpi_colors[i]);
            });
            if let Some(y_stages) = &mut profile.dpi_y_stages
                && independent
            {
                ui.add_sized(
                    [300.0, 24.0],
                    egui::Slider::new(&mut y_stages[i], 100..=26000)
                        .step_by(100.0)
                        .suffix(" Y DPI"),
                );
            }
        }
        ui.label(muted(
            "DPI effect controls will be available when device behavior is verified.",
        ));
        ui.add_space(8.0);
        let supported = self.snapshot.dpi_and_color_writes_supported;
        ui.label(muted(if supported {
            "Changes are saved on this PC. Apply sends only DPI stages and their colors to the current mouse profile."
        } else {
            "DPI stages and colors are saved on this PC. Applying them to the mouse is paused until changes are verified."
        }));
        let pending = matches!(
            self.snapshot.dpi_and_color_apply,
            DpiAndColorApplyState::Pending
        );
        let ready = self.snapshot.receiver == ReceiverState::Detected
            && self.snapshot.reported_profile_id.is_some()
            && self.monitor.is_some();
        if ui
            .add_enabled(
                supported && ready && !pending,
                egui::Button::new("Apply DPI and colors to mouse"),
            )
            .clicked()
            && supported
        {
            let profile = self.settings.settings.active();
            let settings = DpiAndColorSettings {
                x_stages: profile
                    .dpi_stages
                    .iter()
                    .map(|value| *value as u16)
                    .collect(),
                y_stages: profile.dpi_independent.then(|| {
                    profile
                        .dpi_y_stages
                        .unwrap_or(profile.dpi_stages)
                        .iter()
                        .map(|value| *value as u16)
                        .collect()
                }),
                colors: profile.dpi_colors,
            };
            if self
                .monitor
                .as_ref()
                .is_some_and(|monitor| monitor.request_dpi_and_colors(settings))
            {
                self.snapshot.dpi_and_color_apply = DpiAndColorApplyState::Pending;
                self.apply_message.clear();
            } else {
                self.apply_message = "Could not start the request. Please try again.".to_owned();
            }
        }
        if !ready && !pending {
            ui.label(muted(
                "Connect the mouse and wait for its profile to be detected before applying.",
            ));
        }
        if !self.apply_message.is_empty() {
            ui.label(muted(&self.apply_message));
        } else {
            match &self.snapshot.dpi_and_color_apply {
                DpiAndColorApplyState::Idle => {}
                DpiAndColorApplyState::Pending => {
                    ui.label(muted("Sending DPI and colors…"));
                }
                DpiAndColorApplyState::Unsupported { reason } => {
                    if supported {
                        ui.label(muted(reason));
                    }
                }
                DpiAndColorApplyState::Applied { .. } => {
                    ui.label(muted("The mouse acknowledged the last DPI and color request. Current edits may differ; verify the settings on the mouse."));
                }
                DpiAndColorApplyState::Failed {
                    acknowledged_chunks,
                    reason,
                } => {
                    let detail = if *acknowledged_chunks > 0 {
                        "Some changes may have reached the mouse."
                    } else {
                        "The mouse may not have received the changes."
                    };
                    ui.label(muted(format!(
                        "Could not complete the last request. {detail} {reason}"
                    )));
                }
            }
        }
        divider(ui);
        section(
            ui,
            "Polling rate",
            "How often the mouse reports its position.",
        );
        ui.horizontal_wrapped(|ui| {
            for rate in [125, 250, 500, 1000] {
                ui.selectable_value(
                    &mut self.settings.settings.active_mut().polling_hz,
                    rate,
                    format!("{rate} Hz"),
                );
            }
        });
        divider(ui);
        section(ui, "Sensor options", "Fine adjustments for tracking.");
        ui.horizontal(|ui| {
            ui.label("Lift-off distance");
            ui.radio_value(
                &mut self.settings.settings.active_mut().lift_off_distance_mm,
                1,
                "1 mm",
            );
            ui.radio_value(
                &mut self.settings.settings.active_mut().lift_off_distance_mm,
                2,
                "2 mm",
            );
        });
        ui.checkbox(
            &mut self.settings.settings.active_mut().ripple_control,
            "Ripple control",
        );
        ui.checkbox(
            &mut self.settings.settings.active_mut().angle_snapping,
            "Angle snapping",
        );
    }

    fn lighting(&mut self, ui: &mut egui::Ui) {
        heading(
            ui,
            "Lighting",
            "Set the look and behavior of your mouse lighting.",
        );
        section(
            ui,
            "Main lighting",
            "Choose the effect and color for the main zone.",
        );
        let effects = [
            "Steady",
            "Breathing",
            "Streaming",
            "Neon",
            "Single color flow",
            "Colorful breathing",
            "Off",
        ];
        egui::ComboBox::from_id_salt("effect")
            .selected_text(effects[self.settings.settings.active_mut().lighting_effect])
            .width(200.0)
            .show_ui(ui, |ui| {
                for (i, effect) in effects.iter().enumerate() {
                    ui.selectable_value(
                        &mut self.settings.settings.active_mut().lighting_effect,
                        i,
                        *effect,
                    );
                }
            });
        ui.add_space(12.0);
        ui.label("Brightness");
        ui.add_sized(
            [300.0, 24.0],
            egui::Slider::new(
                &mut self.settings.settings.active_mut().brightness_percent,
                0.0..=100.0,
            )
            .suffix("%"),
        );
        ui.label("Main speed");
        ui.add_sized(
            [300.0, 24.0],
            egui::Slider::new(
                &mut self.settings.settings.active_mut().speed_percent,
                0.0..=100.0,
            )
            .suffix("%"),
        );
        ui.horizontal(|ui| {
            ui.label("Main color");
            ui.color_edit_button_srgb(&mut self.settings.settings.active_mut().main_color);
        });
        divider(ui);
        section(
            ui,
            "Logo lighting",
            "Choose the effect and color for the logo zone.",
        );
        let profile = self.settings.settings.active_mut();
        egui::ComboBox::from_id_salt("logo_effect")
            .selected_text(effects[profile.logo_effect])
            .width(200.0)
            .show_ui(ui, |ui| {
                for (i, effect) in effects.iter().enumerate() {
                    ui.selectable_value(&mut profile.logo_effect, i, *effect);
                }
            });
        ui.label("Logo brightness");
        ui.add_sized(
            [300.0, 24.0],
            egui::Slider::new(&mut profile.logo_brightness_percent, 0.0..=100.0).suffix("%"),
        );
        ui.label("Logo speed");
        ui.add_sized(
            [300.0, 24.0],
            egui::Slider::new(&mut profile.logo_speed_percent, 0.0..=100.0).suffix("%"),
        );
        ui.horizontal(|ui| {
            ui.label("Logo color");
            ui.color_edit_button_srgb(&mut profile.logo_color);
        });
        divider(ui);
        section(
            ui,
            "Power behavior",
            "Manage lighting while the mouse is in use or idle.",
        );
        ui.checkbox(
            &mut self
                .settings
                .settings
                .active_mut()
                .turn_off_lighting_while_moving,
            "Turn off lighting while moving",
        );
        ui.horizontal(|ui| {
            let label = ui.label("Dormancy after");
            ui.add(
                egui::DragValue::new(&mut self.settings.settings.active_mut().dormancy_seconds)
                    .range(0..=3600)
                    .speed(10.0)
                    .suffix(" sec"),
            )
            .labelled_by(label.id);
        });
    }

    fn macros(&mut self, ui: &mut egui::Ui) {
        heading(ui, "Macros", "Create a sequence of actions.");
        ui.label(muted(
            "Macros are saved locally. They are not yet executed on the mouse.",
        ));
        section(ui, "Macro library", "Select a macro to edit its steps.");
        let macro_count = self.settings.settings.macros.len();
        if macro_count == 0 {
            ui.label(muted("No macros yet. Create one to get started."));
        }
        if !self.macro_message.is_empty() {
            ui.label(muted(&self.macro_message));
        }
        let mut requested_macro = None;
        for item in &self.settings.settings.macros {
            if ui
                .selectable_label(self.selected_macro == Some(item.id), &item.name)
                .clicked()
                && self.selected_macro != Some(item.id)
            {
                requested_macro = Some(item.id);
            }
        }
        if let Some(id) = requested_macro {
            #[cfg(target_os = "windows")]
            self.stop_recording("Recording stopped when changing macros.", 0);
            self.commit_macro_name();
            self.selected_macro = Some(id);
            self.macro_name = self
                .settings
                .settings
                .macro_by_id(id)
                .map_or_else(String::new, |item| item.name.clone());
            self.deleting_macro = false;
        }
        if ui
            .add_enabled(macro_count < MAX_MACROS, egui::Button::new("New macro"))
            .clicked()
        {
            #[cfg(target_os = "windows")]
            self.stop_recording("Recording stopped when changing macros.", 0);
            self.commit_macro_name();
            if let Some(id) = self.settings.settings.add_macro("New macro") {
                self.selected_macro = Some(id);
                self.macro_name = "New macro".to_owned();
                self.deleting_macro = false;
                self.macro_message.clear();
            }
        }
        if macro_count >= MAX_MACROS {
            ui.label(muted("Maximum of 64 macros reached."));
        }
        divider(ui);
        section(
            ui,
            "Import and export",
            "Share one macro or the entire library as Daxa Control JSON.",
        );
        #[cfg(target_os = "windows")]
        ui.horizontal_wrapped(|ui| {
            for (single, title) in [(true, "Import macro…"), (false, "Import library…")] {
                if ui
                    .add_enabled(macro_count < MAX_MACROS, egui::Button::new(title))
                    .clicked()
                {
                    self.stop_recording("Recording stopped before importing.", 0);
                    self.commit_macro_name();
                    if let Some(path) = rfd::FileDialog::new()
                        .add_filter("Daxa macros", &["json"])
                        .pick_file()
                    {
                        match self.settings.import_macros(path, single) {
                            Ok(ids) => {
                                self.mark_macro_edited();
                                self.selected_macro = ids.first().copied();
                                self.macro_name = self
                                    .selected_macro
                                    .and_then(|id| self.settings.settings.macro_by_id(id))
                                    .map_or_else(String::new, |item| item.name.clone());
                                self.deleting_macro = false;
                                self.macro_message =
                                    format!("Imported {} macro(s) and saved locally.", ids.len());
                                self.save_at = None;
                            }
                            Err(error) => self.macro_message = error,
                        }
                    }
                }
            }
            for (single, title, filename) in [
                (true, "Export selected…", "daxa-macro.json"),
                (false, "Export library…", "daxa-macros.json"),
            ] {
                let enabled = if single {
                    self.selected_macro.is_some()
                } else {
                    macro_count > 0
                };
                if ui.add_enabled(enabled, egui::Button::new(title)).clicked() {
                    self.commit_macro_name();
                    if let Some(path) = rfd::FileDialog::new()
                        .add_filter("Daxa macros", &["json"])
                        .set_file_name(filename)
                        .save_file()
                    {
                        self.macro_message = match self
                            .settings
                            .export_macros(path, if single { self.selected_macro } else { None })
                        {
                            Ok(()) => "Macros exported.".to_owned(),
                            Err(error) => error,
                        };
                    }
                }
            }
        });
        #[cfg(not(target_os = "windows"))]
        ui.label(muted("Macro import and export require Windows."));
        ui.label(muted(
            "Each imported macro is added to this library; existing macros are kept.",
        ));
        let Some(id) = self.selected_macro else {
            return;
        };
        if self.settings.settings.macro_by_id(id).is_none() {
            self.selected_macro = None;
            return;
        }
        divider(ui);
        section(
            ui,
            "Edit macro",
            "Name the macro and choose how many times it repeats.",
        );
        ui.label("Macro name");
        let response = ui.add(
            egui::TextEdit::singleline(&mut self.macro_name)
                .char_limit(64)
                .desired_width(240.0),
        );
        if response.lost_focus()
            || (response.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)))
        {
            self.commit_macro_name();
        }
        let mut macro_edited = false;
        if let Some(item) = self.settings.settings.macro_by_id_mut(id) {
            ui.label("Repeat mode");
            let mut repeat_mode_changed = false;
            egui::ComboBox::from_id_salt("macro_repeat_mode")
                .selected_text(match item.repeat_mode {
                    MacroRepeatMode::Count => "Repeat a set number of times",
                    MacroRepeatMode::UntilRelease => "Repeat until button is released",
                    MacroRepeatMode::UntilAnyKey => "Repeat until any key is pressed",
                })
                .show_ui(ui, |ui| {
                    repeat_mode_changed |= ui
                        .selectable_value(
                            &mut item.repeat_mode,
                            MacroRepeatMode::Count,
                            "Repeat a set number of times",
                        )
                        .changed();
                    repeat_mode_changed |= ui
                        .selectable_value(
                            &mut item.repeat_mode,
                            MacroRepeatMode::UntilRelease,
                            "Repeat until button is released",
                        )
                        .changed();
                    repeat_mode_changed |= ui
                        .selectable_value(
                            &mut item.repeat_mode,
                            MacroRepeatMode::UntilAnyKey,
                            "Repeat until any key is pressed",
                        )
                        .changed();
                });
            macro_edited |= repeat_mode_changed;
            if item.repeat_mode == MacroRepeatMode::Count {
                ui.horizontal(|ui| {
                    ui.label("Repeat count");
                    macro_edited |= ui
                        .add(egui::DragValue::new(&mut item.repeat_count).range(1..=999))
                        .changed();
                });
            }
        }
        divider(ui);
        section(
            ui,
            "Record actions",
            "Capture keyboard and mouse actions in other windows. Daxa Control is skipped. Actions are stored locally and are not sent to or executed on the mouse.",
        );
        #[cfg(target_os = "windows")]
        {
            if self.recorder.is_recording() {
                ui.horizontal_wrapped(|ui| {
                    ui.label(
                        RichText::new("● RECORDING")
                            .strong()
                            .color(Color32::from_rgb(155, 51, 43)),
                    );
                    ui.label(format!(
                        "{} / {} actions",
                        self.recording_count, self.recording_limit
                    ));
                    if ui.button("Stop recording").clicked() {
                        self.stop_recording("Recording stopped.", 0);
                    }
                });
            } else {
                let available = MAX_MACRO_STEPS.saturating_sub(
                    self.settings
                        .settings
                        .macro_by_id(id)
                        .map_or(0, |item| item.steps.len()),
                );
                if ui
                    .add_enabled(available > 0, egui::Button::new("Record actions"))
                    .clicked()
                {
                    match self.recorder.start() {
                        Ok(()) => {
                            self.recording_macro = Some(id);
                            self.recording_limit = available;
                            self.recording_count = 0;
                            self.macro_message.clear();
                            ui.ctx().request_repaint_after(Duration::from_millis(100));
                        }
                        Err(error) => self.macro_message = error,
                    }
                }
                if available == 0 {
                    ui.label(muted("Remove a step before recording more actions."));
                }
            }
        }
        #[cfg(not(target_os = "windows"))]
        ui.label(muted("Recording is available on Windows."));
        if ui.button("Delete macro…").clicked() {
            #[cfg(target_os = "windows")]
            self.stop_recording("Recording stopped before deleting.", 0);
            self.deleting_macro = true;
        }
        if self.deleting_macro {
            ui.label(format!(
                "Delete ‘{}’?",
                self.settings
                    .settings
                    .macro_by_id(id)
                    .map(|m| m.name.as_str())
                    .unwrap_or("macro")
            ));
            ui.horizontal_wrapped(|ui| {
                if ui.button("Delete").clicked() {
                    self.settings.settings.remove_macro(id);
                    self.selected_macro = None;
                    self.macro_name.clear();
                    self.deleting_macro = false;
                    self.macro_message = "Macro deleted.".to_owned();
                }
                if ui.button("Cancel").clicked() {
                    self.deleting_macro = false;
                }
            });
        }
        if self.selected_macro.is_none() {
            ui.label(muted(&self.macro_message));
            return;
        }
        divider(ui);
        section(
            ui,
            "Steps",
            "Each step may include a delay after the action. For a delay-only step, set the duration in milliseconds.",
        );
        let kinds = [
            (MacroStepKind::KeyDown, "Key down"),
            (MacroStepKind::KeyUp, "Key up"),
            (MacroStepKind::MouseButtonDown, "Mouse button down"),
            (MacroStepKind::MouseButtonUp, "Mouse button up"),
            (MacroStepKind::Wheel, "Wheel"),
            (MacroStepKind::Delay, "Delay"),
        ];
        let count = self
            .settings
            .settings
            .macro_by_id(id)
            .map_or(0, |m| m.steps.len());
        if count == 0 {
            ui.label(muted("No steps yet."));
        }
        let mut remove = None;
        let mut move_step = None;
        if let Some(item) = self.settings.settings.macro_by_id_mut(id) {
            for (index, step) in item.steps.iter_mut().enumerate() {
                ui.push_id(index, |ui| {
                    egui::Frame::new()
                        .fill(ui.visuals().extreme_bg_color)
                        .stroke(ui.visuals().widgets.noninteractive.bg_stroke)
                        .corner_radius(8)
                        .inner_margin(12)
                        .show(ui, |ui| {
                            ui.horizontal_wrapped(|ui| {
                                ui.label(RichText::new(format!("Step {}", index + 1)).strong());
                                egui::ComboBox::from_id_salt("kind")
                                    .selected_text(
                                        kinds
                                            .iter()
                                            .find(|(kind, _)| *kind == step.kind)
                                            .map_or("Delay", |(_, label)| *label),
                                    )
                                    .show_ui(ui, |ui| {
                                        for (kind, label) in kinds {
                                            macro_edited |= ui
                                                .selectable_value(&mut step.kind, kind, label)
                                                .changed();
                                        }
                                    });
                            });
                            if step.kind != MacroStepKind::Delay {
                                ui.horizontal_wrapped(|ui| {
                                    let label = match step.kind {
                                        MacroStepKind::KeyDown | MacroStepKind::KeyUp => "Key name",
                                        MacroStepKind::Wheel => "Direction (up/down)",
                                        _ => "Button name",
                                    };
                                    ui.label(label);
                                    macro_edited |= ui
                                        .add(
                                            egui::TextEdit::singleline(&mut step.value)
                                                .char_limit(64)
                                                .desired_width(140.0)
                                                .hint_text(match step.kind {
                                                    MacroStepKind::Wheel => "up or down",
                                                    MacroStepKind::KeyDown
                                                    | MacroStepKind::KeyUp => "e.g. A",
                                                    _ => "e.g. Left",
                                                }),
                                        )
                                        .changed();
                                });
                            }
                            ui.horizontal_wrapped(|ui| {
                                ui.label(if step.kind == MacroStepKind::Delay {
                                    "Duration"
                                } else {
                                    "Delay after"
                                });
                                macro_edited |= ui
                                    .add(
                                        egui::DragValue::new(&mut step.delay_ms)
                                            .range(0..=60_000)
                                            .suffix(" ms"),
                                    )
                                    .changed();
                            });
                            ui.horizontal_wrapped(|ui| {
                                if ui
                                    .add_enabled(
                                        index > 0,
                                        egui::Button::new(format!("Move step {} up", index + 1)),
                                    )
                                    .clicked()
                                {
                                    move_step = Some((index, index - 1));
                                }
                                if ui
                                    .add_enabled(
                                        index + 1 < count,
                                        egui::Button::new(format!("Move step {} down", index + 1)),
                                    )
                                    .clicked()
                                {
                                    move_step = Some((index, index + 1));
                                }
                                if ui.button(format!("Remove step {}", index + 1)).clicked() {
                                    remove = Some(index);
                                }
                            });
                        });
                });
            }
        }
        if let Some((from, to)) = move_step {
            if let Some(item) = self.settings.settings.macro_by_id_mut(id) {
                item.steps.swap(from, to);
                macro_edited = true;
            }
        }
        if let Some(index) = remove {
            macro_edited |= self
                .settings
                .settings
                .remove_macro_step(id, index)
                .is_some();
        }
        if ui
            .add_enabled(count < MAX_MACRO_STEPS, egui::Button::new("Add step"))
            .clicked()
        {
            macro_edited |= self
                .settings
                .settings
                .add_macro_step(id, MacroStep::default());
        }
        if count >= MAX_MACRO_STEPS {
            ui.label(muted("Maximum of 256 steps reached."));
        }
        if macro_edited {
            self.mark_macro_edited();
        }
    }

    fn windows_input(&mut self, ui: &mut egui::Ui) {
        heading(ui, "Windows input", "Adjust mouse behavior across Windows.");
        ui.label(muted(
            "These settings affect Windows globally and persist after Daxa Control closes. Changes are only applied when you select Apply to Windows.",
        ));

        #[cfg(target_os = "windows")]
        {
            if !self.windows_input_loaded {
                self.windows_input_loaded = true;
                match windows_pointer::read_current() {
                    Ok(settings) => self.windows_input = Some(settings),
                    Err(error) => {
                        self.windows_input_message =
                            format!("Could not read Windows settings: {error}");
                    }
                }
            }

            divider(ui);
            if let Some(settings) = &mut self.windows_input {
                let mut changed = false;
                section(ui, "Double-click", "Set the maximum time between clicks.");
                changed |= ui
                    .add(
                        egui::Slider::new(&mut settings.double_click_ms, 100..=5000)
                            .clamping(egui::SliderClamping::Edits)
                            .suffix(" ms"),
                    )
                    .changed();
                section(ui, "Pointer", "Control cursor speed and acceleration.");
                changed |= ui
                    .add(
                        egui::Slider::new(&mut settings.pointer_speed, 1..=20)
                            .clamping(egui::SliderClamping::Edits)
                            .text("Speed"),
                    )
                    .changed();
                changed |= ui
                    .checkbox(
                        &mut settings.enhance_pointer_precision,
                        "Enhance pointer precision",
                    )
                    .changed();
                section(ui, "Mouse wheel", "Choose how far one wheel notch scrolls.");
                let mut page_mode = matches!(&settings.wheel_scroll, WheelScroll::Page);
                ui.horizontal_wrapped(|ui| {
                    changed |= ui.radio_value(&mut page_mode, false, "Lines").changed();
                    changed |= ui.radio_value(&mut page_mode, true, "One page").changed();
                });
                if page_mode {
                    settings.wheel_scroll = WheelScroll::Page;
                } else {
                    let mut lines = match &settings.wheel_scroll {
                        WheelScroll::Lines(lines) => *lines,
                        WheelScroll::Page => 3,
                    };
                    changed |= ui
                        .add(
                            egui::Slider::new(&mut lines, 0..=100)
                                .clamping(egui::SliderClamping::Edits)
                                .text("Lines per notch"),
                        )
                        .changed();
                    settings.wheel_scroll = WheelScroll::Lines(lines);
                }
                if changed {
                    self.windows_input_message = "Changes are not applied yet.".to_owned();
                }
            } else {
                ui.label(muted(
                    "Windows settings are unavailable. Refresh to try again.",
                ));
            }

            divider(ui);
            ui.horizontal_wrapped(|ui| {
                if ui
                    .add_enabled(self.windows_input.is_some(), egui::Button::new("Apply to Windows"))
                    .clicked()
                {
                    if let Some(settings) = &self.windows_input {
                        self.windows_input_message = match windows_pointer::apply(settings) {
                            Ok(()) => "Windows input settings applied.".to_owned(),
                            Err(error) => format!("Could not apply all settings: {error}. Some Windows settings may have changed. Refresh to see current values."),
                        };
                    }
                }
                if ui.button("Refresh from Windows").clicked() {
                    match windows_pointer::read_current() {
                        Ok(settings) => {
                            self.windows_input = Some(settings);
                            self.windows_input_message = "Showing current Windows settings.".to_owned();
                        }
                        Err(error) => {
                            self.windows_input_message = format!("Could not refresh Windows settings: {error}");
                        }
                    }
                }
            });
            ui.label(muted(
                "Refreshing replaces any changes that have not been applied.",
            ));
            if !self.windows_input_message.is_empty() {
                ui.label(&self.windows_input_message);
            }
        }
        #[cfg(not(target_os = "windows"))]
        ui.label(muted("Windows input settings are available on Windows."));
    }

    fn profiles(&mut self, ui: &mut egui::Ui) {
        heading(ui, "Profiles", "Keep different mouse setups organized.");
        section(
            ui,
            "Mouse profile",
            "Switch the profile stored on your mouse. This does not apply settings saved on this PC.",
        );
        let online = self.snapshot.connection_probe == ConnectionProbeState::Online;
        let current_id = if online {
            self.snapshot.reported_profile_id
        } else {
            None
        };
        match current_id {
            Some(id) => ui.label(format!("Current mouse profile: ID {id}")),
            None => ui.label(muted(
                "Mouse profile unavailable. Connect the mouse and wait for its status.",
            )),
        };
        let pending = matches!(
            self.snapshot.mouse_profile_switch,
            MouseProfileSwitchState::Pending { .. }
        );
        ui.horizontal_wrapped(|ui| {
            let target_label = ui.label("Switch to mouse profile");
            ui.add_enabled_ui(!pending, |ui| {
                egui::ComboBox::from_id_salt("mouse_profile_target")
                    .selected_text(format!("ID {}", self.mouse_profile_target))
                    .width(100.0)
                    .show_ui(ui, |ui| {
                        for id in 0..=3 {
                            if ui
                                .selectable_value(
                                    &mut self.mouse_profile_target,
                                    id,
                                    format!("ID {id}"),
                                )
                                .changed()
                            {
                                self.mouse_profile_message.clear();
                            }
                        }
                    })
                    .response
                    .labelled_by(target_label.id);
            });
            let can_switch = online
                && self.monitor.is_some()
                && current_id.is_some_and(|id| id != self.mouse_profile_target)
                && !pending;
            if ui
                .add_enabled(can_switch, egui::Button::new("Switch mouse profile"))
                .clicked()
            {
                self.mouse_profile_message = if self.monitor.as_ref().is_some_and(|monitor| {
                    monitor.request_profile_switch(self.mouse_profile_target)
                }) {
                    String::new()
                } else {
                    "Could not start the switch. Please try again.".to_owned()
                };
            }
        });
        if !self.mouse_profile_message.is_empty() {
            ui.label(&self.mouse_profile_message);
        } else {
            match &self.snapshot.mouse_profile_switch {
                MouseProfileSwitchState::Idle => {}
                MouseProfileSwitchState::Pending { target_id } => {
                    ui.label(muted(format!("Switching to mouse profile ID {target_id}…")));
                }
                MouseProfileSwitchState::Applied { profile_id } => {
                    ui.label(muted(format!("Mouse profile ID {profile_id} is active.")));
                }
                MouseProfileSwitchState::Failed { target_id, reason } => {
                    ui.label(format!(
                        "Could not confirm mouse profile ID {target_id}. {reason}"
                    ));
                }
            }
        }
        divider(ui);
        section(
            ui,
            "Profiles on this PC",
            "Choose a local profile to edit its settings.",
        );
        let active = self.settings.settings.active_profile;
        let names: Vec<_> = self
            .settings
            .settings
            .profiles
            .iter()
            .map(|p| p.name.clone())
            .collect();
        for (index, name) in names.iter().enumerate() {
            if ui.selectable_label(index == active, name).clicked() && index != active {
                self.commit_profile_name();
                self.settings.settings.set_active_profile(index);
                self.profile_name = self.settings.settings.active().name.clone();
                self.deleting_profile = false;
            }
        }
        ui.add_space(12.0);
        ui.horizontal_wrapped(|ui| {
            if ui
                .add_enabled(names.len() < 12, egui::Button::new("Add profile"))
                .clicked()
            {
                self.commit_profile_name();
                self.settings.settings.add_profile();
                self.profile_name = self.settings.settings.active().name.clone();
                self.deleting_profile = false;
            }
            if ui
                .add_enabled(names.len() > 1, egui::Button::new("Delete profile…"))
                .clicked()
            {
                self.deleting_profile = true;
            }
        });
        if names.len() == 12 {
            ui.label(muted("Maximum of 12 profiles reached."));
        }
        if self.deleting_profile {
            ui.add_space(6.0);
            ui.label(format!(
                "Delete ‘{}’?",
                self.settings.settings.active().name
            ));
            ui.horizontal_wrapped(|ui| {
                if ui.button("Delete").clicked() {
                    self.settings.settings.remove_active_profile();
                    self.profile_name = self.settings.settings.active().name.clone();
                    self.deleting_profile = false;
                }
                if ui.button("Cancel").clicked() {
                    self.deleting_profile = false;
                }
            });
        }
        divider(ui);
        section(
            ui,
            "Profile name",
            "Press Enter or leave the field to save the name.",
        );
        let response = ui.add(
            egui::TextEdit::singleline(&mut self.profile_name)
                .hint_text("Profile name")
                .char_limit(48)
                .desired_width(240.0),
        );
        if response.lost_focus()
            || (response.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)))
        {
            self.commit_profile_name();
        }
        divider(ui);
        ui.label(RichText::new("Import and export").size(17.0).strong());
        ui.label(muted("Export the active profile as JSON, or import a Daxa Control profile. Macros are kept separately on this PC."));
        #[cfg(target_os = "windows")]
        ui.horizontal_wrapped(|ui| {
            if ui.button("Import profile…").clicked() {
                self.commit_profile_name();
                if let Some(path) = rfd::FileDialog::new()
                    .add_filter("Daxa profile", &["json"])
                    .pick_file()
                {
                    match self.settings.import_profile(path) {
                        Ok(_) => {
                            self.profile_name = self.settings.settings.active().name.clone();
                            self.deleting_profile = false;
                            self.profile_message = "Profile imported and saved locally.".to_owned();
                            self.save_at = None;
                        }
                        Err(error) => self.profile_message = error,
                    }
                }
            }
            if ui.button("Export active profile…").clicked() {
                self.commit_profile_name();
                if let Some(path) = rfd::FileDialog::new()
                    .add_filter("Daxa profile", &["json"])
                    .set_file_name("daxa-profile.json")
                    .save_file()
                {
                    self.profile_message = match self.settings.export_active_profile(path) {
                        Ok(()) => "Profile exported.".to_owned(),
                        Err(error) => error,
                    };
                }
            }
        });
        #[cfg(not(target_os = "windows"))]
        ui.label(muted("Profile import and export require Windows."));
        if !self.profile_message.is_empty() {
            ui.label(&self.profile_message);
        }
    }
}

/// Orientation guide for the numbered assignment rows.
fn button_map(ui: &mut egui::Ui) {
    ui.vertical(|ui| {
        ui.set_width(190.0);
        ui.label(muted("BUTTON MAP").size(10.0).strong());
        let (rect, _) = ui.allocate_exact_size(egui::vec2(190.0, 200.0), egui::Sense::hover());
        let accent = theme::accent(ui);
        let soft = theme::soft(ui);
        let border = ui.visuals().widgets.noninteractive.bg_stroke.color;
        let surface = ui.visuals().extreme_bg_color;
        let painter = ui.painter_at(rect);
        let center = rect.center().x;
        let top = rect.top() + 12.0;
        let mouse =
            egui::Rect::from_min_size(egui::pos2(center - 53.0, top), egui::vec2(106.0, 166.0));
        painter.rect_filled(mouse, 48.0, surface);
        painter.rect_stroke(
            mouse,
            48.0,
            Stroke::new(1.5, border),
            egui::StrokeKind::Inside,
        );
        painter.line_segment(
            [
                egui::pos2(center, top + 4.0),
                egui::pos2(center, top + 72.0),
            ],
            Stroke::new(1.0, border),
        );
        painter.line_segment(
            [
                egui::pos2(center - 49.0, top + 74.0),
                egui::pos2(center + 49.0, top + 74.0),
            ],
            Stroke::new(1.0, border),
        );
        let wheel =
            egui::Rect::from_center_size(egui::pos2(center, top + 37.0), egui::vec2(13.0, 29.0));
        painter.rect_filled(wheel, 6.0, soft);
        painter.rect_stroke(
            wheel,
            6.0,
            Stroke::new(1.0, accent),
            egui::StrokeKind::Inside,
        );
        let dpi =
            egui::Rect::from_center_size(egui::pos2(center, top + 88.0), egui::vec2(13.0, 19.0));
        painter.rect_filled(dpi, 6.0, soft);
        painter.rect_stroke(dpi, 6.0, Stroke::new(1.0, accent), egui::StrokeKind::Inside);
        painter.line_segment(
            [
                egui::pos2(center, top + 19.0),
                egui::pos2(center, top + 25.0),
            ],
            Stroke::new(1.0, accent),
        );
        painter.line_segment(
            [
                egui::pos2(center + 7.0, top + 88.0),
                egui::pos2(center + 17.0, top + 88.0),
            ],
            Stroke::new(1.0, accent),
        );
        for (number, x, y) in [
            ("01", center - 32.0, top + 37.0),
            ("02", center + 32.0, top + 37.0),
            ("03", center, top + 8.0),
            ("04", center - 57.0, top + 82.0),
            ("05", center - 57.0, top + 112.0),
            ("06", center + 28.0, top + 88.0),
        ] {
            let point = egui::pos2(x, y);
            painter.circle_filled(point, 11.0, ACCENT);
            painter.text(
                point,
                egui::Align2::CENTER_CENTER,
                number,
                egui::FontId::proportional(10.0),
                Color32::WHITE,
            );
        }
        ui.label(muted("Numbers match the assignments."));
    });
}

impl eframe::App for DaxaApp {
    fn ui(&mut self, root: &mut egui::Ui, _frame: &mut eframe::Frame) {
        #[cfg(target_os = "windows")]
        {
            if root.ctx().input(|input| input.viewport().close_requested()) {
                self.stop_recording("Recording stopped when closing the window.", 0);
            }
            self.update_recording(root.ctx());
        }
        #[cfg(target_os = "windows")]
        if let Some(tray) = &self.tray {
            while let Some(action) = tray.next_action() {
                match action {
                    crate::tray::Action::Open => {
                        root.ctx()
                            .send_viewport_cmd(egui::ViewportCommand::Visible(true));
                        root.ctx()
                            .send_viewport_cmd(egui::ViewportCommand::Minimized(false));
                        root.ctx().send_viewport_cmd(egui::ViewportCommand::Focus);
                    }
                    crate::tray::Action::Exit => {
                        self.exiting = true;
                        root.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                }
            }
            if !self.exiting && root.ctx().input(|input| input.viewport().close_requested()) {
                root.ctx()
                    .send_viewport_cmd(egui::ViewportCommand::CancelClose);
                root.ctx()
                    .send_viewport_cmd(egui::ViewportCommand::Visible(false));
            }
        }
        if let Some(snapshot) = self.monitor.as_ref().and_then(DeviceMonitor::try_latest) {
            self.snapshot = snapshot;
        }
        let receiver = match self.snapshot.receiver {
            ReceiverState::Scanning => "Searching for device",
            ReceiverState::NotFound => "Device not found",
            ReceiverState::Detected => match self.snapshot.connection_probe {
                ConnectionProbeState::NotChecked => "Checking mouse",
                ConnectionProbeState::Online => "Mouse connected",
                ConnectionProbeState::Offline => "Mouse offline",
                ConnectionProbeState::NoResponse => "Mouse did not respond",
                ConnectionProbeState::Unavailable => "Connection check unavailable",
            },
            ReceiverState::Unavailable => "Device status unavailable",
        };
        let battery = match &self.snapshot.battery {
            BatteryState::Unavailable => "Battery unavailable".to_owned(),
            BatteryState::Ambiguous => "Battery unavailable · multiple endpoints".to_owned(),
            BatteryState::Available(reading) => {
                let age = reading.observed_at.elapsed().unwrap_or(Duration::MAX);
                let stale_after = Duration::from_secs(120);
                if age < stale_after {
                    root.ctx()
                        .request_repaint_after(stale_after - age + Duration::from_millis(1));
                }
                let charging = if reading.charging { " · charging" } else { "" };
                let stale = if age >= stale_after {
                    " · last known"
                } else {
                    ""
                };
                format!("Battery {}%{charging}{stale}", reading.percent)
            }
        };
        let connected = self.snapshot.connection_probe == ConnectionProbeState::Online;
        let fresh_percent = match &self.snapshot.battery {
            BatteryState::Available(reading)
                if connected
                    && reading.observed_at.elapsed().unwrap_or(Duration::MAX)
                        < Duration::from_secs(120) =>
            {
                Some(reading.percent)
            }
            _ => None,
        };
        let title = format!("Daxa Control · {receiver} · {battery}");
        let window_status = (connected, fresh_percent, title.clone());
        if self.window_status.as_ref() != Some(&window_status) {
            root.ctx()
                .send_viewport_cmd(egui::ViewportCommand::Title(title));
            root.ctx()
                .send_viewport_cmd(egui::ViewportCommand::Icon(Some(std::sync::Arc::new(
                    crate::taskbar::status_icon_data(connected, fresh_percent),
                ))));
            self.window_status = Some(window_status);
        }
        #[cfg(target_os = "windows")]
        if let Some(tray) = &mut self.tray {
            tray.update_status(&format!("{receiver} · {battery}"));
        }
        if !self.configured {
            theme::install(root.ctx(), self.settings.settings.dark_mode);
            self.configured = true;
        }
        let tracked_page = self.page;
        let tracked_macro = self.selected_macro;
        let before = SettingsSnapshot::capture(
            &self.settings.settings,
            tracked_page,
            tracked_macro,
            self.macro_edit_revision,
        );
        self.navigation(root, receiver, &battery);
        egui::CentralPanel::default().show(root, |ui| {
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.add_space(20.0);
                    let content_width = (ui.available_width() - 48.0).clamp(0.0, 800.0);
                    let inset = ((ui.available_width() - content_width) * 0.5).max(0.0);
                    ui.horizontal(|ui| {
                        ui.add_space(inset);
                        ui.vertical(|ui| {
                            ui.set_width(content_width);
                            match self.page {
                                Page::Overview => self.overview(ui, receiver, &battery),
                                Page::Buttons => self.buttons(ui),
                                Page::Performance => self.performance(ui),
                                Page::Lighting => self.lighting(ui),
                                Page::Macros => self.macros(ui),
                                Page::Profiles => self.profiles(ui),
                                Page::WindowsInput => self.windows_input(ui),
                            }
                            ui.add_space(30.0);
                        });
                    });
                });
        });
        if before
            != SettingsSnapshot::capture(
                &self.settings.settings,
                tracked_page,
                tracked_macro,
                self.macro_edit_revision,
            )
        {
            if self.page == Page::Profiles {
                let _ = self.settings.save_if_dirty();
                self.save_at = None;
                root.ctx().request_repaint();
            } else {
                self.save_at = Some(Instant::now() + Duration::from_millis(500));
            }
        }
        if let Some(deadline) = self.save_at {
            if Instant::now() >= deadline {
                let _ = self.settings.save_if_dirty();
                self.save_at = None;
                root.ctx().request_repaint();
            } else {
                root.ctx()
                    .request_repaint_after(deadline.saturating_duration_since(Instant::now()));
            }
        }
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        #[cfg(target_os = "windows")]
        self.stop_recording("Recording stopped when exiting.", 0);
        self.commit_profile_name();
        self.commit_macro_name();
        let _ = self.settings.save_if_dirty();
    }
}
