//! Windows notification-area interaction. Created on eframe's event-loop thread.

use std::sync::mpsc::{self, Receiver};

use eframe::egui;
use tray_icon::{
    Icon, MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent,
    menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem},
};

const OPEN_ID: &str = "daxa-open";
const EXIT_ID: &str = "daxa-exit";

pub enum Action {
    Open,
    Exit,
}

pub struct Tray {
    icon: TrayIcon,
    battery: MenuItem,
    current_status: String,
    actions: Receiver<Action>,
}

impl Tray {
    pub fn new(ctx: egui::Context) -> Result<Self, String> {
        let menu = Menu::new();
        let battery = MenuItem::new("Battery unavailable", false, None);
        let open = MenuItem::with_id(OPEN_ID, "&Open Daxa Control", true, None);
        let exit = MenuItem::with_id(EXIT_ID, "E&xit", true, None);
        menu.append_items(&[&battery, &PredefinedMenuItem::separator(), &open, &exit])
            .map_err(|error| error.to_string())?;

        let icon = TrayIconBuilder::new()
            .with_icon(app_icon()?)
            .with_tooltip("Daxa Control · Battery unavailable")
            .with_menu(Box::new(menu))
            .build()
            .map_err(|error| error.to_string())?;

        // The handlers wake eframe's event loop even while its window is hidden.
        let (sender, actions) = mpsc::channel();
        let menu_sender = sender.clone();
        let menu_ctx = ctx.clone();
        MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
            let action = if event.id.0 == OPEN_ID {
                Some(Action::Open)
            } else if event.id.0 == EXIT_ID {
                Some(Action::Exit)
            } else {
                None
            };
            if let Some(action) = action {
                let _ = menu_sender.send(action);
                menu_ctx.request_repaint();
            }
        }));
        TrayIconEvent::set_event_handler(Some(move |event| {
            if matches!(
                event,
                TrayIconEvent::Click {
                    button: MouseButton::Left,
                    button_state: MouseButtonState::Up,
                    ..
                }
            ) {
                let _ = sender.send(Action::Open);
                ctx.request_repaint();
            }
        }));

        Ok(Self {
            icon,
            battery,
            current_status: "Battery unavailable".to_owned(),
            actions,
        })
    }

    pub fn update_status(&mut self, status: &str) {
        if self.current_status != status {
            self.battery.set_text(status);
            let _ = self
                .icon
                .set_tooltip(Some(format!("Daxa Control · {status}")));
            self.current_status = status.to_owned();
        }
    }

    pub fn next_action(&self) -> Option<Action> {
        self.actions.try_recv().ok()
    }
}

// A small original mark in the interface's accent color; no external asset is
// required for the notification area. Transparent corners suit both themes.
fn app_icon() -> Result<Icon, String> {
    const SIZE: usize = 32;
    let mut pixels = vec![0; SIZE * SIZE * 4];
    for y in 2_usize..30 {
        for x in 2_usize..30 {
            let dx = if x < 6 { 6 - x } else { x.saturating_sub(25) };
            let dy = if y < 6 { 6 - y } else { y.saturating_sub(25) };
            if dx * dx + dy * dy > 16 {
                continue;
            }
            let i = (y * SIZE + x) * 4;
            pixels[i..i + 4].copy_from_slice(&[38, 111, 94, 255]);
            let letter = (x >= 9 && x <= 12 && (9..=23).contains(&y))
                || ((10..=19).contains(&x) && (y == 9 || y == 23))
                || ((19..=22).contains(&x)
                    && (10..=22).contains(&y)
                    && ((x == 22 && (13..=19).contains(&y))
                        || (x == 20 && (y == 10 || y == 22))
                        || (x == 21 && (11..=12).contains(&y))
                        || (x == 21 && (20..=21).contains(&y))));
            if letter {
                pixels[i..i + 4].copy_from_slice(&[255, 255, 255, 255]);
            }
        }
    }
    Icon::from_rgba(pixels, SIZE as u32, SIZE as u32).map_err(|error| error.to_string())
}
