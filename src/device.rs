//! Background HID discovery and battery-only telemetry for the Daxa receiver.
//!
//! Receiver enumeration does not establish that the mouse itself is online.
//! The battery command is sent only after a read-only feature-report probe
//! identifies exactly one matching vendor collection.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, TryLockError, mpsc};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant, SystemTime};

const VENDOR_ID: u16 = 0x25A7;
const PRODUCT_IDS: [u16; 2] = [0xFA7B, 0xFA7C];
const VENDOR_USAGE_PAGE_MIN: u16 = 0xFF00;
const DISCOVERY_INTERVAL: Duration = Duration::from_secs(5);
const BATTERY_INTERVAL: Duration = Duration::from_secs(60);
const BATTERY_REPLY_TIMEOUT: Duration = Duration::from_secs(1);
const CONNECTION_STARTUP_PROBE_TIMEOUT: Duration = Duration::from_millis(100);
const CONNECTION_STATUS_REPLY_TIMEOUT: Duration = Duration::from_millis(1_010);
const CONNECTION_STATUS_MAX_READS: usize = 50;
const CONNECTION_STATUS_MAX_ATTEMPTS: usize = 3;
const CURRENT_PROFILE_REPLY_TIMEOUT: Duration = Duration::from_secs(1);
const PROFILE_SWITCH_REPLY_TIMEOUT: Duration = Duration::from_secs(1);
const FEATURE_REPORT_ID: u8 = 0x08;
const INPUT_REPORT_OFFSET: usize = 1;
const BATTERY_INPUT_REPORT_ID: u8 = 0x04;
const CURRENT_PROFILE_INPUT_REPORT_ID: u8 = 0x0E;
const PROFILE_SWITCH_INPUT_REPORT_ID: u8 = 0x10;
const RESPONSE_STATUS_INDEX: usize = 1;
const RESPONSE_FIRST_DATA_INDEX: usize = 5;
const BATTERY_STATUS_INDEX: usize = 1;
const BATTERY_PERCENT_INDEX: usize = 5;
const BATTERY_CHARGING_INDEX: usize = 6;
const CONNECTION_SETUP_COMMAND: [u8; 17] =
    [0x08, 0x03, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x4A];
const CONNECTION_STARTUP_PROBE_COMMAND: [u8; 17] =
    [0x08, 0x03, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
const CURRENT_PROFILE_COMMAND: [u8; 17] =
    [0x08, 0x0E, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x3F];
const BATTERY_COMMAND: [u8; 17] = [0x08, 0x04, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x49];
pub const DPI_AND_COLOR_WRITE_UNSUPPORTED_REASON: &str =
    "Apply stays paused until changed DPI and color values are verified on the mouse";

/// Settings explicitly submitted for the connected mouse.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DpiAndColorSettings {
    pub x_stages: Vec<u16>,
    pub y_stages: Option<Vec<u16>>,
    /// Six RGB triples, one per DPI stage.
    pub colors: [[u8; 3]; 6],
}

/// Result of the most recent explicitly requested write operation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DpiAndColorApplyState {
    Idle,
    Pending,
    Unsupported {
        reason: String,
    },
    /// Reserved while device writes remain gated on live protocol verification.
    #[allow(dead_code)]
    Applied {
        acknowledged_chunks: usize,
    },
    Failed {
        acknowledged_chunks: usize,
        reason: String,
    },
}

/// Result of an explicit request to switch the mouse's onboard profile.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub enum MouseProfileSwitchState {
    #[default]
    Idle,
    Pending {
        target_id: u8,
    },
    Applied {
        profile_id: u8,
    },
    Failed {
        target_id: u8,
        reason: String,
    },
}

#[cfg(target_os = "windows")]
const ERROR_IO_PENDING: u32 = 997;
#[cfg(target_os = "windows")]
const ERROR_NO_UNICODE_TRANSLATION: u32 = 1113;
#[cfg(target_os = "windows")]
const FILE_FLAG_OVERLAPPED: u32 = 0x4000_0000;
#[cfg(target_os = "windows")]
const FILE_GENERIC_READ: u32 = 0x0012_0089;
#[cfg(target_os = "windows")]
const FILE_GENERIC_WRITE: u32 = 0x0012_0116;
#[cfg(target_os = "windows")]
const FILE_SHARE_READ: u32 = 0x0000_0001;
#[cfg(target_os = "windows")]
const FILE_SHARE_WRITE: u32 = 0x0000_0002;
#[cfg(target_os = "windows")]
const OPEN_EXISTING: u32 = 3;
#[cfg(target_os = "windows")]
const WAIT_OBJECT_0: u32 = 0;
#[cfg(target_os = "windows")]
const WAIT_TIMEOUT: u32 = 258;

#[cfg(target_os = "windows")]
struct NativeHandle(*mut std::ffi::c_void);

#[cfg(target_os = "windows")]
impl Drop for NativeHandle {
    fn drop(&mut self) {
        if !self.0.is_null() && self.0 as isize != -1 {
            // SAFETY: This wrapper owns the handle and closes it once.
            unsafe { CloseHandle(self.0) };
        }
    }
}

#[cfg(target_os = "windows")]
#[repr(C)]
struct NativeOverlapped {
    internal: usize,
    internal_high: usize,
    offset: u32,
    offset_high: u32,
    event: *mut std::ffi::c_void,
}

#[cfg(target_os = "windows")]
impl NativeOverlapped {
    fn new(event: *mut std::ffi::c_void) -> Self {
        Self {
            internal: 0,
            internal_high: 0,
            offset: 0,
            offset_high: 0,
            event,
        }
    }
}

/// Separate Win32 handles own feature writes and overlapped input reads,
/// matching OemDrv's open semantics. The endpoint paths may differ: wired
/// PID 0xFA7B was observed to send on FF02/0002 and reply on FF01/0000, while
/// other layouts fall back to one path. Qualification remains a read-only
/// hidapi probe. The split also avoids hidapi 2.6.7's overlapped ReadFile
/// wrapper, which passes a non-null byte-count pointer alongside
/// FILE_FLAG_OVERLAPPED.
#[cfg(target_os = "windows")]
struct NativeHidDevice {
    read_handle: NativeHandle,
    write_handle: NativeHandle,
    event: NativeHandle,
}

#[cfg(target_os = "windows")]
impl NativeHidDevice {
    fn open(read_path: &[u8], write_path: &[u8]) -> Result<Self, u32> {
        let read_handle = open_hid_handle(read_path, FILE_GENERIC_READ, FILE_FLAG_OVERLAPPED)?;
        let write_handle = open_hid_handle(write_path, FILE_GENERIC_WRITE, 0)?;

        // SAFETY: Creates a private manual-reset event for this HID reader.
        let raw_event = unsafe { CreateEventW(std::ptr::null_mut(), 1, 0, std::ptr::null()) };
        if raw_event.is_null() {
            return Err(last_error());
        }

        Ok(Self {
            read_handle,
            write_handle,
            event: NativeHandle(raw_event),
        })
    }

    fn set_feature_report(&self, report: &[u8]) -> Result<(), u32> {
        if report.is_empty() {
            return Err(87); // ERROR_INVALID_PARAMETER
        }
        let mut report_buffer = report.to_vec();

        // SAFETY: The mutable report copy remains valid for the synchronous
        // HidD_SetFeature call, which accepts a mutable buffer pointer.
        let success = unsafe {
            HidD_SetFeature(
                self.write_handle.0,
                report_buffer.as_mut_ptr() as *mut std::ffi::c_void,
                report_buffer.len() as u32,
            )
        };
        if success == 0 {
            Err(last_error())
        } else {
            Ok(())
        }
    }

