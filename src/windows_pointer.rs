//! Read and explicitly apply Windows-wide mouse pointer preferences.
//!
//! These settings are independent of the Daxa receiver. Call [`apply`] only
//! after a user explicitly asks to change Windows pointer behavior.

use std::ffi::c_void;

const SPI_GETMOUSE: u32 = 0x0003;
const SPI_SETMOUSE: u32 = 0x0004;
const SPI_SETDOUBLECLICKTIME: u32 = 0x0020;
const SPI_GETWHEELSCROLLLINES: u32 = 0x0068;
const SPI_SETWHEELSCROLLLINES: u32 = 0x0069;
const SPI_GETMOUSESPEED: u32 = 0x0070;
const SPI_SETMOUSESPEED: u32 = 0x0071;
const SPIF_UPDATEINIFILE: u32 = 0x0001;
const SPIF_SENDCHANGE: u32 = 0x0002;
const WHEEL_PAGESCROLL: u32 = u32::MAX;
const MAX_DOUBLE_CLICK_MS: u32 = 5_000;
const MAX_WHEEL_LINES: u32 = 100;
const PERSIST_AND_NOTIFY: u32 = SPIF_UPDATEINIFILE | SPIF_SENDCHANGE;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WheelScroll {
    Lines(u32),
    Page,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WindowsPointerSettings {
    pub double_click_ms: u32,
    pub pointer_speed: u32,
    pub wheel_scroll: WheelScroll,
    pub enhance_pointer_precision: bool,
}

/// Read the current user-wide pointer settings without changing them.
pub fn read_current() -> Result<WindowsPointerSettings, String> {
    let double_click_ms = unsafe { GetDoubleClickTime() };
    if double_click_ms == 0 {
        return Err("Windows returned an invalid double-click time.".to_owned());
    }

    let mut mouse_info = [0_i32; 3];
    read_parameter(SPI_GETMOUSE, &mut mouse_info, "mouse acceleration")?;

    let mut pointer_speed = 0_i32;
    read_parameter(SPI_GETMOUSESPEED, &mut pointer_speed, "pointer speed")?;
    if !(1..=20).contains(&pointer_speed) {
        return Err("Windows returned an invalid pointer speed.".to_owned());
    }

    let mut wheel_lines = 0_u32;
    read_parameter(
        SPI_GETWHEELSCROLLLINES,
        &mut wheel_lines,
        "wheel scroll amount",
    )?;

    Ok(WindowsPointerSettings {
        double_click_ms,
        pointer_speed: pointer_speed as u32,
        wheel_scroll: if wheel_lines == WHEEL_PAGESCROLL {
            WheelScroll::Page
        } else {
            WheelScroll::Lines(wheel_lines)
        },
        enhance_pointer_precision: mouse_info[2] != 0,
    })
}

/// Apply values to Windows and persist them in the current user profile.
///
/// The caller should invoke this only after an explicit user action. Windows
/// updates these parameters one at a time, so a later failure can leave earlier
/// settings applied; the error identifies those earlier changes.
pub fn apply(settings: &WindowsPointerSettings) -> Result<(), String> {
    validate(settings)?;

    let mut applied = Vec::with_capacity(4);

    // SPI_SETDOUBLECLICKTIME takes the duration in `uiParam` and no data
    // pointer. Persist and broadcast each change so other apps update too.
    apply_parameter(
        SPI_SETDOUBLECLICKTIME,
        settings.double_click_ms,
        std::ptr::null_mut(),
        "double-click speed",
        &mut applied,
    )?;

    // The original Daxa utility uses these thresholds when pointer precision
    // is enabled and zeros them when it is disabled.
    let mut mouse_info = if settings.enhance_pointer_precision {
        [6_i32, 10, 1]
    } else {
        [0_i32; 3]
    };
    apply_parameter(
        SPI_SETMOUSE,
        0,
        mouse_info.as_mut_ptr().cast(),
        "pointer precision",
        &mut applied,
    )?;

    let mut pointer_speed = settings.pointer_speed as i32;
    apply_parameter(
        SPI_SETMOUSESPEED,
        0,
        (&mut pointer_speed as *mut i32).cast(),
        "pointer speed",
        &mut applied,
    )?;

    let wheel_lines = match settings.wheel_scroll {
        WheelScroll::Lines(lines) => lines,
        WheelScroll::Page => WHEEL_PAGESCROLL,
    };
    apply_parameter(
        SPI_SETWHEELSCROLLLINES,
        wheel_lines,
        std::ptr::null_mut(),
        "wheel scroll amount",
        &mut applied,
    )?;

    Ok(())
}

fn validate(settings: &WindowsPointerSettings) -> Result<(), String> {
    if !(1..=MAX_DOUBLE_CLICK_MS).contains(&settings.double_click_ms) {
        return Err(format!(
            "Double-click time must be between 1 and {MAX_DOUBLE_CLICK_MS} ms."
        ));
    }
    if !(1..=20).contains(&settings.pointer_speed) {
        return Err("Pointer speed must be between 1 and 20.".to_owned());
    }
    if let WheelScroll::Lines(lines) = settings.wheel_scroll
        && lines > MAX_WHEEL_LINES
    {
        return Err(format!(
            "Wheel scrolling must be between 0 and {MAX_WHEEL_LINES} lines, or one page."
        ));
    }
    Ok(())
}

fn read_parameter<T>(action: u32, value: &mut T, setting: &str) -> Result<(), String> {
    // SAFETY: The pointer refers to an initialized writable value of the size
    // expected by the selected SystemParametersInfo action and remains valid
    // until the synchronous Windows call returns.
    let success = unsafe { SystemParametersInfoW(action, 0, (value as *mut T).cast(), 0) };
    if success == 0 {
        Err(format!("Windows could not read {setting}."))
    } else {
        Ok(())
    }
}

fn apply_parameter(
    action: u32,
    ui_param: u32,
    data: *mut c_void,
    setting: &str,
    applied: &mut Vec<&'static str>,
) -> Result<(), String> {
    // SAFETY: Each call site supplies the parameter shape required by its
    // SystemParametersInfo action; any non-null data pointer remains valid
    // through the synchronous call.
    let success = unsafe { SystemParametersInfoW(action, ui_param, data, PERSIST_AND_NOTIFY) };
    if success == 0 {
        let previous = if applied.is_empty() {
            String::new()
        } else {
            format!(
                " Earlier settings may already have changed: {}.",
                applied.join(", ")
            )
        };
        Err(format!("Windows could not apply {setting}.{previous}"))
    } else {
        // These are fixed labels selected only at the internal call sites.
        applied.push(match setting {
            "double-click speed" => "double-click speed",
            "pointer precision" => "pointer precision",
            "pointer speed" => "pointer speed",
            "wheel scroll amount" => "wheel scroll amount",
            _ => "another pointer setting",
        });
        Ok(())
    }
}

#[link(name = "user32")]
unsafe extern "system" {
    fn GetDoubleClickTime() -> u32;
    fn SystemParametersInfoW(action: u32, ui_param: u32, data: *mut c_void, flags: u32) -> i32;
}
