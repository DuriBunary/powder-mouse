//! Explicitly started, local-only macro input capture for Windows.
//!
//! Low-level hooks are installed only while recording. Events produced by
//! software and events sent while Daxa Control is foreground are ignored so
//! the recorder cannot capture its own controls or edit fields.

use crate::settings::{MAX_MACRO_STEPS, MacroStep, MacroStepKind};
use std::collections::HashSet;
use std::ffi::c_void;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender, TrySendError};
use std::sync::{Mutex, OnceLock};

const WH_KEYBOARD_LL: i32 = 13;
const WH_MOUSE_LL: i32 = 14;
const WM_KEYDOWN: usize = 0x0100;
const WM_KEYUP: usize = 0x0101;
const WM_SYSKEYDOWN: usize = 0x0104;
const WM_SYSKEYUP: usize = 0x0105;
const WM_LBUTTONDOWN: usize = 0x0201;
const WM_LBUTTONUP: usize = 0x0202;
const WM_RBUTTONDOWN: usize = 0x0204;
const WM_RBUTTONUP: usize = 0x0205;
const WM_MBUTTONDOWN: usize = 0x0207;
const WM_MBUTTONUP: usize = 0x0208;
const WM_MOUSEWHEEL: usize = 0x020A;
const WM_XBUTTONDOWN: usize = 0x020B;
const WM_XBUTTONUP: usize = 0x020C;
const LLKHF_LOWER_IL_INJECTED: u32 = 0x02;
const LLKHF_INJECTED: u32 = 0x10;
const LLMHF_INJECTED: u32 = 0x01;
const LLMHF_LOWER_IL_INJECTED: u32 = 0x02;
const EVENT_QUEUE_CAPACITY: usize = 1024;
const MAX_STEP_DELAY_MS: u32 = 60_000;

type HHook = *mut c_void;
type HookProc = unsafe extern "system" fn(code: i32, wparam: usize, lparam: isize) -> isize;

#[derive(Clone, Debug)]
struct TimedInput {
    kind: MacroStepKind,
    value: String,
    tick_ms: u32,
}

#[repr(C)]
struct KeyboardHookData {
    virtual_key: u32,
    scan_code: u32,
    flags: u32,
    time: u32,
    extra_info: usize,
}

#[repr(C)]
struct Point {
    x: i32,
    y: i32,
}

#[repr(C)]
struct MouseHookData {
    point: Point,
    mouse_data: u32,
    flags: u32,
    time: u32,
    extra_info: usize,
}

static EVENT_SENDER: OnceLock<Mutex<Option<SyncSender<TimedInput>>>> = OnceLock::new();
static PRESSED_KEYS: OnceLock<Mutex<HashSet<u32>>> = OnceLock::new();
static PRESSED_MOUSE_BUTTONS: OnceLock<Mutex<HashSet<u8>>> = OnceLock::new();
static DROPPED_EVENTS: AtomicUsize = AtomicUsize::new(0);

/// Captures input on the current UI thread. The thread's existing message loop
/// dispatches low-level hook events, keeping capture lightweight without a
/// polling worker.
pub(crate) struct MacroRecorder {
    keyboard_hook: HHook,
    mouse_hook: HHook,
    receiver: Option<Receiver<TimedInput>>,
    steps: Vec<MacroStep>,
    last_tick_ms: Option<u32>,
}

impl Default for MacroRecorder {
    fn default() -> Self {
        Self {
            keyboard_hook: std::ptr::null_mut(),
            mouse_hook: std::ptr::null_mut(),
            receiver: None,
            steps: Vec::new(),
            last_tick_ms: None,
        }
    }
}