    fn begin_read<'a>(&'a self, buffer: &'a mut [u8]) -> Result<PendingRead<'a>, u32> {
        if buffer.is_empty() || buffer.len() > u32::MAX as usize {
            return Err(87); // ERROR_INVALID_PARAMETER
        }

        // The original OemDrv reader seeds the raw 17-byte buffer with 0x09,
        // then passes raw[1..] to its report handler. Match and parse the
        // logical response after this transport prefix.
        buffer[0] = 0x09;

        // SAFETY: Previous reads are drained before returning to this method.
        if unsafe { ResetEvent(self.event.0) } == 0 {
            return Err(last_error());
        }

        // Box the OVERLAPPED because Windows retains its address until the
        // pending read completes or is cancelled and drained.
        let mut overlapped = Box::new(NativeOverlapped::new(self.event.0));
        // SAFETY: The borrowed buffer and boxed OVERLAPPED remain stable and
        // live in PendingRead. The byte-count pointer is NULL for async I/O.
        let started = unsafe {
            ReadFile(
                self.read_handle.0,
                buffer.as_mut_ptr() as *mut std::ffi::c_void,
                buffer.len() as u32,
                std::ptr::null_mut(),
                overlapped.as_mut(),
            )
        };
        let pending = started == 0;
        if pending && last_error() != ERROR_IO_PENDING {
            let error = last_error();
            return Err(error);
        }

        Ok(PendingRead {
            device: self,
            _buffer: buffer,
            overlapped,
            pending,
        })
    }

    fn overlapped_result(
        &self,
        overlapped: &mut NativeOverlapped,
        wait: bool,
    ) -> Result<usize, u32> {
        let mut bytes_read = 0_u32;
        // SAFETY: The read is complete or `wait` asks Windows to wait for it.
        let success = unsafe {
            GetOverlappedResult(
                self.read_handle.0,
                overlapped,
                &mut bytes_read,
                i32::from(wait),
            )
        };
        if success == 0 {
            Err(last_error())
        } else {
            Ok(bytes_read as usize)
        }
    }

    fn cancel_and_drain(&self, overlapped: &mut NativeOverlapped) {
        // SAFETY: Cancel this handle's read and wait for its completion before
        // allowing the caller's buffer and OVERLAPPED to leave scope.
        unsafe {
            CancelIoEx(self.read_handle.0, overlapped);
            let mut bytes_read = 0_u32;
            GetOverlappedResult(self.read_handle.0, overlapped, &mut bytes_read, 1);
        }
    }
}

#[cfg(target_os = "windows")]
fn open_hid_handle(path: &[u8], access: u32, flags: u32) -> Result<NativeHandle, u32> {
    let path = std::str::from_utf8(path).map_err(|_| ERROR_NO_UNICODE_TRANSLATION)?;
    let path_wide: Vec<u16> = path.encode_utf16().chain([0]).collect();
    // SAFETY: The NUL-terminated path remains alive for the CreateFileW call.
    let raw_read_handle = unsafe {
        CreateFileW(
            path_wide.as_ptr(),
            access,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            std::ptr::null_mut(),
            OPEN_EXISTING,
            flags,
            std::ptr::null_mut(),
        )
    };
    if raw_read_handle as isize == -1 {
        return Err(last_error());
    }
    Ok(NativeHandle(raw_read_handle))
}

#[cfg(target_os = "windows")]
struct PendingRead<'a> {
    device: &'a NativeHidDevice,
    _buffer: &'a mut [u8],
    overlapped: Box<NativeOverlapped>,
    pending: bool,
}

#[cfg(target_os = "windows")]
impl PendingRead<'_> {
    fn wait(mut self, timeout: Duration) -> Result<usize, u32> {
        if !self.pending {
            return self
                .device
                .overlapped_result(self.overlapped.as_mut(), false);
        }

        let timeout_ms = timeout.as_millis().clamp(1, (u32::MAX - 1) as u128) as u32;
        // SAFETY: Waits on the event associated with this outstanding read.
        match unsafe { WaitForSingleObject(self.device.event.0, timeout_ms) } {
            WAIT_OBJECT_0 => {
                self.pending = false;
                self.device
                    .overlapped_result(self.overlapped.as_mut(), false)
            }
            WAIT_TIMEOUT => {
                self.device.cancel_and_drain(self.overlapped.as_mut());
                self.pending = false;
                Ok(0)
            }
            _ => {
                let wait_error = last_error();
                self.device.cancel_and_drain(self.overlapped.as_mut());
                self.pending = false;
                Err(wait_error)
            }
        }
    }
}

#[cfg(target_os = "windows")]
impl Drop for PendingRead<'_> {
    fn drop(&mut self) {
        if self.pending {
            self.device.cancel_and_drain(self.overlapped.as_mut());
            self.pending = false;
        }
    }
}

#[cfg(target_os = "windows")]
#[link(name = "kernel32")]
unsafe extern "system" {
    fn CreateFileW(
        file_name: *const u16,
        desired_access: u32,
        share_mode: u32,
        security_attributes: *mut std::ffi::c_void,
        creation_disposition: u32,
        flags_and_attributes: u32,
        template_file: *mut std::ffi::c_void,
    ) -> *mut std::ffi::c_void;
    fn CreateEventW(
        event_attributes: *mut std::ffi::c_void,
        manual_reset: i32,
        initial_state: i32,
        name: *const u16,
    ) -> *mut std::ffi::c_void;
    fn ResetEvent(event: *mut std::ffi::c_void) -> i32;
    fn ReadFile(
        file: *mut std::ffi::c_void,
        buffer: *mut std::ffi::c_void,
        bytes_to_read: u32,
        bytes_read: *mut u32,
        overlapped: *mut NativeOverlapped,
    ) -> i32;
    fn WaitForSingleObject(handle: *mut std::ffi::c_void, milliseconds: u32) -> u32;
    fn GetOverlappedResult(
        file: *mut std::ffi::c_void,
        overlapped: *mut NativeOverlapped,
        bytes_transferred: *mut u32,
        wait: i32,
    ) -> i32;
    fn CancelIoEx(file: *mut std::ffi::c_void, overlapped: *mut NativeOverlapped) -> i32;
    fn GetLastError() -> u32;
    fn CloseHandle(handle: *mut std::ffi::c_void) -> i32;
}

#[cfg(target_os = "windows")]
#[link(name = "hid")]
unsafe extern "system" {
    fn HidD_SetFeature(
        device: *mut std::ffi::c_void,
        report: *mut std::ffi::c_void,
        report_length: u32,
    ) -> i32;
}

#[cfg(target_os = "windows")]
fn last_error() -> u32 {
    // SAFETY: GetLastError returns the current thread's last Win32 error.
    unsafe { GetLastError() }
}

#[cfg(debug_assertions)]
macro_rules! battery_debug {
    ($($argument:tt)*) => {
        eprintln!($($argument)*);
    };
}

#[cfg(not(debug_assertions))]
macro_rules! battery_debug {
    ($($argument:tt)*) => {
        if cfg!(debug_assertions) {
            eprintln!($($argument)*);
        }
    };
}

/// Enumeration state of the USB receiver. `Detected` means Windows exposes a
/// matching receiver HID device; it does not mean the mouse is online.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReceiverState {
    Scanning,
    NotFound,
    Detected,
    Unavailable,
}

/// A vendor-defined HID collection exposed by a supported receiver.
/// Serial numbers and device paths are intentionally omitted.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CollectionDiagnostic {
    pub product_id: u16,
    pub usage_page: u16,
    pub usage: u16,
    pub interface_number: i32,
}

/// A validated battery reply and the local time it was read.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BatteryReading {
    pub percent: u8,
    pub charging: bool,
    pub observed_at: SystemTime,
}

/// Battery query outcome. `Ambiguous` means multiple vendor collections
/// matched the read-only endpoint probe, so no battery command was sent.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BatteryState {
    Unavailable,
    Ambiguous,
    Available(BatteryReading),
}

/// Outcome of the receiver's read-only connection-status probe.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConnectionProbeState {
    NotChecked,
    Online,
    Offline,
    NoResponse,
    Unavailable,
}

