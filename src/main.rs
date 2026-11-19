#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

mod device;
#[cfg(target_os = "windows")]
mod macro_recorder;
mod settings;
mod taskbar;
mod theme;
#[cfg(target_os = "windows")]
mod tray;
mod ui;
#[cfg(target_os = "windows")]
mod windows_pointer;

const APP_TITLE: &str = "Daxa Control";

fn main() {
    if let Err(error) = run() {
        show_startup_error(&error.to_string());
    }
}

fn run() -> eframe::Result {
    let options = eframe::NativeOptions {
        renderer: eframe::Renderer::Glow,
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title(APP_TITLE)
            .with_inner_size([1180.0, 780.0])
            .with_min_inner_size([900.0, 620.0]),
        ..Default::default()
    };

    eframe::run_native(
        APP_TITLE,
        options,
        Box::new(|creation_context| {
            Ok(Box::new(ui::DaxaApp::new(
                creation_context.egui_ctx.clone(),
            )))
        }),
    )
}

#[cfg(target_os = "windows")]
fn show_startup_error(message: &str) {
    use std::ffi::c_void;

    const MB_OK: u32 = 0x0000;
    const MB_ICONERROR: u32 = 0x0010;
    const CAPTION: &str = "Daxa Control could not start";

    #[link(name = "user32")]
    unsafe extern "system" {
        fn MessageBoxW(
            window: *mut c_void,
            text: *const u16,
            caption: *const u16,
            kind: u32,
        ) -> i32;
    }

    let message: Vec<u16> = message.encode_utf16().chain([0]).collect();
    let caption: Vec<u16> = CAPTION.encode_utf16().chain([0]).collect();

    // SAFETY: Both strings are NUL-terminated UTF-16 buffers that remain alive
    // for the duration of the synchronous Windows API call.
    unsafe {
        MessageBoxW(
            std::ptr::null_mut(),
            message.as_ptr(),
            caption.as_ptr(),
            MB_OK | MB_ICONERROR,
        );
    }
}

#[cfg(not(target_os = "windows"))]
fn show_startup_error(message: &str) {
    eprintln!("{APP_TITLE} could not start: {message}");
}
