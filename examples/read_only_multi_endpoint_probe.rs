//! Console-only endpoint diagnostic. Exit Daxa Control first. It opens input
//! reads on every vendor collection for one PID, sends one checksummed read-only
//! `0x03` status request through the unique `0x08` feature-report endpoint.
//! By default it reads only polling bytes `0x0000..0x0001`, DPI bytes
//! `0x000C..0x002B`, and DPI-color bytes `0x002C..0x004B`. Opt-in modes include
//! a guarded same-value write, a profile-scope check, and `--dpi-roundtrip-test`,
//! which changes only stage 6 at `0x001C` and restores the exact original bytes.
//! `--profile-scope-check` temporarily selects profile 1, reads only the
//! six-stage DPI range, and attempts to restore profile 0; it sends no EEPROM
//! writes and changes no DPI or button values.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DiagnosticMode {
    ReadOnly,
    SameValueDpiTest,
    ProfileScopeCheck,
    DpiRoundtripTest,
}

#[cfg(windows)]
mod win {
    use std::{
        collections::BTreeSet,
        ffi::c_void,
        time::{Duration, Instant},
    };

    use super::DiagnosticMode;
    use hidapi::HidApi;

    const VID: u16 = 0x25A7;
    const PIDS: [u16; 2] = [0xFA7B, 0xFA7C];
    const PAGE_MIN: u16 = 0xFF00;
    const REPORT_ID: u8 = 0x08;
    const PREFIX: u8 = 0x09;
    const STATUS: u8 = 0x03;
    const STATUS_REQUEST: [u8; 17] = [0x08, 0x03, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x4A];
    const PROFILE_REQUEST: [u8; 17] = [0x08, 0x0E, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x3F];
    const WIRED_DIAGNOSTIC_PID: u16 = 0xFA7B;
    const WIRED_EXPECTED_COLLECTIONS: usize = 4;
    const PROFILE_SCOPE_DPI_READS: [(u16, u8); 3] = [(0x000C, 8), (0x0014, 8), (0x001C, 8)];
    const PROFILE_RESTORE_ATTEMPTS: usize = 3;
    const DPI_ROUNDTRIP_ADDRESS: u16 = 0x001C;
    const DPI_ROUNDTRIP_BASELINE: [u8; 8] = [0x3F, 0x3F, 0x00, 0xD7, 0x7F, 0x7F, 0x00, 0x57];
    const DPI_ROUNDTRIP_CHANGED: [u8; 8] = [0x3F, 0x3F, 0x00, 0xD7, 0x7D, 0x7D, 0x00, 0x5B];
    const DPI_RESTORE_ATTEMPTS: usize = 3;
    const BATTERY_REQUEST: [u8; 17] = [0x08, 0x04, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x49];
    const DPI_EEPROM_READS: [(u16, u8); 9] = [
        (0x000C, 8),
        (0x0000, 2),
        (0x0014, 8),
        (0x001C, 8),
        (0x0024, 8),
        (0x002C, 8),
        (0x0034, 8),
        (0x003C, 8),
        (0x0044, 8),
    ];
    const SAME_VALUE_DPI_BYTES: [u8; 8] = [0x1F, 0x1F, 0x00, 0x17, 0x1F, 0x1F, 0x00, 0x17];
    const TIMEOUT: Duration = Duration::from_millis(1_050);
    const MAX_COLLECTIONS: usize = 8;
    const READ: u32 = 0x0012_0089;
    const WRITE: u32 = 0x0012_0116;
    const SHARE: u32 = 3;
    const OVERLAPPED: u32 = 0x4000_0000;
    const OPEN_EXISTING: u32 = 3;
    const IO_PENDING: u32 = 997;
    const WAIT_OBJECT: u32 = 0;
    const WAIT_TIMEOUT: u32 = 258;
    const WAIT_FAILED: u32 = u32::MAX;

    type Handle = *mut c_void;