/// Latest receiver enumeration and battery query state.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DeviceSnapshot {
    pub receiver: ReceiverState,
    pub connection_probe: ConnectionProbeState,
    pub vendor_collections: Vec<CollectionDiagnostic>,
    pub selected_collection: Option<CollectionDiagnostic>,
    pub reported_profile_id: Option<u8>,
    pub mouse_profile_switch: MouseProfileSwitchState,
    pub(crate) profile_identity_generation: u64,
    pub battery: BatteryState,
    pub dpi_and_color_apply: DpiAndColorApplyState,
    pub apply_request_id: u64,
    /// True only when the complete device write encoding is verified.
    pub dpi_and_color_writes_supported: bool,
}

impl Default for DeviceSnapshot {
    fn default() -> Self {
        Self {
            receiver: ReceiverState::Scanning,
            connection_probe: ConnectionProbeState::NotChecked,
            vendor_collections: Vec::new(),
            selected_collection: None,
            reported_profile_id: None,
            mouse_profile_switch: MouseProfileSwitchState::Idle,
            profile_identity_generation: 0,
            battery: BatteryState::Unavailable,
            dpi_and_color_apply: DpiAndColorApplyState::Idle,
            apply_request_id: 0,
            dpi_and_color_writes_supported: false,
        }
    }
}

struct VersionedSnapshot {
    revision: u64,
    snapshot: Option<DeviceSnapshot>,
}

struct MonitorState {
    latest: Mutex<VersionedSnapshot>,
    last_delivered_revision: AtomicU64,
    stop_requested: AtomicBool,
    wake_lock: Mutex<()>,
    wake: Condvar,
    apply_sender: mpsc::SyncSender<DpiAndColorSettings>,
    apply_pending: AtomicBool,
    apply_request_id: AtomicU64,
    apply_state: Mutex<DpiAndColorApplyState>,
    profile_switch_sender: mpsc::SyncSender<ProfileSwitchRequest>,
    profile_switch_pending: AtomicBool,
    profile_switch_state: Mutex<MouseProfileSwitchState>,
    profile_switch_gate: Mutex<()>,
    profile_identity_generation: AtomicU64,
}

#[derive(Clone, Copy)]
struct ProfileSwitchRequest {
    target_id: u8,
    expected_current_id: u8,
    identity_generation: u64,
}

/// Background HID discovery. The UI polls new snapshots without blocking.
pub struct DeviceMonitor {
    state: Arc<MonitorState>,
    worker: Option<JoinHandle<()>>,
}

impl DeviceMonitor {
    /// Start discovery in a worker thread and request a UI repaint after updates.
    pub fn start(ctx: eframe::egui::Context) -> Self {
        let (apply_sender, apply_receiver) = mpsc::sync_channel(1);
        let (profile_switch_sender, profile_switch_receiver) = mpsc::sync_channel(1);
        let state = Arc::new(MonitorState {
            latest: Mutex::new(VersionedSnapshot {
                revision: 0,
                snapshot: None,
            }),
            last_delivered_revision: AtomicU64::new(0),
            stop_requested: AtomicBool::new(false),
            wake_lock: Mutex::new(()),
            wake: Condvar::new(),
            apply_sender,
            apply_pending: AtomicBool::new(false),
            apply_request_id: AtomicU64::new(0),
            apply_state: Mutex::new(DpiAndColorApplyState::Idle),
            profile_switch_sender,
            profile_switch_pending: AtomicBool::new(false),
            profile_switch_state: Mutex::new(MouseProfileSwitchState::Idle),
            profile_switch_gate: Mutex::new(()),
            profile_identity_generation: AtomicU64::new(0),
        });

        let worker_state = Arc::clone(&state);
        let worker = thread::Builder::new()
            .name("daxa-hid-discovery".to_owned())
            .spawn(move || {
                discovery_loop(worker_state, ctx, apply_receiver, profile_switch_receiver)
            })
            .ok();

        if worker.is_none() {
            publish(
                &state,
                DeviceSnapshot {
                    receiver: ReceiverState::Unavailable,
                    ..DeviceSnapshot::default()
                },
            );
        }

        Self { state, worker }
    }

    /// Return a new snapshot if one is ready. This method never waits for the
    /// discovery worker; `None` means no new snapshot is ready.
    pub fn try_latest(&self) -> Option<DeviceSnapshot> {
        let latest = match self.state.latest.try_lock() {
            Ok(latest) => latest,
            Err(TryLockError::WouldBlock | TryLockError::Poisoned(_)) => return None,
        };

        if latest.revision == self.state.last_delivered_revision.load(Ordering::Relaxed) {
            return None;
        }

        let snapshot = latest.snapshot.clone()?;
        self.state
            .last_delivered_revision
            .store(latest.revision, Ordering::Relaxed);
        Some(snapshot)
    }

    /// Queue one explicit apply request. This never waits for the HID worker.
    pub fn request_dpi_and_colors(&self, settings: DpiAndColorSettings) -> bool {
        if settings.x_stages.len() != 6
            || settings
                .y_stages
                .as_ref()
                .is_some_and(|stages| stages.len() != 6)
        {
            return false;
        }
        if !self
            .state
            .latest
            .try_lock()
            .ok()
            .and_then(|latest| {
                latest
                    .snapshot
                    .as_ref()
                    .map(|s| s.dpi_and_color_writes_supported)
            })
            .unwrap_or(false)
        {
            return false;
        }
        if self
            .state
            .apply_pending
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Relaxed)
            .is_err()
        {
            return false;
        }
        match self.state.apply_sender.try_send(settings) {
            Ok(()) => {
                if let Ok(mut result) = self.state.apply_state.try_lock() {
                    *result = DpiAndColorApplyState::Pending;
                }
                let request_id = self
                    .state
                    .apply_request_id
                    .fetch_add(1, Ordering::AcqRel)
                    .wrapping_add(1);
                if let Ok(mut latest) = self.state.latest.try_lock() {
                    if let Some(snapshot) = latest.snapshot.as_mut() {
                        snapshot.dpi_and_color_apply = DpiAndColorApplyState::Pending;
                        snapshot.apply_request_id = request_id;
                        latest.revision = latest.revision.wrapping_add(1);
                    }
                }
                self.state.wake.notify_one();
                true
            }
            Err(_) => {
                self.state.apply_pending.store(false, Ordering::Release);
                false
            }
        }
    }

    /// Queue a switch to one of the mouse's four onboard profiles.
    /// This method never waits for HID I/O and never sends an EEPROM write.
    pub fn request_profile_switch(&self, profile_id: u8) -> bool {
        let Ok(_profile_switch_guard) = self.state.profile_switch_gate.try_lock() else {
            return false;
        };
        if profile_id > 3 || self.state.apply_pending.load(Ordering::Acquire) {
            return false;
        }

        let request = {
            let Ok(latest) = self.state.latest.try_lock() else {
                return false;
            };
            let Some(snapshot) = latest.snapshot.as_ref() else {
                return false;
            };
            let Some(current_id) = snapshot.reported_profile_id else {
                return false;
            };
            if snapshot.connection_probe != ConnectionProbeState::Online
                || profile_id == current_id
                || snapshot.profile_identity_generation
                    != self
                        .state
                        .profile_identity_generation
                        .load(Ordering::Acquire)
            {
                return false;
            }
            ProfileSwitchRequest {
                target_id: profile_id,
                expected_current_id: current_id,
                identity_generation: snapshot.profile_identity_generation,
            }
        };

        if self
            .state
            .profile_switch_pending
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Relaxed)
            .is_err()
        {
            return false;
        }

        let pending = MouseProfileSwitchState::Pending {
            target_id: profile_id,
        };
        let Ok(mut status) = self.state.profile_switch_state.try_lock() else {
            self.state
                .profile_switch_pending
                .store(false, Ordering::Release);
            return false;
        };
        *status = pending.clone();
        drop(status);

        match self.state.profile_switch_sender.try_send(request) {
            Ok(()) => {
                if let Ok(status) = self.state.profile_switch_state.try_lock()
                    && *status == pending
                {
                    if let Ok(mut latest) = self.state.latest.try_lock() {
                        if let Some(snapshot) = latest.snapshot.as_mut() {
                            if snapshot.profile_identity_generation == request.identity_generation {
                                snapshot.mouse_profile_switch = pending;
                                latest.revision = latest.revision.wrapping_add(1);
                            }
                        }
                    }
                }
                self.state.wake.notify_one();
                true
            }
            Err(_) => {
                self.state
                    .profile_switch_pending
                    .store(false, Ordering::Release);
                if let Ok(mut status) = self.state.profile_switch_state.try_lock() {
                    *status = MouseProfileSwitchState::Idle;
                }
                false
            }
        }
    }
}