impl MacroRecorder {
    /// Start capturing keyboard and mouse actions until `stop` is called.
    pub(crate) fn start(&mut self) -> Result<(), String> {
        if self.is_recording() {
            return Err("Macro recording is already active".to_owned());
        }

        let (sender, receiver) = mpsc::sync_channel(EVENT_QUEUE_CAPACITY);
        let sender_slot = EVENT_SENDER.get_or_init(|| Mutex::new(None));
        let mut active_sender = sender_slot
            .lock()
            .map_err(|_| "Macro recorder could not initialize its event queue".to_owned())?;
        if active_sender.is_some() {
            return Err("Another macro recording session is already active".to_owned());
        }

        let keys = PRESSED_KEYS.get_or_init(|| Mutex::new(HashSet::new()));
        keys.lock()
            .map_err(|_| "Macro recorder could not reset key state".to_owned())?
            .clear();
        let buttons = PRESSED_MOUSE_BUTTONS.get_or_init(|| Mutex::new(HashSet::new()));
        buttons
            .lock()
            .map_err(|_| "Macro recorder could not reset mouse state".to_owned())?
            .clear();
        DROPPED_EVENTS.store(0, Ordering::Release);
        self.steps.clear();
        self.last_tick_ms = None;
        *active_sender = Some(sender);
        drop(active_sender);

        // SAFETY: The null module name requests the current process module.
        let module = unsafe { GetModuleHandleW(std::ptr::null()) };
        if module.is_null() {
            self.clear_sender();
            return Err(last_error_message("Could not find the Daxa Control module"));
        }

        // SAFETY: The hooks call static callbacks with the corresponding
        // Windows hook structures. The eframe UI thread has a message loop.
        self.keyboard_hook =
            unsafe { SetWindowsHookExW(WH_KEYBOARD_LL, keyboard_hook_proc, module, 0) };
        if self.keyboard_hook.is_null() {
            let error = last_error_message("Could not start keyboard capture");
            self.clear_sender();
            return Err(error);
        }

        // SAFETY: Same lifetime and callback guarantees as the keyboard hook.
        self.mouse_hook = unsafe { SetWindowsHookExW(WH_MOUSE_LL, mouse_hook_proc, module, 0) };
        if self.mouse_hook.is_null() {
            let error = last_error_message("Could not start mouse capture");
            self.unhook_keyboard();
            self.clear_sender();
            return Err(error);
        }

        self.receiver = Some(receiver);
        Ok(())
    }

    pub(crate) fn is_recording(&self) -> bool {
        !self.keyboard_hook.is_null() && !self.mouse_hook.is_null()
    }

    /// Drain pending hook events and return the current step count without
    /// cloning the recorded action sequence.
    pub(crate) fn step_count(&mut self) -> usize {
        self.collect_events();
        self.steps.len()
    }

    /// Stop capture and return the complete action sequence.
    pub(crate) fn stop(&mut self) -> Vec<MacroStep> {
        let was_recording = self.is_recording();
        self.unhook_mouse();
        self.unhook_keyboard();
        if was_recording {
            self.clear_sender();
            if let Some(keys) = PRESSED_KEYS.get() {
                if let Ok(mut keys) = keys.lock() {
                    keys.clear();
                }
            }
            if let Some(buttons) = PRESSED_MOUSE_BUTTONS.get() {
                if let Ok(mut buttons) = buttons.lock() {
                    buttons.clear();
                }
            }
        }
        self.collect_events();
        self.receiver = None;
        self.last_tick_ms = None;
        std::mem::take(&mut self.steps)
    }

    /// Return the number of actions dropped because the bounded queue was full.
    pub(crate) fn take_dropped_events(&mut self) -> usize {
        DROPPED_EVENTS.swap(0, Ordering::AcqRel)
    }

    fn collect_events(&mut self) {
        let Some(receiver) = self.receiver.as_ref() else {
            return;
        };

        for input in receiver.try_iter() {
            if self.steps.len() >= MAX_MACRO_STEPS {
                DROPPED_EVENTS.fetch_add(1, Ordering::Relaxed);
                continue;
            }
            if let Some(previous) = self.steps.last_mut() {
                let elapsed = input
                    .tick_ms
                    .wrapping_sub(self.last_tick_ms.unwrap_or(input.tick_ms));
                previous.delay_ms = elapsed.min(MAX_STEP_DELAY_MS);
            }
            self.steps.push(MacroStep {
                kind: input.kind,
                value: input.value,
                delay_ms: 0,
            });
            self.last_tick_ms = Some(input.tick_ms);
        }
    }

    fn clear_sender(&mut self) {
        if let Some(sender) = EVENT_SENDER.get() {
            if let Ok(mut sender) = sender.lock() {
                *sender = None;
            }
        }
    }

    fn unhook_mouse(&mut self) {
        if !self.mouse_hook.is_null() {
            // SAFETY: This hook handle was created by this recorder and is
            // released exactly once before the recorder is dropped.
            unsafe { UnhookWindowsHookEx(self.mouse_hook) };
            self.mouse_hook = std::ptr::null_mut();
        }
    }

