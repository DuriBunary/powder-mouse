# Daxa Control

Daxa Control is a lightweight Windows 11 companion for the Daxa Air Mini Wireless. It brings mouse status and the controls being reconstructed from Daxa Control into one native Rust app, with a quieter interface, local profiles, macros, Windows pointer options, and system-tray access.

The Overview uses the supplied mouse artwork. The interface supports light and dark themes. When the app window has a taskbar button, the icon indicates connection and battery level, and the window title contains the exact battery percentage. Windows may show the title on the taskbar or only on hover, depending on its taskbar settings. The tray menu and tooltip show the exact status while the app keeps running there. A pinned shortcut by itself cannot display live status after the app exits or its window is hidden. Device status is only as current as a valid receiver response.

## Current device support

- Wired USB telemetry has been observed on Windows 11: online status, current onboard profile, and battery percentage. Dongle battery/status still needs a live verification session.
- The Profiles page can request onboard profile selection. The protocol was verified for IDs 0 and 1; the integrated UI and IDs 2 and 3 still need device validation.
- Daxa stores settings in device EEPROM. A developer diagnostic confirmed ACK, immediate readback, and restoration for one DPI stage in profile 0. That does not yet establish power-cycle persistence or safe writes for every DPI stage, polling, buttons, or lighting.
- General mouse-setting Apply stays paused until each write path is validated. DPI, polling, button, lighting, local profiles, and macros can currently be edited locally; local edits do not mean those values were applied to the mouse. Macro recording is opt-in and records actions on this PC; mouse-side macro execution is not implemented.
- Windows input settings can explicitly apply the current user's double-click time, pointer speed, wheel scrolling, and pointer-precision setting. These affect Windows globally, not the mouse EEPROM.

The app reports unavailable or stale telemetry instead of treating it as current. See [the reverse-engineering notes](docs/RE_NOTES.md) and [implementation status](docs/IMPLEMENTATION_STATUS.md) for the evidence and remaining device-parity work.

## Build

Build the application from this folder with:

```powershell
cargo build --release --target x86_64-pc-windows-gnu --locked
```

To create the per-user Windows installer, install Inno Setup 6 or 7 and run:

```powershell
.\build-installer.ps1
```

The installer is written to `dist\DaxaControl-Setup-x64.exe`. It installs under `%LOCALAPPDATA%\Programs\Daxa Control`, creates a Start Menu shortcut, and keeps profiles in `%APPDATA%\Daxa Control` when uninstalled.

## Read-only device diagnostic

For a console check without opening the app window, run the prebuilt Windows x64 utility:

```powershell
.\dist\tools\DaxaReadOnlyProbe-x64.exe
```

To rebuild it from source instead:

```powershell
cargo run --release --target x86_64-pc-windows-gnu --example read_only_probe --locked
```

It reports receiver discovery, the selected HID collection, connection status, and—only after a validated online response—profile and battery telemetry. The example sends no settings or EEPROM writes and is not included in the installer. Close Daxa Control before running it so two processes do not compete for receiver replies. The check does not open a window or move the cursor.