impl Drop for DeviceMonitor {
    fn drop(&mut self) {
        match self.state.wake_lock.lock() {
            Ok(_guard) => {
                self.state.stop_requested.store(true, Ordering::Release);
                self.state.wake.notify_all();
            }
            Err(_) => {
                self.state.stop_requested.store(true, Ordering::Release);
                self.state.wake.notify_all();
            }
        }

        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

#[derive(Clone)]
struct HidCandidate {
    path: Vec<u8>,
    diagnostic: CollectionDiagnostic,
}

struct ReceiverScan {
    snapshot: DeviceSnapshot,
    identity: Vec<Vec<u8>>,
}

#[derive(Clone)]
struct SelectedEndpoint {
    /// Feature-report endpoint used to send status and telemetry requests.
    path: Vec<u8>,
    /// Input-report endpoint, which may differ from the feature endpoint.
    input_path: Vec<u8>,
    diagnostic: CollectionDiagnostic,
}

fn discovery_loop(
    state: Arc<MonitorState>,
    ctx: eframe::egui::Context,
    apply_receiver: mpsc::Receiver<DpiAndColorSettings>,
    profile_switch_receiver: mpsc::Receiver<ProfileSwitchRequest>,
) {
    #[cfg(not(target_os = "windows"))]
    let _ = &profile_switch_receiver;

    #[cfg(target_os = "windows")]
    let mut hid_api = hidapi::HidApi::new().ok();

    #[cfg(target_os = "windows")]
    let mut previous_identity: Option<Vec<Vec<u8>>> = None;
    #[cfg(target_os = "windows")]
    let mut last_status_refresh: Option<Instant> = None;
    #[cfg(target_os = "windows")]
    let mut last_connection_probe: Option<Instant> = None;
    #[cfg(target_os = "windows")]
    let mut battery_state = BatteryState::Unavailable;
    #[cfg(target_os = "windows")]
    let mut reported_profile_id = None;
    #[cfg(target_os = "windows")]
    let mut selected_collection = None;
    #[cfg(target_os = "windows")]
    let mut selected_endpoint: Option<SelectedEndpoint> = None;
    #[cfg(target_os = "windows")]
    let mut mouse_online = false;
    #[cfg(target_os = "windows")]
    let mut connection_probe = ConnectionProbeState::NotChecked;

    loop {
        if state.stop_requested.load(Ordering::Acquire) {
            break;
        }

        #[cfg(target_os = "windows")]
        let snapshot = {
            let mut scan_result = match hid_api.as_mut() {
                Some(api) => scan_receiver(api),
                None => Err(hidapi::HidError::InitializationError),
            };

            if scan_result.is_err() {
                hid_api = hidapi::HidApi::new().ok();
                scan_result = match hid_api.as_mut() {
                    Some(api) => scan_receiver(api),
                    None => Err(hidapi::HidError::InitializationError),
                };
            }

            match scan_result {
                Ok(mut scan) => {
                    let connected = scan.snapshot.receiver == ReceiverState::Detected;
                    let connection_changed =
                        connected && previous_identity.as_ref() != Some(&scan.identity);

                    if !connected {
                        if previous_identity.take().is_some() {
                            state
                                .profile_identity_generation
                                .fetch_add(1, Ordering::AcqRel);
                            reset_profile_switch(&state, &profile_switch_receiver);
                        }
                        previous_identity = None;
                        last_status_refresh = None;
                        last_connection_probe = None;
                        selected_collection = None;
                        selected_endpoint = None;
                        mouse_online = false;
                        connection_probe = ConnectionProbeState::NotChecked;
                        reported_profile_id = None;
                        battery_state = BatteryState::Unavailable;
                    } else if connection_changed {
                        state
                            .profile_identity_generation
                            .fetch_add(1, Ordering::AcqRel);
                        reset_profile_switch(&state, &profile_switch_receiver);
                        previous_identity = Some(scan.identity.clone());
                        last_status_refresh = None;
                        last_connection_probe = None;
                        selected_endpoint = None;
                        selected_collection = None;
                        mouse_online = false;
                        connection_probe = ConnectionProbeState::NotChecked;
                        reported_profile_id = None;
                    }

                    let retry_probe_due = !mouse_online
                        && last_connection_probe
                            .is_none_or(|last| last.elapsed() >= DISCOVERY_INTERVAL);
                    let online_refresh_due = mouse_online
                        && last_status_refresh
                            .is_none_or(|last| last.elapsed() >= BATTERY_INTERVAL);
                    let should_probe = connected && (retry_probe_due || online_refresh_due);
                    if should_probe {
                        last_connection_probe = Some(Instant::now());
                        let endpoint = match selected_endpoint.clone() {
                            Some(endpoint) => Ok(endpoint),
                            None => match hid_api.as_ref() {
                                Some(api) => select_endpoint(api, &scan),
                                None => Err(BatteryState::Unavailable),
                            },
                        };
                        match endpoint {
                            Ok(endpoint) => {
                                let was_mouse_online = mouse_online;
                                let previous_profile_id = reported_profile_id;
                                selected_collection = Some(endpoint.diagnostic.clone());
                                let (new_state, profile_id, probe_state) =
                                    connect_and_query_battery(&endpoint);
                                battery_state = new_state;
                                reported_profile_id = profile_id;
                                connection_probe = probe_state;
                                mouse_online = probe_state == ConnectionProbeState::Online;
                                selected_endpoint = Some(endpoint);
                                if mouse_online {
                                    if was_mouse_online
                                        && previous_profile_id.is_some()
                                        && profile_id != previous_profile_id
                                        && !state.profile_switch_pending.load(Ordering::Acquire)
                                    {
                                        state
                                            .profile_identity_generation
                                            .fetch_add(1, Ordering::AcqRel);
                                        reset_profile_switch(&state, &profile_switch_receiver);
                                    }
                                    last_status_refresh = Some(Instant::now());
                                } else {
                                    if was_mouse_online {
                                        state
                                            .profile_identity_generation
                                            .fetch_add(1, Ordering::AcqRel);
                                        reset_profile_switch(&state, &profile_switch_receiver);
                                    }
                                    last_status_refresh = None;
                                    reported_profile_id = None;
                                    battery_state = BatteryState::Unavailable;
                                }
                            }
                            Err(selection_state) => {
                                if mouse_online {
                                    state
                                        .profile_identity_generation
                                        .fetch_add(1, Ordering::AcqRel);
                                    reset_profile_switch(&state, &profile_switch_receiver);
                                }
                                battery_state = selection_state;
                                connection_probe = ConnectionProbeState::Unavailable;
                                mouse_online = false;
                                last_status_refresh = None;
                                reported_profile_id = None;
                            }
                        }
                    }

                    scan.snapshot.battery = battery_state.clone();
                    scan.snapshot.connection_probe = connection_probe;
                    scan.snapshot.selected_collection = selected_collection.clone();
                    scan.snapshot.reported_profile_id = reported_profile_id;
                    scan.snapshot.profile_identity_generation =
                        state.profile_identity_generation.load(Ordering::Acquire);
                    scan.snapshot
                }
                Err(_) => {
                    if previous_identity.take().is_some() {
                        state
                            .profile_identity_generation
                            .fetch_add(1, Ordering::AcqRel);
                        reset_profile_switch(&state, &profile_switch_receiver);
                    }
                    previous_identity = None;
                    last_status_refresh = None;
                    last_connection_probe = None;
                    selected_collection = None;
                    selected_endpoint = None;
                    mouse_online = false;
                    connection_probe = ConnectionProbeState::NotChecked;
                    reported_profile_id = None;
                    battery_state = BatteryState::Unavailable;
                    DeviceSnapshot {
                        receiver: ReceiverState::Unavailable,
                        profile_identity_generation: state
                            .profile_identity_generation
                            .load(Ordering::Acquire),
                        ..DeviceSnapshot::default()
                    }
                }
            }
        };

        #[cfg(target_os = "windows")]
        if let Ok(settings) = apply_receiver.try_recv() {
            let _ = settings;
            let result = if mouse_online && reported_profile_id.is_some() {
                DpiAndColorApplyState::Unsupported {
                    reason: DPI_AND_COLOR_WRITE_UNSUPPORTED_REASON.to_owned(),
                }
            } else {
                DpiAndColorApplyState::Failed {
                    acknowledged_chunks: 0,
                    reason: "successful connection handshake and valid profile ID required"
                        .to_owned(),
                }
            };
            if let Ok(mut state_result) = state.apply_state.lock() {
                *state_result = result;
            }
            state.apply_pending.store(false, Ordering::Release);
        }

        #[cfg(target_os = "windows")]
        if let Ok(request) = profile_switch_receiver.try_recv() {
            let generation = state.profile_identity_generation.load(Ordering::Acquire);
            let result = if request.identity_generation != generation {
                None
            } else if !mouse_online || reported_profile_id.is_none() {
                Some(Err((
                    "the mouse is no longer online; wait for status to refresh and try again"
                        .to_owned(),
                    reported_profile_id,
                )))
            } else if let Some(endpoint) = selected_endpoint.as_ref() {
                Some(switch_mouse_profile(
                    endpoint,
                    request.target_id,
                    request.expected_current_id,
                ))
            } else {
                Some(Err((
                    "the mouse profile endpoint is unavailable; wait for status to refresh and try again"
                        .to_owned(),
                    reported_profile_id,
                )))
            };

            if let Some(result) = result {
                let state_value = match result {
                    Ok(profile_id) => {
                        reported_profile_id = Some(profile_id);
                        MouseProfileSwitchState::Applied { profile_id }
                    }
                    Err((reason, observed_profile_id)) => {
                        reported_profile_id = observed_profile_id;
                        MouseProfileSwitchState::Failed {
                            target_id: request.target_id,
                            reason,
                        }
                    }
                };
                if request.identity_generation
                    == state.profile_identity_generation.load(Ordering::Acquire)
                    && let Ok(mut status) = state.profile_switch_state.lock()
                {
                    *status = state_value;
                }
            } else {
                let _profile_switch_guard = state
                    .profile_switch_gate
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                *state
                    .profile_switch_state
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner()) =
                    MouseProfileSwitchState::Idle;
                state.profile_switch_pending.store(false, Ordering::Release);
            }
            state.profile_switch_pending.store(false, Ordering::Release);
        }

        #[cfg(target_os = "windows")]
        let snapshot = {
            let mut snapshot = snapshot;
            snapshot.reported_profile_id = reported_profile_id;
            snapshot.dpi_and_color_apply = state
                .apply_state
                .lock()
                .map(|s| s.clone())
                .unwrap_or(DpiAndColorApplyState::Idle);
            snapshot.apply_request_id = state.apply_request_id.load(Ordering::Acquire);
            snapshot.mouse_profile_switch = state
                .profile_switch_state
                .lock()
                .map(|status| status.clone())
                .unwrap_or_default();
            snapshot.profile_identity_generation =
                state.profile_identity_generation.load(Ordering::Acquire);
            snapshot
        };

        #[cfg(not(target_os = "windows"))]
        let snapshot = DeviceSnapshot {
            receiver: ReceiverState::Unavailable,
            ..DeviceSnapshot::default()
        };

        if publish(&state, snapshot) {
            ctx.request_repaint();
        }

        let guard = match state.wake_lock.lock() {
            Ok(guard) => guard,
            Err(_) => break,
        };
        if state.stop_requested.load(Ordering::Acquire) {
            break;
        }
        if state.apply_pending.load(Ordering::Acquire) {
            drop(guard);
            continue;
        }
        let _ = state.wake.wait_timeout(guard, DISCOVERY_INTERVAL);
    }
}

#[cfg(target_os = "windows")]
fn scan_receiver(api: &mut hidapi::HidApi) -> hidapi::HidResult<ReceiverScan> {
    api.refresh_devices()?;

    let matching_devices: Vec<_> = api
        .device_list()
        .filter(|device| {
            device.vendor_id() == VENDOR_ID && PRODUCT_IDS.contains(&device.product_id())
        })
        .collect();

    let mut candidates: Vec<_> = matching_devices
        .iter()
        .filter(|device| device.usage_page() >= VENDOR_USAGE_PAGE_MIN)
        .map(|device| HidCandidate {
            path: device.path().to_bytes().to_vec(),
            diagnostic: CollectionDiagnostic {
                product_id: device.product_id(),
                usage_page: device.usage_page(),
                usage: device.usage(),
                interface_number: device.interface_number(),
            },
        })
        .collect();

    candidates.sort_by(|left, right| {
        (
            left.diagnostic.product_id,
            left.diagnostic.interface_number,
            left.diagnostic.usage_page,
            left.diagnostic.usage,
        )
            .cmp(&(
                right.diagnostic.product_id,
                right.diagnostic.interface_number,
                right.diagnostic.usage_page,
                right.diagnostic.usage,
            ))
    });
    candidates.dedup_by(|left, right| left.path == right.path);

    let identity = candidates
        .iter()
        .map(|candidate| candidate.path.clone())
        .collect();
    let vendor_collections = candidates
        .iter()
        .map(|candidate| candidate.diagnostic.clone())
        .collect::<Vec<_>>();

    #[cfg(debug_assertions)]
    log_collections(&vendor_collections);

    let receiver = if matching_devices.is_empty() {
        ReceiverState::NotFound
    } else {
        ReceiverState::Detected
    };

    Ok(ReceiverScan {
        snapshot: DeviceSnapshot {
            receiver,
            vendor_collections,
            selected_collection: None,
            reported_profile_id: None,
            battery: BatteryState::Unavailable,
            ..DeviceSnapshot::default()
        },
        identity,
    })
}

#[cfg(target_os = "windows")]
fn select_endpoint(
    api: &hidapi::HidApi,
    scan: &ReceiverScan,
) -> Result<SelectedEndpoint, BatteryState> {
    let mut feature_matches = Vec::new();

    for (candidate, diagnostic) in scan.identity.iter().zip(&scan.snapshot.vendor_collections) {
        let Some(info) = api
            .device_list()
            .find(|info| info.path().to_bytes() == candidate)
        else {
            battery_debug!(
                "Daxa battery probe: candidate disappeared before open (PID=0x{:04X}, usage_page=0x{:04X}, usage=0x{:04X}, interface={})",
                diagnostic.product_id,
                diagnostic.usage_page,
                diagnostic.usage,
                diagnostic.interface_number
            );
            continue;
        };

        let device = match api.open_path(info.path()) {
            Ok(device) => device,
            Err(error) => {
                battery_debug!(
                    "Daxa battery probe: open failed (PID=0x{:04X}, usage_page=0x{:04X}, usage=0x{:04X}, interface={}, error={})",
                    diagnostic.product_id,
                    diagnostic.usage_page,
                    diagnostic.usage,
                    diagnostic.interface_number,
                    safe_hid_error(&error)
                );
                continue;
            }
        };

        let mut probe = [0_u8; 17];
        probe[0] = FEATURE_REPORT_ID;
        match device.get_feature_report(&mut probe) {
            Ok(length) => {
                let report_id = (length > 0).then_some(probe[0]);
                let qualified = length == 17 && report_id == Some(FEATURE_REPORT_ID);
                battery_debug!(
                    "Daxa battery probe: PID=0x{:04X}, usage_page=0x{:04X}, usage=0x{:04X}, interface={}, hidapi=Ok, length={}, report_id={}, qualified={}",
                    diagnostic.product_id,
                    diagnostic.usage_page,
                    diagnostic.usage,
                    diagnostic.interface_number,
                    length,
                    format_report_id(report_id),
                    qualified
                );
                if qualified {
                    feature_matches.push(SelectedEndpoint {
                        path: candidate.clone(),
                        input_path: candidate.clone(),
                        diagnostic: diagnostic.clone(),
                    });
                }
            }
            Err(error) => {
                battery_debug!(
                    "Daxa battery probe: PID=0x{:04X}, usage_page=0x{:04X}, usage=0x{:04X}, interface={}, hidapi=Err, length=none, report_id=none, error={}",
                    diagnostic.product_id,
                    diagnostic.usage_page,
                    diagnostic.usage,
                    diagnostic.interface_number,
                    safe_hid_error(&error)
                );
            }
        }
    }

    match feature_matches.len() {
        0 => {
            battery_debug!(
                "Daxa battery endpoint selection: no unique endpoint (zero candidates qualified); battery unavailable"
            );
            Err(BatteryState::Unavailable)
        }
        1 => {
            let mut endpoint = feature_matches.pop().expect("one matching endpoint");
            if endpoint.diagnostic.product_id == 0xFA7B {
                let product_id = endpoint.diagnostic.product_id;
                let interface_number = endpoint.diagnostic.interface_number;
                let input_paths = scan
                    .identity
                    .iter()
                    .zip(&scan.snapshot.vendor_collections)
                    .filter(|(_, diagnostic)| {
                        diagnostic.product_id == product_id
                            && diagnostic.interface_number == interface_number
                            && diagnostic.usage_page == 0xFF01
                            && diagnostic.usage == 0
                    })
                    .map(|(path, _)| path.clone())
                    .collect::<Vec<_>>();
                if input_paths.len() == 1 {
                    endpoint.input_path = input_paths[0].clone();
                    battery_debug!(
                        "Daxa input endpoint selection: using observed wired input collection PID=0x{:04X}, usage_page=0xFF01, usage=0x0000, interface={}",
                        endpoint.diagnostic.product_id,
                        endpoint.diagnostic.interface_number
                    );
                } else if input_paths.len() > 1 {
                    battery_debug!(
                        "Daxa input endpoint selection: multiple FF01/0000 candidates for PID=0x{:04X}; falling back to feature endpoint",
                        endpoint.diagnostic.product_id
                    );
                } else {
                    battery_debug!(
                        "Daxa input endpoint selection: no FF01/0000 candidate for PID=0x{:04X}; falling back to feature endpoint",
                        endpoint.diagnostic.product_id
                    );
                }
            }
            Ok(endpoint)
        }
        count => {
            battery_debug!(
                "Daxa battery endpoint selection: ambiguous ({} candidates qualified); battery command not sent",
                count
            );
            Err(BatteryState::Ambiguous)
        }
    }
}

#[cfg(target_os = "windows")]
fn run_connection_startup_probe(device: &NativeHidDevice) {
    // OemDrv starts a raw input read before sending this zero-checksum 0x03
    // probe, then waits briefly for any input. Match that startup sequence only
    // as a wake/input probe: its response is discarded and never establishes
    // online or offline state.
    let mut raw_input = [0_u8; 17];
    let pending_read = match device.begin_read(&mut raw_input) {
        Ok(pending_read) => pending_read,
        Err(error_code) => {
            battery_debug!(
                "Daxa connection preflight: could not begin raw input read (Win32 error code={}); proceeding to checksummed status attempts",
                error_code
            );
            return;
        }
    };

    if let Err(error_code) = device.set_feature_report(&CONNECTION_STARTUP_PROBE_COMMAND) {
        drop(pending_read);
        battery_debug!(
            "Daxa connection preflight: zero-checksum 0x03 send failed (Win32 error code={}); proceeding to checksummed status attempts",
            error_code
        );
        return;
    }

    battery_debug!(
        "Daxa connection preflight: zero-checksum 0x03 probe sent; waiting up to 100 ms for raw input"
    );
    match pending_read.wait(CONNECTION_STARTUP_PROBE_TIMEOUT) {
        Ok(0) => {
            battery_debug!(
                "Daxa connection preflight: no raw input within 100 ms; proceeding to checksummed status attempts"
            );
        }
        Ok(length) => {
            battery_debug!(
                "Daxa connection preflight: received {} raw input bytes; discarded without status parsing",
                length
            );
        }
        Err(error_code) => {
            battery_debug!(
                "Daxa connection preflight: raw input read failed after send (Win32 error code={}); proceeding to checksummed status attempts",
                error_code
            );
        }
    }
}

#[cfg(target_os = "windows")]
fn connect_and_query_battery(
    endpoint: &SelectedEndpoint,
) -> (BatteryState, Option<u8>, ConnectionProbeState) {
    let device = match NativeHidDevice::open(&endpoint.input_path, &endpoint.path) {
        Ok(device) => device,
        Err(error_code) => {
            battery_debug!(
                "Daxa connection setup: open failed (Win32 error code={})",
                error_code
            );
            return (
                BatteryState::Unavailable,
                None,
                ConnectionProbeState::Unavailable,
            );
        }
    };

    run_connection_startup_probe(&device);

    let mut setup_ready = false;
    let mut setup_response_seen = false;
    let mut setup_command_sent = false;
    let mut probe_state = ConnectionProbeState::Unavailable;
    let mut total_reads = 0;

    'status_attempts: for attempt in 1..=CONNECTION_STATUS_MAX_ATTEMPTS {
        let attempt_deadline = Instant::now() + CONNECTION_STATUS_REPLY_TIMEOUT;
        let mut attempt_reads = 0;
        let mut attempt_command_sent = false;
        battery_debug!(
            "Daxa connection setup: read-only status attempt {}/{}",
            attempt,
            CONNECTION_STATUS_MAX_ATTEMPTS
        );

        while attempt_reads < CONNECTION_STATUS_MAX_READS && Instant::now() < attempt_deadline {
            attempt_reads += 1;
            total_reads += 1;
            let mut setup_reply = [0_u8; 17];
            let pending_read = match device.begin_read(&mut setup_reply) {
                Ok(pending_read) => pending_read,
                Err(error_code) => {
                    battery_debug!(
                        "Daxa connection setup: could not begin input read (Win32 error code={})",
                        error_code
                    );
                    return (
                        BatteryState::Unavailable,
                        None,
                        ConnectionProbeState::Unavailable,
                    );
                }
            };

            if !attempt_command_sent {
                if let Err(error_code) = device.set_feature_report(&CONNECTION_SETUP_COMMAND) {
                    battery_debug!(
                        "Daxa connection setup: feature-send failure on attempt {} (Win32 error code={})",
                        attempt,
                        error_code
                    );
                    return (
                        BatteryState::Unavailable,
                        None,
                        ConnectionProbeState::Unavailable,
                    );
                }
                attempt_command_sent = true;
                setup_command_sent = true;
            }

            let remaining = attempt_deadline.saturating_duration_since(Instant::now());
            match pending_read.wait(remaining) {
                Ok(length)
                    if length > INPUT_REPORT_OFFSET && setup_reply[INPUT_REPORT_OFFSET] == 0x03 =>
                {
                    setup_response_seen = true;
                    let status_index = INPUT_REPORT_OFFSET + RESPONSE_STATUS_INDEX;
                    let online_value_index = INPUT_REPORT_OFFSET + RESPONSE_FIRST_DATA_INDEX;
                    if length <= online_value_index {
                        battery_debug!(
                            "Daxa connection setup: short 0x03 response on attempt {} (raw_length={}, expected>=7 with 0x09 prefix)",
                            attempt,
                            length
                        );
                        break;
                    }

                    let status = setup_reply[status_index];
                    if status != 0 {
                        battery_debug!(
                            "Daxa connection setup: nonzero status on attempt {} (report_id=0x03, status=0x{:02X})",
                            attempt,
                            status
                        );
                        break;
                    }

                    match setup_reply[online_value_index] {
                        1 => {
                            battery_debug!(
                                "Daxa connection setup: validated online status on attempt {} (length={})",
                                attempt,
                                length
                            );
                            probe_state = ConnectionProbeState::Online;
                            setup_ready = true;
                            break 'status_attempts;
                        }
                        0 => {
                            battery_debug!(
                                "Daxa connection setup: validated offline status on attempt {} (length={})",
                                attempt,
                                length
                            );
                            probe_state = ConnectionProbeState::Offline;
                            break 'status_attempts;
                        }
                        value => {
                            battery_debug!(
                                "Daxa connection setup: unexpected online value on attempt {} (0x{:02X})",
                                attempt,
                                value
                            );
                            break;
                        }
                    }
                }
                Ok(length) if length > 0 => {
                    battery_debug!(
                        "Daxa connection setup: ignored unrelated input report on attempt {} (report_id={}, length={})",
                        attempt,
                        format_report_id(
                            setup_reply
                                .get(INPUT_REPORT_OFFSET)
                                .copied()
                                .filter(|_| length > INPUT_REPORT_OFFSET)
                        ),
                        length
                    );
                }
                Ok(_) => break,
                Err(error_code) => {
                    battery_debug!(
                        "Daxa connection setup: input read failed on attempt {} (Win32 error code={})",
                        attempt,
                        error_code
                    );
                    return (
                        BatteryState::Unavailable,
                        None,
                        ConnectionProbeState::Unavailable,
                    );
                }
            }
        }

        if attempt < CONNECTION_STATUS_MAX_ATTEMPTS {
            battery_debug!(
                "Daxa connection setup: attempt {} ended without a validated status after {} reads; sending a fresh read-only query",
                attempt,
                attempt_reads
            );
        }
    }