    fn unhook_keyboard(&mut self) {
        if !self.keyboard_hook.is_null() {
            // SAFETY: This hook handle was created by this recorder and is
            // released exactly once before the recorder is dropped.
            unsafe { UnhookWindowsHookEx(self.keyboard_hook) };
            self.keyboard_hook = std::ptr::null_mut();
        }
    }
}

impl Drop for MacroRecorder {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}

unsafe extern "system" fn keyboard_hook_proc(code: i32, wparam: usize, lparam: isize) -> isize {
    if code >= 0 {
        let data = lparam as *const KeyboardHookData;
        // SAFETY: Windows supplies KBDLLHOOKSTRUCT for WH_KEYBOARD_LL events.
        if let Some(data) = unsafe { data.as_ref() }
            && data.flags & (LLKHF_INJECTED | LLKHF_LOWER_IL_INJECTED) == 0
        {
            let (kind, is_down) = match wparam {
                WM_KEYDOWN | WM_SYSKEYDOWN => (MacroStepKind::KeyDown, true),
                WM_KEYUP | WM_SYSKEYUP => (MacroStepKind::KeyUp, false),
                _ => return call_next_keyboard_hook(code, wparam, lparam),
            };

            let pressed = PRESSED_KEYS.get_or_init(|| Mutex::new(HashSet::new()));
            let Ok(mut pressed) = pressed.try_lock() else {
                DROPPED_EVENTS.fetch_add(1, Ordering::Relaxed);
                return call_next_keyboard_hook(code, wparam, lparam);
            };
            let should_capture = if is_down {
                !foreground_is_daxa_control() && pressed.insert(data.virtual_key)
            } else {
                pressed.remove(&data.virtual_key)
            };
            drop(pressed);
            if should_capture {
                send_input(TimedInput {
                    kind,
                    value: key_name(data.virtual_key),
                    tick_ms: data.time,
                });
            }
        }
    }

    call_next_keyboard_hook(code, wparam, lparam)
}

unsafe extern "system" fn mouse_hook_proc(code: i32, wparam: usize, lparam: isize) -> isize {
    if code >= 0 {
        let data = lparam as *const MouseHookData;
        // SAFETY: Windows supplies MSLLHOOKSTRUCT for WH_MOUSE_LL events.
        if let Some(data) = unsafe { data.as_ref() }
            && data.flags & (LLMHF_INJECTED | LLMHF_LOWER_IL_INJECTED) == 0
        {
            let external_foreground = match wparam {
                WM_LBUTTONDOWN | WM_RBUTTONDOWN | WM_MBUTTONDOWN | WM_XBUTTONDOWN
                | WM_MOUSEWHEEL => !foreground_is_daxa_control(),
                _ => false,
            };
            let action = match wparam {
                WM_LBUTTONDOWN => Some((0, true, "Left")),
                WM_LBUTTONUP => Some((0, false, "Left")),
                WM_RBUTTONDOWN => Some((1, true, "Right")),
                WM_RBUTTONUP => Some((1, false, "Right")),
                WM_MBUTTONDOWN => Some((2, true, "Middle")),
                WM_MBUTTONUP => Some((2, false, "Middle")),
                WM_XBUTTONDOWN => match (data.mouse_data >> 16) as u16 {
                    1 => Some((3, true, "X1")),
                    2 => Some((4, true, "X2")),
                    _ => None,
                },
                WM_XBUTTONUP => match (data.mouse_data >> 16) as u16 {
                    1 => Some((3, false, "X1")),
                    2 => Some((4, false, "X2")),
                    _ => None,
                },
                WM_MOUSEWHEEL if external_foreground => {
                    let direction = if (data.mouse_data >> 16) as i16 > 0 {
                        "up"
                    } else {
                        "down"
                    };
                    send_input(TimedInput {
                        kind: MacroStepKind::Wheel,
                        value: direction.to_owned(),
                        tick_ms: data.time,
                    });
                    None
                }
                _ => None,
            };
            if let Some((button, is_down, value)) = action {
                let pressed = PRESSED_MOUSE_BUTTONS.get_or_init(|| Mutex::new(HashSet::new()));
                if let Ok(mut pressed) = pressed.try_lock() {
                    let should_capture = if is_down {
                        external_foreground && pressed.insert(button)
                    } else {
                        pressed.remove(&button)
                    };
                    drop(pressed);
                    if should_capture {
                        send_input(TimedInput {
                            kind: if is_down {
                                MacroStepKind::MouseButtonDown
                            } else {
                                MacroStepKind::MouseButtonUp
                            },
                            value: value.to_owned(),
                            tick_ms: data.time,
                        });
                    }
                } else {
                    DROPPED_EVENTS.fetch_add(1, Ordering::Relaxed);
                }
            }
        }
    }

    call_next_mouse_hook(code, wparam, lparam)
}