    #[repr(C)]
    struct Overlapped {
        internal: usize,
        internal_high: usize,
        offset: u32,
        offset_high: u32,
        event: Handle,
    }

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn CreateFileW(
            name: *const u16,
            access: u32,
            share: u32,
            security: *mut c_void,
            disposition: u32,
            flags: u32,
            template: Handle,
        ) -> Handle;
        fn CreateEventW(
            attributes: *mut c_void,
            manual_reset: i32,
            initial: i32,
            name: *const u16,
        ) -> Handle;
        fn CloseHandle(handle: Handle) -> i32;
        fn GetLastError() -> u32;
        fn ResetEvent(event: Handle) -> i32;
        fn ReadFile(
            file: Handle,
            buffer: *mut c_void,
            length: u32,
            read: *mut u32,
            overlapped: *mut Overlapped,
        ) -> i32;
        fn WaitForMultipleObjects(
            count: u32,
            handles: *const Handle,
            wait_all: i32,
            milliseconds: u32,
        ) -> u32;
        fn GetOverlappedResult(
            file: Handle,
            overlapped: *mut Overlapped,
            transferred: *mut u32,
            wait: i32,
        ) -> i32;
        fn CancelIoEx(file: Handle, overlapped: *mut Overlapped) -> i32;
    }

    #[link(name = "hid")]
    unsafe extern "system" {
        fn HidD_SetFeature(device: Handle, report: *mut c_void, length: u32) -> i32;
    }

    struct Owned(Handle);

    impl Owned {
        fn open(path: &[u8], access: u32, flags: u32) -> Result<Self, u32> {
            let path = std::str::from_utf8(path).map_err(|_| 1113_u32)?;
            let wide: Vec<u16> = path.encode_utf16().chain([0]).collect();
            // SAFETY: The NUL-terminated path is alive for the call.
            let handle = unsafe {
                CreateFileW(
                    wide.as_ptr(),
                    access,
                    SHARE,
                    std::ptr::null_mut(),
                    OPEN_EXISTING,
                    flags,
                    std::ptr::null_mut(),
                )
            };
            if handle as isize == -1 {
                Err(error())
            } else {
                Ok(Self(handle))
            }
        }

        fn event() -> Result<Self, u32> {
            // SAFETY: Creates a private unnamed manual-reset event.
            let handle = unsafe { CreateEventW(std::ptr::null_mut(), 1, 0, std::ptr::null()) };
            if handle.is_null() {
                Err(error())
            } else {
                Ok(Self(handle))
            }
        }
    }

    impl Drop for Owned {
        fn drop(&mut self) {
            if !self.0.is_null() && self.0 as isize != -1 {
                // SAFETY: This wrapper owns the handle.
                unsafe { CloseHandle(self.0) };
            }
        }
    }

    #[derive(Clone)]
    struct Candidate {
        path: Vec<u8>,
        pid: u16,
        page: u16,
        usage: u16,
        interface: i32,
        feature: bool,
    }

    impl Candidate {
        fn label(&self) -> String {
            format!(
                "PID=0x{:04X} page=0x{:04X} usage=0x{:04X} interface={}",
                self.pid, self.page, self.usage, self.interface
            )
        }
    }

    struct Reader {
        candidate: Candidate,
        file: Owned,
        event: Owned,
        overlapped: Box<Overlapped>,
        buffer: [u8; 17],
        pending: bool,
    }

    impl Reader {
        fn open(candidate: Candidate) -> Result<Self, u32> {
            let file = Owned::open(&candidate.path, READ, OVERLAPPED)?;
            let event = Owned::event()?;
            let overlapped = Box::new(Overlapped {
                internal: 0,
                internal_high: 0,
                offset: 0,
                offset_high: 0,
                event: event.0,
            });
            Ok(Self {
                candidate,
                file,
                event,
                overlapped,
                buffer: [0; 17],
                pending: false,
            })
        }

        // Re-arm after at most eight inline completions, so noisy input cannot
        // spin this diagnostic indefinitely before its single query.
        fn arm(&mut self) -> Result<bool, u32> {
            for _ in 0..8 {
                self.buffer.fill(0);
                self.buffer[0] = PREFIX;
                // SAFETY: Resets this reader's event before the next operation.
                if unsafe { ResetEvent(self.event.0) } == 0 {
                    return Err(error());
                }
                *self.overlapped = Overlapped {
                    internal: 0,
                    internal_high: 0,
                    offset: 0,
                    offset_high: 0,
                    event: self.event.0,
                };
                // SAFETY: The boxed OVERLAPPED and reader buffer remain stable
                // until the operation completes or cancel-and-drain runs.
                let started = unsafe {
                    ReadFile(
                        self.file.0,
                        self.buffer.as_mut_ptr().cast(),
                        17,
                        std::ptr::null_mut(),
                        self.overlapped.as_mut(),
                    )
                };
                if started == 0 {
                    let code = error();
                    if code == IO_PENDING {
                        self.pending = true;
                        return Ok(true);
                    }
                    return Err(code);
                }
                // Ignore reports already queued before the status query and
                // immediately re-arm this collection.
                let _ = self.take()?;
            }
            // SAFETY: No read remains pending after eight inline completions.
            unsafe {
                ResetEvent(self.event.0);
            }
            Ok(false)
        }

        fn take(&mut self) -> Result<usize, u32> {
            let mut read = 0;
            // SAFETY: Called after synchronous completion or event signal.
            let ok =
                unsafe { GetOverlappedResult(self.file.0, self.overlapped.as_mut(), &mut read, 0) };
            self.pending = false;
            if ok == 0 {
                Err(error())
            } else {
                Ok(read as usize)
            }
        }

        fn cancel(&mut self) {
            if self.pending {
                // SAFETY: Drain the outstanding read before dropping its buffer.
                unsafe {
                    CancelIoEx(self.file.0, self.overlapped.as_mut());
                    let mut read = 0;
                    GetOverlappedResult(self.file.0, self.overlapped.as_mut(), &mut read, 1);
                }
                self.pending = false;
            }
        }
    }

    impl Drop for Reader {
        fn drop(&mut self) {
            self.cancel();
        }
    }

    pub fn run(mode: DiagnosticMode) -> Result<(), String> {
        println!("Daxa multi-collection status and telemetry diagnostic");
        match mode {
            DiagnosticMode::ReadOnly => println!("Read-only mode. No settings write is sent."),
            DiagnosticMode::SameValueDpiTest => {
                println!("Opt-in same-value DPI test; only the guarded 0x000C write can be sent.");
            }
            DiagnosticMode::ProfileScopeCheck => {
                println!(
                    "Opt-in profile scope check: active profile may change briefly from ID 0 to ID 1 and back."
                );
                println!(
                    "This mode sends no EEPROM writes and does not change DPI or button values."
                );
            }
            DiagnosticMode::DpiRoundtripTest => {
                println!(
                    "Opt-in DPI roundtrip at address 0x001C only: stage 6 changes 6400 -> 6300, then the original bytes are restored."
                );
                println!(
                    "The normal path remains on profile ID 0; profile 0 is reselected only for recovery."
                );
                println!("Leave the mouse's profile controls alone during this short test.");
                println!("No other EEPROM address is read or written by the roundtrip path.");
            }
        }
        println!("Exit Daxa Control first.\n");
        let api = HidApi::new().map_err(|e| format!("HID initialization failed: {e}"))?;
        let mut candidates = Vec::new();
        for info in api.device_list().filter(|d| {
            d.vendor_id() == VID && PIDS.contains(&d.product_id()) && d.usage_page() >= PAGE_MIN
        }) {
            let mut report = [0; 17];
            report[0] = REPORT_ID;
            let feature = api
                .open_path(info.path())
                .and_then(|device| device.get_feature_report(&mut report))
                .is_ok_and(|n| n == 17 && report[0] == REPORT_ID);
            candidates.push(Candidate {
                path: info.path().to_bytes().to_vec(),
                pid: info.product_id(),
                page: info.usage_page(),
                usage: info.usage(),
                interface: info.interface_number(),
                feature,
            });
        }
        candidates.sort_by_key(|c| (c.pid, c.interface, c.page, c.usage));
        candidates.dedup_by(|a, b| a.path == b.path);
        if candidates.is_empty() {
            println!("No matching HID collections.");
            return Ok(());
        }
        if candidates.len() > MAX_COLLECTIONS {
            return Err(format!(
                "Found {} collections; limit is {MAX_COLLECTIONS}.",
                candidates.len()
            ));
        }
        for c in &candidates {
            println!("{} feature_report_0x08={}", c.label(), c.feature);
        }
        let feature: Vec<_> = candidates.iter().filter(|c| c.feature).collect();
        let pids: BTreeSet<_> = candidates.iter().map(|c| c.pid).collect();
        if feature.len() != 1 || pids.len() != 1 {
            return Err(format!(
                "Expected one PID and one feature endpoint (found {} feature endpoints, {} PIDs); no status request sent.",
                feature.len(),
                pids.len()
            ));
        }
        let pid = feature[0].pid;
        if requires_wired_pid(mode) && pid != WIRED_DIAGNOSTIC_PID {
            return Err(format!(
                "This diagnostic mode requires wired PID 0x{WIRED_DIAGNOSTIC_PID:04X}; found PID 0x{pid:04X}. No profile-select or EEPROM write sent."
            ));
        }
        let write_path = feature[0].path.clone();
        let input_candidates: Vec<_> = candidates.into_iter().filter(|c| c.pid == pid).collect();
        let expected_readers = input_candidates.len();
        if requires_all_input_collections(mode) && expected_readers != WIRED_EXPECTED_COLLECTIONS {
            return Err(format!(
                "This diagnostic mode requires all {WIRED_EXPECTED_COLLECTIONS} wired input collections; found {expected_readers}. No profile-select or EEPROM write sent."
            ));
        }
        let mut readers = Vec::new();
        for candidate in input_candidates {
            match Reader::open(candidate.clone()) {
                Ok(reader) => readers.push(reader),
                Err(code) => println!("{} open failed (Win32 {code}).", candidate.label()),
            }
        }
        if requires_all_input_collections(mode) && readers.len() != expected_readers {
            return Err(format!(
                "This diagnostic mode requires all {expected_readers} input collections; only {} opened. No profile-select or EEPROM write sent.",
                readers.len()
            ));
        }
        if readers.is_empty() {
            return Err("No pending input reader; no status request sent.".into());
        }

        // Finish growing the Vec before starting reads. Each overlapped read
        // retains a pointer into its Reader, so moving the Vec afterward would
        // invalidate the borrowed input buffer.
        for reader in readers.iter_mut() {
            match reader.arm() {
                Ok(true) => {}
                Ok(false) => println!(
                    "{} produced repeated inline input; skipped.",
                    reader.candidate.label()
                ),
                Err(code) => println!(
                    "{} input read failed (Win32 {code}).",
                    reader.candidate.label()
                ),
            }
        }
        if !readers.iter().any(|reader| reader.pending) {
            return Err("No pending input reader; no status request sent.".into());
        }

        let writer = Owned::open(&write_path, WRITE, 0)
            .map_err(|code| format!("Feature writer open failed (Win32 {code})."))?;
        let mut request = STATUS_REQUEST;
        // SAFETY: This is the original checksummed status query, not a settings write.
        if unsafe { HidD_SetFeature(writer.0, request.as_mut_ptr().cast(), request.len() as u32) }
            == 0
        {
            return Err(format!("Status query failed (Win32 {}).", error()));
        }
        println!(
            "Sent one read-only 0x03 request; listening on {} input collections.",
            readers.len()
        );

        let deadline = Instant::now() + TIMEOUT;
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                break;
            }
            let events: Vec<_> = readers.iter().map(|r| r.event.0).collect();
            let ms = remaining.as_millis().clamp(1, (u32::MAX - 1) as u128) as u32;
            // SAFETY: `events` contains one valid event per stable reader.
            let result =
                unsafe { WaitForMultipleObjects(events.len() as u32, events.as_ptr(), 0, ms) };
            if result == WAIT_TIMEOUT {
                break;
            }
            if result == WAIT_FAILED {
                return Err(format!("Wait failed (Win32 {}).", error()));
            }
            let index = (result - WAIT_OBJECT).min(u32::MAX) as usize;
            if index >= readers.len() {
                return Err(format!("Unexpected wait result 0x{result:08X}."));
            }
            let reader = &mut readers[index];
            let length = reader
                .take()
                .map_err(|code| format!("Input completion failed (Win32 {code})."))?;
            if length > 1 && reader.buffer[0] == PREFIX && reader.buffer[1] == STATUS {
                let bytes = reader.buffer[..length.min(17)]
                    .iter()
                    .map(|byte| format!("{byte:02X}"))
                    .collect::<Vec<_>>()
                    .join(" ");
                if length >= 7 && reader.buffer[2] == 0 && matches!(reader.buffer[6], 0 | 1) {
                    let online = reader.buffer[6] == 1;
                    println!(
                        "Validated 0x03 status on {} (online={}, {} bytes): {bytes}",
                        reader.candidate.label(),
                        online,
                        length
                    );
                    if online {
                        query_profile_and_battery(&mut readers, &writer, expected_readers, mode)?;
                    } else if requires_wired_pid(mode) {
                        return Err("This diagnostic mode requires a validated online mouse. No profile-select or EEPROM write sent.".to_owned());
                    }
                    return Ok(());
                }
                println!(
                    "Matching 0x09/0x03 header, but unvalidated on {} ({} bytes): {bytes}",
                    reader.candidate.label(),
                    length
                );
            }
            if length > 0 {
                println!(
                    "Unrelated input on {} ({} bytes).",
                    reader.candidate.label(),
                    length
                );
            }
            let _ = reader.arm();
        }
        println!("No matching 0x03 response within 1050 ms.");
        if requires_wired_pid(mode) {
            return Err("This diagnostic mode stopped because online status was not validated. No profile-select or EEPROM write sent.".to_owned());
        }
        Ok(())
    }

    fn requires_wired_pid(mode: DiagnosticMode) -> bool {
        matches!(
            mode,
            DiagnosticMode::ProfileScopeCheck | DiagnosticMode::DpiRoundtripTest
        )
    }

    fn requires_all_input_collections(mode: DiagnosticMode) -> bool {
        requires_wired_pid(mode)
    }

    fn query_profile_and_battery(
        readers: &mut [Reader],
        writer: &Owned,
        expected: usize,
        mode: DiagnosticMode,
    ) -> Result<(), String> {
        if readers.len() != expected {
            println!(
                "Skipped profile and battery queries: only {} of {} input collections opened.",
                readers.len(),
                expected
            );
            return if requires_all_input_collections(mode) {
                Err("This diagnostic mode stopped before selection because not all input collections were available.".to_owned())
            } else {
                Ok(())
            };
        }

        let profile_id = match request_reply(
            readers,
            writer,
            &PROFILE_REQUEST,
            0x0E,
            "current profile",
        ) {
            Ok(Some((index, report, length)))
                if length >= 7 && report[2] == 0 && report[6] <= 3 =>
            {
                println!(
                    "Current profile: {} (validated on {}).",
                    report[6],
                    readers[index].candidate.label()
                );
                Some(report[6])
            }
            Ok(Some((index, report, length))) => {
                println!(
                    "Current profile reply from {} was invalid (length={length}, status=0x{:02X}, value={}).",
                    readers[index].candidate.label(),
                    report[2],
                    report[6]
                );
                None
            }
            Ok(None) => {
                println!("Current profile: no reply within 1000 ms.");
                None
            }
            Err(error) => {
                println!("Current profile query failed: {error}");
                None
            }
        };

        if let Some(profile_id) = profile_id {
            if mode == DiagnosticMode::ProfileScopeCheck {
                if profile_id == 0 {
                    run_profile_scope_check(readers, writer)?;
                } else {
                    return Err(format!(
                        "Profile scope check requires current profile ID 0, got {profile_id}. No profile-select request sent."
                    ));
                }
            } else if mode == DiagnosticMode::DpiRoundtripTest {
                if profile_id == 0 {
                    dpi_roundtrip_test(readers, writer)?;
                } else {
                    return Err(format!(
                        "DPI roundtrip requires current profile ID 0, got {profile_id}. No EEPROM write sent."
                    ));
                }
            } else if mode == DiagnosticMode::SameValueDpiTest {
                if profile_id == 0 {
                    same_value_dpi_write_test(readers, writer);
                } else {
                    println!(
                        "Skipped same-value DPI write test: reported profile ID must be 0, got {profile_id}."
                    );
                }
            } else {
                read_dpi_eeprom(readers, writer, profile_id);
            }
        } else if requires_wired_pid(mode) {
            return Err("This diagnostic mode stopped because the current profile ID was not validated. No profile-select or EEPROM write sent.".to_owned());
        } else {
            println!("Skipped DPI EEPROM read because profile ID was not validated.");
        }

        if matches!(
            mode,
            DiagnosticMode::ProfileScopeCheck | DiagnosticMode::DpiRoundtripTest
        ) {
            return Ok(());
        }

        match request_reply(readers, writer, &BATTERY_REQUEST, 0x04, "battery") {
            Ok(Some((index, report, length)))
                if length >= 8 && report[2] == 0 && report[6] <= 100 && report[7] <= 1 =>
            {
                println!(
                    "Battery: {}%{} (validated on {}).",
                    report[6],
                    if report[7] == 1 { " · charging" } else { "" },
                    readers[index].candidate.label()
                );
            }
            Ok(Some((index, report, length))) => println!(
                "Battery reply from {} was invalid (length={length}, status=0x{:02X}, percent={}, charging={}).",
                readers[index].candidate.label(),
                report[2],
                report[6],
                report[7]
            ),
            Ok(None) => println!("Battery: no reply within 1000 ms."),
            Err(error) => println!("Battery query failed: {error}"),
        }
        Ok(())
    }

    fn run_profile_scope_check(readers: &mut [Reader], writer: &Owned) -> Result<(), String> {
        let profile_zero = read_profile_dpi_region(readers, writer, 0)?;
        println!(
            "Profile 0 DPI bytes 0x000C..0x0023: {}",
            format_bytes(&profile_zero)
        );

        // Do not return from this point until profile 0 has been explicitly
        // selected and confirmed. Even an ACK timeout can leave the device in
        // the newly selected profile, so restoration is attempted regardless.
        let profile_one = match select_and_verify_profile(readers, writer, 1) {
            Ok(()) => match read_profile_dpi_region(readers, writer, 1) {
                Ok(bytes) => Some(bytes),
                Err(error) => {
                    println!("Profile 1 DPI read failed: {error}");
                    None
                }
            },
            Err(error) => {
                println!("Profile 1 selection/verification failed: {error}");
                None
            }
        };

        restore_profile_zero(readers, writer)?;
        println!("Active profile restored and verified as ID 0.");

        let profile_one = profile_one.ok_or_else(|| {
            "Profile comparison incomplete; active profile 0 was restored and verified.".to_owned()
        })?;
        println!(
            "Profile 1 DPI bytes 0x000C..0x0023: {}",
            format_bytes(&profile_one)
        );
        if profile_zero == profile_one {
            println!(
                "The sampled DPI region is identical. EEPROM profile scope remains inconclusive because the two profiles may contain equal values."
            );
        } else {
            println!(
                "The sampled DPI region differs between profile IDs 0 and 1; this is evidence that the region is profile-dependent. No settings bytes were written."
            );
        }
        Ok(())
    }

    fn read_profile_dpi_region(
        readers: &mut [Reader],
        writer: &Owned,
        profile_id: u8,
    ) -> Result<[u8; 24], String> {
        let mut bytes = [0_u8; 24];
        for (index, (address, requested_length)) in PROFILE_SCOPE_DPI_READS.into_iter().enumerate()
        {
            let name = format!("profile {profile_id} DPI EEPROM 0x{address:04X}");
            let request = eeprom_read_request(address, requested_length);
            let (reader_index, report, length) =
                request_reply(readers, writer, &request, 0x08, &name)?
                    .ok_or_else(|| format!("No reply for {name}."))?;
            let end = 6 + requested_length as usize;
            if length < end || report[0] != PREFIX || report[1] != 0x08 || report[2] != 0 {
                return Err(format!(
                    "Invalid {name} reply from {} (length={length}, prefix=0x{:02X}/0x{:02X}, status=0x{:02X}).",
                    readers[reader_index].candidate.label(),
                    report[0],
                    report[1],
                    report[2]
                ));
            }
            let offset = index * requested_length as usize;
            bytes[offset..offset + requested_length as usize].copy_from_slice(&report[6..end]);
        }
        Ok(bytes)
    }

    fn select_and_verify_profile(
        readers: &mut [Reader],
        writer: &Owned,
        profile_id: u8,
    ) -> Result<(), String> {
        send_profile_select(readers, writer, profile_id)?;
        let current = current_profile_id(readers, writer)?;
        if current != profile_id {
            return Err(format!(
                "Profile-select ACK arrived, but current profile query returned ID {current}, expected {profile_id}."
            ));
        }
        Ok(())
    }

    fn send_profile_select(
        readers: &mut [Reader],
        writer: &Owned,
        profile_id: u8,
    ) -> Result<(), String> {
        let request = profile_select_request(profile_id);
        let (index, report, length) = request_reply(
            readers,
            writer,
            &request,
            0x10,
            &format!("select profile {profile_id}"),
        )?
        .ok_or_else(|| format!("No profile-select ACK for ID {profile_id}."))?;
        if length < 3 || report[0] != PREFIX || report[1] != 0x10 || report[2] != 0 {
            return Err(format!(
                "Profile-select ACK for ID {profile_id} was invalid on {} (length={length}, status=0x{:02X}).",
                readers[index].candidate.label(),
                report[2]
            ));
        }
        Ok(())
    }

    fn restore_profile_zero(readers: &mut [Reader], writer: &Owned) -> Result<(), String> {
        let mut last_error = None;
        for attempt in 1..=PROFILE_RESTORE_ATTEMPTS {
            let select_result = send_profile_select(readers, writer, 0);
            let profile_result = current_profile_id(readers, writer);
            if profile_result == Ok(0) {
                if let Err(error) = select_result {
                    println!(
                        "Profile 0 select ACK was not validated, but the follow-up 0x0E query confirmed profile ID 0: {error}"
                    );
                }
                return Ok(());
            }
            let detail = match (select_result, profile_result) {
                (Err(select_error), Err(profile_error)) => {
                    format!(
                        "select request failed ({select_error}); current-profile verification failed ({profile_error})"
                    )
                }
                (Err(select_error), Ok(profile_id)) => {
                    format!(
                        "select request failed ({select_error}); current profile is {profile_id}"
                    )
                }
                (Ok(()), Err(profile_error)) => {
                    format!(
                        "select ACK succeeded; current-profile verification failed ({profile_error})"
                    )
                }
                (Ok(()), Ok(profile_id)) => {
                    format!("select ACK succeeded; current profile is {profile_id}, expected 0")
                }
            };
            println!(
                "Profile 0 restore attempt {attempt}/{PROFILE_RESTORE_ATTEMPTS} did not validate: {detail}"
            );
            last_error = Some(detail);
        }
        Err(format!(
            "CRITICAL: could not confirm restoration to profile ID 0 after {PROFILE_RESTORE_ATTEMPTS} attempts. The mouse may still be on profile ID 1; use the mouse/OEM controls to restore ID 0. Last error: {}",
            last_error.unwrap_or_else(|| "unknown profile restore failure".to_owned())
        ))
    }

    fn current_profile_id(readers: &mut [Reader], writer: &Owned) -> Result<u8, String> {
        let (index, report, length) = request_reply(
            readers,
            writer,
            &PROFILE_REQUEST,
            0x0E,
            "current profile verification",
        )?
        .ok_or_else(|| "No current-profile reply within 1000 ms.".to_owned())?;
        if length < 7 || report[0] != PREFIX || report[1] != 0x0E || report[2] != 0 || report[6] > 3
        {
            return Err(format!(
                "Invalid current-profile reply on {} (length={length}, status=0x{:02X}, value={}).",
                readers[index].candidate.label(),
                report[2],
                report[6]
            ));
        }
        Ok(report[6])
    }

    fn profile_select_request(profile_id: u8) -> [u8; 17] {
        let mut request = [0_u8; 17];
        request[0] = 0x08;
        // OEM 0x44E590 passes a payload pointer before pushing the writer
        // arguments: payload[0] is command 0x10, payload[4] is selector 1,
        // and payload[5] is the requested profile ID.
        request[1] = 0x10;
        request[5] = 0x01;
        request[6] = profile_id;
        request[16] = 0x3C_u8.wrapping_sub(profile_id);
        request
    }

    fn dpi_roundtrip_test(readers: &mut [Reader], writer: &Owned) -> Result<(), String> {
        let profile_before_read = current_profile_id(readers, writer).map_err(|error| {
            format!(
                "DPI roundtrip stopped before reading EEPROM because profile ID 0 was not confirmed: {error}"
            )
        })?;
        if profile_before_read != 0 {
            return Err(format!(
                "DPI roundtrip stopped before reading EEPROM because current profile is ID {profile_before_read}, expected 0."
            ));
        }
        let original = read_dpi_chunk(readers, writer, "DPI roundtrip pre-read")?;
        if original != DPI_ROUNDTRIP_BASELINE {
            return Err(format!(
                "DPI roundtrip aborted without writing: 0x{DPI_ROUNDTRIP_ADDRESS:04X}/8 was {}, expected {}.",
                format_bytes(&original),
                format_bytes(&DPI_ROUNDTRIP_BASELINE)
            ));
        }
        println!(
            "Pre-read 0x{DPI_ROUNDTRIP_ADDRESS:04X}/8 matches expected profile-0 bytes: {}",
            format_bytes(&original)
        );
        let profile_after_pre_read = current_profile_id(readers, writer).map_err(|error| {
            format!(
                "DPI roundtrip aborted without writing because profile ID 0 could not be reconfirmed after the baseline read: {error}"
            )
        })?;
        if profile_after_pre_read != 0 {
            return Err(format!(
                "DPI roundtrip aborted without writing because profile changed to ID {profile_after_pre_read} after the baseline read."
            ));
        }

        // From this point, treat the write as possibly applied even when the
        // ACK path fails; restoration is unconditional and readback-verified.
        let write_result = send_dpi_chunk_write(
            readers,
            writer,
            &DPI_ROUNDTRIP_CHANGED,
            "temporary DPI roundtrip write",
        );
        if let Err(error) = &write_result {
            println!("Temporary DPI write was not acknowledged: {error}");
        } else {
            println!("Temporary DPI write ACK validated (status 0).");
        }

        let changed_readback = match ensure_profile_zero(readers, writer, "temporary DPI readback")
        {
            Ok(()) => read_dpi_chunk(readers, writer, "temporary DPI readback"),
            Err(error) => Err(format!(
                "Skipped DPI readback because profile ID 0 could not be confirmed: {error}"
            )),
        };
        let changed_confirmed = match &changed_readback {
            Ok(bytes) if bytes == &DPI_ROUNDTRIP_CHANGED => {
                println!(
                    "Changed readback matches 6300-DPI bytes: {}",
                    format_bytes(bytes)
                );
                true
            }
            Ok(bytes) => {
                println!(
                    "Changed readback differs from the requested bytes: {}",
                    format_bytes(bytes)
                );
                false
            }
            Err(error) => {
                println!("Changed readback failed: {error}");
                false
            }
        };

        restore_dpi_chunk(readers, writer, &original)?;
        println!("Original bytes at 0x{DPI_ROUNDTRIP_ADDRESS:04X}/8 restored and verified.");

        if write_result.is_err() || !changed_confirmed {
            return Err(
                "DPI roundtrip did not validate the temporary changed value; the original bytes were restored and verified."
                    .to_owned(),
            );
        }
        Ok(())
    }

    fn read_dpi_chunk(
        readers: &mut [Reader],
        writer: &Owned,
        name: &str,
    ) -> Result<[u8; 8], String> {
        let request = eeprom_read_request(DPI_ROUNDTRIP_ADDRESS, 8);
        let (index, report, length) = request_reply(readers, writer, &request, 0x08, name)?
            .ok_or_else(|| format!("No reply for {name}."))?;
        if length < 14 || report[0] != PREFIX || report[1] != 0x08 || report[2] != 0 {
            return Err(format!(
                "Invalid {name} reply from {} (length={length}, prefix=0x{:02X}/0x{:02X}, status=0x{:02X}).",
                readers[index].candidate.label(),
                report[0],
                report[1],
                report[2]
            ));
        }
        Ok(report[6..14]
            .try_into()
            .expect("validated exact eight-byte EEPROM response"))
    }

    fn send_dpi_chunk_write(
        readers: &mut [Reader],
        writer: &Owned,
        bytes: &[u8; 8],
        name: &str,
    ) -> Result<(), String> {
        let request = dpi_write_request(DPI_ROUNDTRIP_ADDRESS, bytes);
        let (index, report, length) = request_reply(readers, writer, &request, 0x07, name)?
            .ok_or_else(|| format!("No 0x07 ACK for {name}."))?;
        if length < 3 || report[0] != PREFIX || report[1] != 0x07 || report[2] != 0 {
            return Err(format!(
                "Invalid 0x07 ACK for {name} on {} (length={length}, status=0x{:02X}).",
                readers[index].candidate.label(),
                report[2]
            ));
        }
        Ok(())
    }

    fn restore_dpi_chunk(
        readers: &mut [Reader],
        writer: &Owned,
        original: &[u8; 8],
    ) -> Result<(), String> {
        let mut last_error = None;
        for attempt in 1..=DPI_RESTORE_ATTEMPTS {
            ensure_profile_zero(readers, writer, "DPI restore pre-read").map_err(|error| {
                format!(
                    "CRITICAL: refused to read or restore the saved profile-0 DPI chunk because profile ID 0 could not be confirmed. Verify profile 0 and restore stage 6 to 6400 DPI manually. {error}"
                )
            })?;
            match read_dpi_chunk(readers, writer, "DPI restore pre-read") {
                Ok(bytes) if &bytes == original => return Ok(()),
                Ok(bytes) => println!(
                    "Restore attempt {attempt}/{DPI_RESTORE_ATTEMPTS}: current bytes are {}, writing the saved original.",
                    format_bytes(&bytes)
                ),
                Err(error) => println!(
                    "Restore attempt {attempt}/{DPI_RESTORE_ATTEMPTS}: pre-read failed ({error}); attempting to write the saved original."
                ),
            }

            ensure_profile_zero(readers, writer, "DPI restore write").map_err(|error| {
                format!(
                    "CRITICAL: refused to send saved DPI bytes because profile ID 0 could not be confirmed. Verify profile 0 and restore stage 6 to 6400 DPI manually. {error}"
                )
            })?;
            let ack_result = send_dpi_chunk_write(
                readers,
                writer,
                original,
                "DPI original-value restore write",
            );
            ensure_profile_zero(readers, writer, "DPI restore readback").map_err(|error| {
                format!(
                    "CRITICAL: cannot read back the DPI restore safely because profile ID 0 could not be confirmed. The saved chunk may or may not have been applied; verify profile 0 and stage 6 manually. {error}"
                )
            })?;
            let readback = read_dpi_chunk(readers, writer, "DPI original-value restore readback");
            match readback {
                Ok(bytes) if &bytes == original => {
                    if let Err(error) = ack_result {
                        println!(
                            "Restore ACK was not validated, but exact original-value readback confirmed restoration: {error}"
                        );
                    }
                    return Ok(());
                }
                Ok(bytes) => {
                    last_error = Some(format!(
                        "attempt {attempt} read back {} instead of {}",
                        format_bytes(&bytes),
                        format_bytes(original)
                    ));
                }
                Err(error) => {
                    let ack = ack_result
                        .err()
                        .map(|detail| format!("; restore ACK error: {detail}"))
                        .unwrap_or_default();
                    last_error = Some(format!(
                        "attempt {attempt} could not verify restore readback: {error}{ack}"
                    ));
                }
            }
        }
        Err(format!(
            "CRITICAL: could not restore and verify original DPI bytes at 0x{DPI_ROUNDTRIP_ADDRESS:04X}/8 after {DPI_RESTORE_ATTEMPTS} attempts. The mouse may still use changed DPI values; manually restore stage 6 to 6400 DPI in the OEM app. Last error: {}",
            last_error.unwrap_or_else(|| "unknown restoration failure".to_owned())
        ))
    }

    fn ensure_profile_zero(
        readers: &mut [Reader],
        writer: &Owned,
        context: &str,
    ) -> Result<(), String> {
        match current_profile_id(readers, writer) {
            Ok(0) => Ok(()),
            Ok(profile_id) => {
                println!(
                    "{context}: current profile is ID {profile_id}; attempting bounded recovery to ID 0."
                );
                restore_profile_zero(readers, writer)
            }
            Err(error) => {
                println!(
                    "{context}: current profile is unknown ({error}); attempting bounded recovery to ID 0."
                );
                restore_profile_zero(readers, writer)
            }
        }
    }

    fn read_dpi_eeprom(readers: &mut [Reader], writer: &Owned, profile_id: u8) {
        for (address, requested_length) in DPI_EEPROM_READS {
            let request = eeprom_read_request(address, requested_length);
            let name = format!("EEPROM 0x{address:04X}");
            match request_reply(readers, writer, &request, 0x08, &name) {
                Ok(Some((index, report, length)))
                    if length >= 6 + requested_length as usize
                        && report[0] == PREFIX
                        && report[1] == 0x08
                        && report[2] == 0 =>
                {
                    let end = 6 + requested_length as usize;
                    let bytes = report[6..end]
                        .iter()
                        .map(|byte| format!("{byte:02X}"))
                        .collect::<Vec<_>>()
                        .join(" ");
                    println!(
                        "EEPROM 0x{address:04X}..0x{:04X} (reported profile {profile_id}) via {}: {bytes}",
                        address + requested_length as u16 - 1,
                        readers[index].candidate.label()
                    );
                }
                Ok(Some((index, report, length))) => println!(
                    "EEPROM 0x{address:04X} reply from {} was invalid (length={length}, prefix=0x{:02X}/0x{:02X}, status=0x{:02X}).",
                    readers[index].candidate.label(),
                    report[0],
                    report[1],
                    report[2]
                ),
                Ok(None) => println!("EEPROM 0x{address:04X}: no reply within 1000 ms."),
                Err(error) => println!("EEPROM 0x{address:04X} read failed: {error}"),
            }
        }
    }

    fn same_value_dpi_write_test(readers: &mut [Reader], writer: &Owned) {
        let pre_read_request = eeprom_read_request(0x000C, 8);
        let pre_read = match request_reply(
            readers,
            writer,
            &pre_read_request,
            0x08,
            "same-value DPI pre-read",
        ) {
            Ok(Some((index, report, length)))
                if length >= 14 && report[0] == PREFIX && report[1] == 0x08 && report[2] == 0 =>
            {
                let bytes: [u8; 8] = report[6..14].try_into().expect("validated 8-byte range");
                println!(
                    "Pre-read 0x000C via {}: {}",
                    readers[index].candidate.label(),
                    format_bytes(&bytes)
                );
                bytes
            }
            Ok(Some((index, report, length))) => {
                println!(
                    "Same-value DPI test aborted before write: pre-read from {} was invalid (length={length}, prefix=0x{:02X}/0x{:02X}, status=0x{:02X}).",
                    readers[index].candidate.label(),
                    report[0],
                    report[1],
                    report[2]
                );
                return;
            }
            Ok(None) => {
                println!("Same-value DPI test aborted before write: pre-read timed out.");
                return;
            }
            Err(error) => {
                println!("Same-value DPI test aborted before write: pre-read failed: {error}");
                return;
            }
        };

        if pre_read != SAME_VALUE_DPI_BYTES {
            println!(
                "Same-value DPI test aborted without writing: pre-read bytes differ from the authorized expected value {}.",
                format_bytes(&SAME_VALUE_DPI_BYTES)
            );
            return;
        }

        let write_request = dpi_write_request(0x000C, &pre_read);
        let ack = request_reply(
            readers,
            writer,
            &write_request,
            0x07,
            "same-value DPI write ACK",
        );
        match ack {
            Ok(Some((index, report, length)))
                if length >= 3 && report[0] == PREFIX && report[1] == 0x07 && report[2] == 0 =>
            {
                println!(
                    "Write ACK: status 0 on {}.",
                    readers[index].candidate.label()
                );
            }
            Ok(Some((index, report, length))) => println!(
                "Write ACK: invalid or nonzero status on {} (length={length}, prefix=0x{:02X}/0x{:02X}, status=0x{:02X}).",
                readers[index].candidate.label(),
                report[0],
                report[1],
                report[2]
            ),
            Ok(None) => println!("Write ACK: no reply within 1000 ms."),
            Err(error) => println!("Write ACK: request/wait failed: {error}"),
        }

        let readback_request = eeprom_read_request(0x000C, 8);
        match request_reply(
            readers,
            writer,
            &readback_request,
            0x08,
            "same-value DPI readback",
        ) {
            Ok(Some((index, report, length)))
                if length >= 14 && report[0] == PREFIX && report[1] == 0x08 && report[2] == 0 =>
            {
                let readback: [u8; 8] = report[6..14].try_into().expect("validated 8-byte range");
                println!(
                    "Readback via {}: {}; matches pre-read: {}.",
                    readers[index].candidate.label(),
                    format_bytes(&readback),
                    readback == pre_read
                );
            }
            Ok(Some((index, report, length))) => println!(
                "Readback: invalid reply from {} (length={length}, prefix=0x{:02X}/0x{:02X}, status=0x{:02X}).",
                readers[index].candidate.label(),
                report[0],
                report[1],
                report[2]
            ),
            Ok(None) => println!("Readback: no reply within 1000 ms."),
            Err(error) => println!("Readback failed: {error}"),
        }
    }

    fn dpi_write_request(address: u16, bytes: &[u8; 8]) -> [u8; 17] {
        let mut request = [0_u8; 17];
        request[0] = 0x08;
        request[1] = 0x07;
        request[2] = 0;
        request[3] = (address >> 8) as u8;
        request[4] = address as u8;
        request[5] = bytes.len() as u8;
        request[6..14].copy_from_slice(bytes);
        let sum = request[..16]
            .iter()
            .fold(0_u8, |sum, byte| sum.wrapping_add(*byte));
        request[16] = 0x55_u8.wrapping_sub(sum);
        request
    }

    fn format_bytes(bytes: &[u8]) -> String {
        bytes
            .iter()
            .map(|byte| format!("{byte:02X}"))
            .collect::<Vec<_>>()
            .join(" ")
    }

    fn eeprom_read_request(address: u16, length: u8) -> [u8; 17] {
        let mut request = [0_u8; 17];
        request[0] = 0x08;
        request[1] = 0x08;
        request[3] = (address >> 8) as u8;
        request[4] = address as u8;
        request[5] = length;
        let sum = request[..16]
            .iter()
            .fold(0_u8, |sum, byte| sum.wrapping_add(*byte));
        request[16] = 0x55_u8.wrapping_sub(sum);
        request
    }

    fn request_reply(
        readers: &mut [Reader],
        writer: &Owned,
        request: &[u8; 17],
        expected_id: u8,
        name: &str,
    ) -> Result<Option<(usize, [u8; 17], usize)>, String> {
        for reader in readers.iter_mut() {
            reader.cancel();
        }
        for reader in readers.iter_mut() {
            match reader.arm() {
                Ok(true) => {}
                Ok(false) => return Err(format!("Not all input readers armed for {name}.")),
                Err(code) => return Err(format!("Input reader failed for {name} (Win32 {code}).")),
            }
        }

        let mut request = *request;
        // SAFETY: Requests are read-only except the CLI-gated same-value test,
        // the one guarded DPI roundtrip, or profile selection in its own mode.
        if unsafe { HidD_SetFeature(writer.0, request.as_mut_ptr().cast(), request.len() as u32) }
            == 0
        {
            return Err(format!(
                "Feature send failed for {name} (Win32 {}).",
                error()
            ));
        }

        let deadline = Instant::now() + Duration::from_secs(1);
        let mut reads = 0;
        while Instant::now() < deadline && reads < 50 {
            let remaining = deadline.saturating_duration_since(Instant::now());
            let events: Vec<_> = readers.iter().map(|reader| reader.event.0).collect();
            let milliseconds = remaining.as_millis().clamp(1, (u32::MAX - 1) as u128) as u32;
            // SAFETY: The event array contains one live event per reader.
            let result = unsafe {
                WaitForMultipleObjects(events.len() as u32, events.as_ptr(), 0, milliseconds)
            };
            if result == WAIT_TIMEOUT {
                return Ok(None);
            }
            if result == WAIT_FAILED {
                return Err(format!("Wait failed for {name} (Win32 {}).", error()));
            }
            let index = (result - WAIT_OBJECT) as usize;
            if index >= readers.len() {
                return Err(format!(
                    "Unexpected wait result for {name}: 0x{result:08X}."
                ));
            }
            reads += 1;
            let reader = &mut readers[index];
            let length = reader
                .take()
                .map_err(|code| format!("Input completion failed for {name} (Win32 {code})."))?;
            if length > 1 && reader.buffer[0] == PREFIX && reader.buffer[1] == expected_id {
                return Ok(Some((index, reader.buffer, length)));
            }
            let _ = reader.arm();
            if !readers.iter().any(|reader| reader.pending) {
                return Ok(None);
            }
        }
        Ok(None)
    }

    fn error() -> u32 {
        // SAFETY: Reads the current thread's last Win32 error code.
        unsafe { GetLastError() }
    }
}

#[cfg(windows)]
fn main() {
    let mode = match parse_args() {
        Ok(value) => value,
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(2);
        }
    };
    if let Err(error) = win::run(mode) {
        eprintln!("Diagnostic stopped: {error}");
        std::process::exit(1);
    }
}

#[cfg(not(windows))]
fn main() {
    if let Err(error) = parse_args() {
        eprintln!("{error}");
        std::process::exit(2);
    }
    eprintln!("This diagnostic is Windows-only.");
}

fn parse_args() -> Result<DiagnosticMode, String> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    match args.as_slice() {
        [] => Ok(DiagnosticMode::ReadOnly),
        [argument] if argument.to_str() == Some("--same-value-dpi-test") => {
            Ok(DiagnosticMode::SameValueDpiTest)
        }
        [argument] if argument.to_str() == Some("--profile-scope-check") => {
            Ok(DiagnosticMode::ProfileScopeCheck)
        }
        [argument] if argument.to_str() == Some("--dpi-roundtrip-test") => {
            Ok(DiagnosticMode::DpiRoundtripTest)
        }
        _ => Err("Usage: read_only_multi_endpoint_probe.exe [--same-value-dpi-test | --profile-scope-check | --dpi-roundtrip-test]".to_owned()),
    }
}