    if probe_state == ConnectionProbeState::Offline {
        return (BatteryState::Unavailable, None, probe_state);
    }

    if !setup_ready {
        battery_debug!(
            "Daxa connection setup: online state was not established after {} status attempts and {} input reads; profile and battery unavailable",
            CONNECTION_STATUS_MAX_ATTEMPTS,
            total_reads
        );
        if !setup_response_seen && setup_command_sent {
            probe_state = ConnectionProbeState::NoResponse;
        }
        return (BatteryState::Unavailable, None, probe_state);
    }

    (
        query_battery_on_device(&device, &endpoint.diagnostic),
        query_current_profile_on_device(&device),
        ConnectionProbeState::Online,
    )
}

#[cfg(target_os = "windows")]
fn reset_profile_switch(state: &MonitorState, receiver: &mpsc::Receiver<ProfileSwitchRequest>) {
    let _profile_switch_guard = state
        .profile_switch_gate
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    while receiver.try_recv().is_ok() {}
    state.profile_switch_pending.store(false, Ordering::Release);
    *state
        .profile_switch_state
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = MouseProfileSwitchState::Idle;
}

#[cfg(target_os = "windows")]
fn switch_mouse_profile(
    endpoint: &SelectedEndpoint,
    target_id: u8,
    expected_current_id: u8,
) -> Result<u8, (String, Option<u8>)> {
    if target_id > 3 {
        return Err(("profile ID must be between 0 and 3".to_owned(), None));
    }

    let device = NativeHidDevice::open(&endpoint.input_path, &endpoint.path).map_err(|code| {
        (
            format!("could not open the mouse profile endpoint (Windows error {code})"),
            None,
        )
    })?;

    let Some(current_id) = query_current_profile_on_device(&device) else {
        return Err((
            "could not verify the current mouse profile before switching".to_owned(),
            None,
        ));
    };
    if current_id != expected_current_id {
        return Err((
            format!(
                "the mouse profile changed from ID {expected_current_id} to ID {current_id}; refresh status and try again"
            ),
            Some(current_id),
        ));
    }
    if current_id == target_id {
        return Ok(current_id);
    }

    let ack_result = send_profile_switch(&device, target_id);
    let observed_id = query_current_profile_on_device(&device);
    match (ack_result, observed_id) {
        (Ok(()), Some(profile_id)) if profile_id == target_id => Ok(profile_id),
        (Ok(()), Some(profile_id)) => Err((
            format!(
                "profile-select ACK was valid, but the mouse reports profile ID {profile_id}, expected {target_id}"
            ),
            Some(profile_id),
        )),
        (Ok(()), None) => Err((
            "profile-select ACK was valid, but the follow-up profile query did not return a valid ID"
                .to_owned(),
            None,
        )),
        (Err(error), Some(profile_id)) => Err((
            format!(
                "profile-select ACK was not validated ({error}); the mouse currently reports profile ID {profile_id}"
            ),
            Some(profile_id),
        )),
        (Err(error), None) => Err((
            format!(
                "profile-select ACK was not validated ({error}), and the follow-up profile query did not return a valid ID"
            ),
            None,
        )),
    }
}