fn send_input(input: TimedInput) {
    let Some(sender) = EVENT_SENDER.get() else {
        return;
    };
    let Ok(sender) = sender.try_lock() else {
        DROPPED_EVENTS.fetch_add(1, Ordering::Relaxed);
        return;
    };
    let Some(sender) = sender.as_ref() else {
        return;
    };
    if let Err(TrySendError::Full(_)) = sender.try_send(input) {
        DROPPED_EVENTS.fetch_add(1, Ordering::Relaxed);
    }
}

fn foreground_is_daxa_control() -> bool {
    // SAFETY: These APIs read the foreground window and owning process only.
    unsafe {
        let window = GetForegroundWindow();
        if window.is_null() {
            return true;
        }
        let mut process_id = 0;
        GetWindowThreadProcessId(window, &mut process_id);
        process_id == GetCurrentProcessId()
    }
}

fn key_name(virtual_key: u32) -> String {
    if (0x41..=0x5A).contains(&virtual_key) || (0x30..=0x39).contains(&virtual_key) {
        return char::from_u32(virtual_key).unwrap_or('?').to_string();
    }
    if (0x70..=0x87).contains(&virtual_key) {
        return format!("F{}", virtual_key - 0x6F);
    }

    let name = match virtual_key {
        0x08 => "Backspace",
        0x09 => "Tab",
        0x0D => "Enter",
        0x10 | 0xA0 | 0xA1 => "Shift",
        0x11 | 0xA2 | 0xA3 => "Ctrl",
        0x12 | 0xA4 | 0xA5 => "Alt",
        0x13 => "Pause",
        0x14 => "Caps Lock",
        0x1B => "Escape",
        0x20 => "Space",
        0x21 => "Page Up",
        0x22 => "Page Down",
        0x23 => "End",
        0x24 => "Home",
        0x25 => "Left",
        0x26 => "Up",
        0x27 => "Right",
        0x28 => "Down",
        0x2D => "Insert",
        0x2E => "Delete",
        0x5B | 0x5C => "Windows",
        0x6A => "Numpad *",
        0x6B => "Numpad +",
        0x6D => "Numpad -",
        0x6E => "Numpad .",
        0x6F => "Numpad /",
        0x90 => "Num Lock",
        0x91 => "Scroll Lock",
        0xBA => ";",
        0xBB => "=",
        0xBC => ",",
        0xBD => "-",
        0xBE => ".",
        0xBF => "/",
        0xC0 => "`",
        0xDB => "[",
        0xDC => "\\",
        0xDD => "]",
        0xDE => "'",
        _ => return format!("VK 0x{virtual_key:02X}"),
    };
    name.to_owned()
}

fn call_next_keyboard_hook(code: i32, wparam: usize, lparam: isize) -> isize {
    // SAFETY: This forwards the hook invocation unchanged to the next hook.
    unsafe { CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam) }
}

fn call_next_mouse_hook(code: i32, wparam: usize, lparam: isize) -> isize {
    // SAFETY: This forwards the hook invocation unchanged to the next hook.
    unsafe { CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam) }
}

fn last_error_message(context: &str) -> String {
    // SAFETY: GetLastError returns the current thread's last Win32 error code.
    let error = unsafe { GetLastError() };
    format!("{context} (Windows error {error})")
}

#[link(name = "user32")]
unsafe extern "system" {
    fn SetWindowsHookExW(
        hook_id: i32,
        callback: HookProc,
        module: *mut c_void,
        thread_id: u32,
    ) -> HHook;
    fn CallNextHookEx(hook: HHook, code: i32, wparam: usize, lparam: isize) -> isize;
    fn UnhookWindowsHookEx(hook: HHook) -> i32;
    fn GetForegroundWindow() -> HHook;
    fn GetWindowThreadProcessId(window: HHook, process_id: *mut u32) -> u32;
}

#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetModuleHandleW(module_name: *const u16) -> *mut c_void;
    fn GetCurrentProcessId() -> u32;
    fn GetLastError() -> u32;
}