#[cfg(target_os = "windows")]
fn send_profile_switch(device: &NativeHidDevice, profile_id: u8) -> Result<(), String> {
    let mut request = [0_u8; 17];
    request[0] = 0x08;
    request[1] = 0x10;
    request[5] = 0x01;
    request[6] = profile_id;
    request[16] = 0x3C_u8.wrapping_sub(profile_id);

    let deadline = Instant::now() + PROFILE_SWITCH_REPLY_TIMEOUT;
    let mut request_sent = false;
    while Instant::now() < deadline {
        let mut report = [0_u8; 17];
        let pending_read = device.begin_read(&mut report).map_err(|code| {
            format!("could not begin the profile ACK read (Windows error {code})")
        })?;

        if !request_sent {
            device
                .set_feature_report(&request)
                .map_err(|code| format!("profile-select request failed (Windows error {code})"))?;
            request_sent = true;
        }

        let remaining = deadline.saturating_duration_since(Instant::now());
        match pending_read.wait(remaining) {
            Ok(length)
                if length > INPUT_REPORT_OFFSET
                    && report[INPUT_REPORT_OFFSET] == PROFILE_SWITCH_INPUT_REPORT_ID =>
            {
                let status_index = INPUT_REPORT_OFFSET + RESPONSE_STATUS_INDEX;
                if length <= status_index {
                    return Err(format!(
                        "profile-select ACK was too short (length {length}, expected at least {})",
                        status_index + 1
                    ));
                }
                let status = report[status_index];
                if status == 0 {
                    return Ok(());
                }
                return Err(format!("profile-select ACK returned status 0x{status:02X}"));
            }
            Ok(0) => break,
            Ok(_) => {}
            Err(code) => {
                return Err(format!("profile ACK read failed (Windows error {code})"));
            }
        }
    }

    if request_sent {
        Err("no profile-select ACK arrived within 1000 ms".to_owned())
    } else {
        Err("profile-select request was not sent before the reply deadline".to_owned())
    }
}

#[cfg(target_os = "windows")]
fn query_current_profile_on_device(device: &NativeHidDevice) -> Option<u8> {
    let deadline = Instant::now() + CURRENT_PROFILE_REPLY_TIMEOUT;
    let mut request_sent = false;

    while Instant::now() < deadline {
        let mut report = [0_u8; 17];
        let pending_read = match device.begin_read(&mut report) {
            Ok(pending_read) => pending_read,
            Err(_error_code) => {
                battery_debug!(
                    "Daxa current profile reply: could not begin input read (Win32 error code={})",
                    _error_code
                );
                return None;
            }
        };

        if !request_sent {
            if let Err(_error_code) = device.set_feature_report(&CURRENT_PROFILE_COMMAND) {
                battery_debug!(
                    "Daxa current profile request: feature-send failure (Win32 error code={})",
                    _error_code
                );
                return None;
            }
            request_sent = true;
        }

        let remaining = deadline.saturating_duration_since(Instant::now());
        match pending_read.wait(remaining) {
            Ok(0) => break,
            Ok(length)
                if length > INPUT_REPORT_OFFSET
                    && report[INPUT_REPORT_OFFSET] == CURRENT_PROFILE_INPUT_REPORT_ID =>
            {
                let status_index = INPUT_REPORT_OFFSET + RESPONSE_STATUS_INDEX;
                let profile_value_index = INPUT_REPORT_OFFSET + RESPONSE_FIRST_DATA_INDEX;
                if length <= profile_value_index {
                    battery_debug!(
                        "Daxa current profile reply: length failure (raw_length={}, expected>=7 with 0x09 prefix)",
                        length
                    );
                    return None;
                }

                let status = report[status_index];
                if status != 0 {
                    battery_debug!(
                        "Daxa current profile reply: status failure (report_id=0x0E, status=0x{:02X})",
                        status
                    );
                    return None;
                }

                let profile_id = report[profile_value_index];
                if profile_id > 3 {
                    battery_debug!(
                        "Daxa current profile reply: profile ID out of range (id={})",
                        profile_id
                    );
                    return None;
                }

                battery_debug!(
                    "Daxa current profile reply: validated profile ID={}",
                    profile_id
                );
                return Some(profile_id);
            }
            Ok(length) if length > 0 => {
                battery_debug!(
                    "Daxa current profile reply: ignored unrelated input report (report_id={}, length={})",
                    format_report_id(
                        report
                            .get(INPUT_REPORT_OFFSET)
                            .copied()
                            .filter(|_| length > INPUT_REPORT_OFFSET)
                    ),
                    length
                );
            }
            Ok(_) => break,
            Err(_error_code) => {
                battery_debug!(
                    "Daxa current profile reply: read failure before timeout (Win32 error code={})",
                    _error_code
                );
                return None;
            }
        }
    }

    battery_debug!("Daxa current profile reply: timeout or no matching report");
    None
}

#[cfg(target_os = "windows")]
fn query_battery_on_device(
    device: &NativeHidDevice,
    selected: &CollectionDiagnostic,
) -> BatteryState {
    let deadline = Instant::now() + BATTERY_REPLY_TIMEOUT;
    let mut last_report: Option<(usize, Option<u8>)> = None;
    let mut ignored_reports = 0_u32;
    let mut request_sent = false;
    while Instant::now() < deadline {
        let mut report = [0_u8; 17];
        let pending_read = match device.begin_read(&mut report) {
            Ok(pending_read) => pending_read,
            Err(error_code) => {
                battery_debug!(
                    "Daxa battery reply: could not begin input read (Win32 error code={})",
                    error_code
                );
                return BatteryState::Unavailable;
            }
        };

        if !request_sent {
            if let Err(error_code) = device.set_feature_report(&BATTERY_COMMAND) {
                battery_debug!(
                    "Daxa battery request: feature-send failure on PID=0x{:04X}, usage_page=0x{:04X}, usage=0x{:04X}, interface={}, Win32 error code={}",
                    selected.product_id,
                    selected.usage_page,
                    selected.usage,
                    selected.interface_number,
                    error_code
                );
                return BatteryState::Unavailable;
            }
            request_sent = true;
        }

        let remaining = deadline.saturating_duration_since(Instant::now());
        match pending_read.wait(remaining) {
            Ok(0) => break,
            Ok(length) => {
                let report_id = report
                    .get(INPUT_REPORT_OFFSET)
                    .copied()
                    .filter(|_| length > INPUT_REPORT_OFFSET);
                last_report = Some((length, report_id));
                if report_id != Some(BATTERY_INPUT_REPORT_ID) {
                    ignored_reports = ignored_reports.saturating_add(1);
                    continue;
                }

                if length <= INPUT_REPORT_OFFSET + BATTERY_CHARGING_INDEX {
                    battery_debug!(
                        "Daxa battery reply: length failure (report_id=0x04, raw_length={}, expected>=8 with 0x09 prefix)",
                        length
                    );
                    return BatteryState::Unavailable;
                }

                let status = report[INPUT_REPORT_OFFSET + BATTERY_STATUS_INDEX];
                if status != 0 {
                    battery_debug!(
                        "Daxa battery reply: status failure (report_id=0x04, status=0x{:02X})",
                        status
                    );
                    return BatteryState::Unavailable;
                }

                let percent = report[INPUT_REPORT_OFFSET + BATTERY_PERCENT_INDEX];
                let charging = report[INPUT_REPORT_OFFSET + BATTERY_CHARGING_INDEX];
                if percent > 100 || charging > 1 {
                    battery_debug!(
                        "Daxa battery reply: validation failure (report_id=0x04, length={}, percent/charging out of range)",
                        length
                    );
                    return BatteryState::Unavailable;
                }

                battery_debug!(
                    "Daxa battery reply: validated percent={}, charging={}",
                    percent,
                    if charging == 1 { "yes" } else { "no" }
                );
                return BatteryState::Available(BatteryReading {
                    percent,
                    charging: charging == 1,
                    observed_at: SystemTime::now(),
                });
            }
            Err(error_code) => {
                battery_debug!(
                    "Daxa battery reply: read failure before timeout (Win32 error code={})",
                    error_code
                );
                return BatteryState::Unavailable;
            }
        }
    }

    if let Some((length, report_id)) = last_report {
        battery_debug!(
            "Daxa battery reply: timeout after ignoring {} reports; last report_id={}, length={}",
            ignored_reports,
            format_report_id(report_id),
            length
        );
    } else {
        battery_debug!("Daxa battery reply: timeout (no input report received)");
    }

    BatteryState::Unavailable
}

#[cfg(target_os = "windows")]
#[allow(dead_code)] // Used only by debug diagnostics; release logging is folded out.
fn format_report_id(report_id: Option<u8>) -> String {
    report_id.map_or_else(|| "none".to_owned(), |id| format!("0x{id:02X}"))
}

#[cfg(target_os = "windows")]
#[allow(dead_code)] // Used only by debug diagnostics; release logging is folded out.
fn safe_hid_error(error: &hidapi::HidError) -> String {
    use hidapi::HidError;

    match error {
        HidError::HidApiError { .. } => "HIDAPI error (details omitted)".to_owned(),
        HidError::HidApiErrorEmpty => "HIDAPI error (no details provided)".to_owned(),
        HidError::FromWideCharError { .. } => "device string conversion failure".to_owned(),
        HidError::InitializationError => "HIDAPI initialization failure".to_owned(),
        HidError::InvalidZeroSizeData => "zero-length HID data".to_owned(),
        HidError::IncompleteSendError { sent, all } => {
            format!("incomplete HID send ({sent}/{all} bytes)")
        }
        HidError::SetBlockingModeError { mode } => {
            format!("blocking-mode change failed ({mode})")
        }
        HidError::OpenHidDeviceWithDeviceInfoError { .. } => {
            "device open failure (metadata omitted)".to_owned()
        }
        HidError::IoError { error } => format!(
            "I/O error kind={:?}, OS code={:?}",
            error.kind(),
            error.raw_os_error()
        ),
    }
}

#[cfg(all(target_os = "windows", debug_assertions))]
fn log_collections(collections: &[CollectionDiagnostic]) {
    use std::sync::OnceLock;

    static LAST_LOGGED: OnceLock<Mutex<Vec<CollectionDiagnostic>>> = OnceLock::new();
    let last_logged = LAST_LOGGED.get_or_init(|| Mutex::new(Vec::new()));

    if let Ok(mut last) = last_logged.lock() {
        if *last == collections {
            return;
        }

        if collections.is_empty() {
            eprintln!("Daxa HID discovery: no vendor-defined receiver collections found");
        } else {
            for collection in collections {
                eprintln!(
                    "Daxa HID candidate: PID=0x{:04X}, usage_page=0x{:04X}, usage=0x{:04X}, interface={}",
                    collection.product_id,
                    collection.usage_page,
                    collection.usage,
                    collection.interface_number
                );
            }
        }

        *last = collections.to_vec();
    }
}

fn publish(state: &MonitorState, snapshot: DeviceSnapshot) -> bool {
    let mut latest = match state.latest.lock() {
        Ok(latest) => latest,
        Err(_) => return false,
    };

    if latest.snapshot.as_ref() == Some(&snapshot) {
        return false;
    }

    latest.revision = latest.revision.wrapping_add(1);
    latest.snapshot = Some(snapshot);
    true
}
