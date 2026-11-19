# Daxa Control implementation ledger

Updated: 2026-11-19

## Program outcome

Deliver a lightweight Rust Windows 11 installer in English that reproduces the
Daxa Air Mini Wireless controls, improves the UI and responsiveness, and keeps
useful status in the system tray, including battery level. Device behavior must
be verified before the app sends configuration writes.

## Acceptance evidence

- DPI, polling, buttons, lighting, macros, and profiles work on the mouse and
  their behavior is checked against the original app or read-back from the
  device; local-only controls are not described as applied.
- Connection and battery status are accurate in the window and tray when the
  receiver provides them; unsupported or stale values are reported honestly.
- The English UI is readable at the default and minimum window sizes and stays
  responsive while device discovery runs.
- A Windows 11 x64 installer builds and installs the app without deleting user
  settings on uninstall.

## Phase ledger

| Phase | Status | Owner | Dependency / checkpoint |
|---|---|---|---|
| Rust app, local profiles, editor pages, tray, and installer foundation | Complete with limits | Lead | Windows-target `cargo check` and Inno Setup compile pass; DPI, button, macro, and lighting settings remain local-only; explicit onboard profile switching is available. Button-to-macro selection uses the local macro library. Recording status reads the action count without cloning recorded steps, and macro dirty detection now uses an edit revision instead of cloning the selected macro every frame. |
| Static reverse engineering | In progress | Lead + Builder | Report framing, battery/profile queries, EEPROM read command `0x08`, write command `0x07`, DPI/color records, the 16-slot key-matrix row/checksum serializer, polling-rate encoding, profile-select `0x10`, the Windows pointer API path, and dormancy's 10-second storage units are reconstructed. Offline disassembly maps the `.jmm` envelope (834 bytes, byte-wise rotate, magic, and 90 eight-byte record slots). Its validator checks the leading WORD at `+0x6A`, applies runtime mask checks to `0xF0`–`0xF6` codes scanned from `+0x72`, and lets other nonzero codes bypass that mask test; converters start entries at `+0x6A`, so a leading-slot alignment is plausible but unproven. No sample exists to validate compatibility, and most remaining action semantics and the six-byte EEPROM payload remain unresolved. The `.jma` outer transfer is traced at 0x28CC bytes with magic `0xFCAA1234`, three groups of sixteen 0x18-byte entries, and embedded `.jmm` blocks; field meanings remain unresolved and no sample exists. OEM `GetConnectStatus` (`0x03`) accepts status byte `0` and treats its first data byte `1` as online and `0` as offline. Vendor artwork confirms six physical assignment zones; `text.xml` and `Cfg.ini` expose the original action labels and ten default mapping records. Static UI-init and menu-builder traces directly link 33 label/action rows (five basic mouse actions, Fire key, Key combination, Disable, RGB on/off/switch, polling/profile switch, eight multimedia actions, ten shortcuts, and three DPI actions) to raw selector/value words; three category rows have uncertain roles, and several other labels remain unmapped. Transfer of a selected row into a particular K descriptor and physical-button mapping remain unproven. The `K%d_%d` parser builds 16×0x18-byte profile blocks, and a separate saver converts 16 descriptors into EEPROM key-matrix rows at `0x60`; direct data flow between these buffers is unproven. K7–K10's `0x08/0xA2` prefix matches the direct UI label mapping for Disable and returns zero through the converter; transfer of the selected UI row into those K descriptors and physical-button identity remain unproven. The 10-byte LogoRGB record at `0x54` exposes RGB, mode, light, and speed offsets with complement checksums. The `0x4C` loader/saver uses data at offsets 0, 2, and 4 with complement bytes between them, but their UI semantics and lighting-zone mapping remain unknown. The MainRGB parser labels `mode`, `speed`, `light`, and RGB at offsets 0, 4, 5, and 1–3; the INI loader maps `LEDParam` to local mode, speed, light, and color defaults. Its load reads eight bytes while the save path uses seven, and the relation is unresolved. Nine live EEPROM reads captured polling, DPI, and DPI-color regions; the DPI range `0x0C–0x23` was later confirmed profile-dependent for IDs 0 and 1, while other region scopes, power-cycle persistence, and changed-value writes outside one tested DPI chunk remain open. |
| Live receiver and battery verification | DeviceMonitor read-only runtime confirmed on wired PID `0xFA7B`; same-value and one changed-value DPI write/readback confirmed; DPI profile scope confirmed | Device + Lead | The user confirms the physical path is USB data cable with the dongle removed. Windows exposes PID `0xFA7B` with four collections. The Rust DeviceMonitor's read-only CLI runtime reports Online, profile 0, and 100% battery. Nine live EEPROM reads captured polling, DPI, and color regions. A corrected `0x10` request selected profile ID 1, a `0x0E` query confirmed it, and the diagnostic restored and verified ID 0. The sampled DPI bytes at `0x0C–0x23` differed between IDs 0 and 1, establishing profile dependence for that range only. A profile-0 stage-6 DPI round trip temporarily changed the eight-byte chunk at 0x001C from 6400 to 6300 DPI, received ACK status 0 and exact changed readback, then restored and verified the original bytes. This confirms only the immediate write/readback/restore path for that chunk; power-cycle persistence and other write paths remain unverified. A first malformed profile request received no ACK and left the query at ID 0. The GUI window/tray presentation after the endpoint split remains unverified. |
| Safe device configuration | Gated | Lead | Profile dependence is established for DPI addresses `0x0C–0x23`, and static DPI encoding plus one same-value write ACK/readback are known. A single profile-0 stage-6 DPI chunk passed changed-value ACK, exact immediate readback, and exact restoration. The general capability remains false because other DPI chunks and profiles, power-cycle persistence, colors, buttons, polling, lighting, and the integrated apply path remain unverified. |
| Original feature parity | Partial | Lead | The local macro editor records keyboard and mouse actions on explicit request, and a button can reference one of those local macros by stable ID. Eight additional OEM button-action labels are now available in the dropdown, with existing saved indices preserved; they remain local selections and are not sent to or executed by the mouse. Dormancy is represented in seconds with v1-minute migration; Windows-wide double-click, pointer speed, wheel scrolling, and pointer precision have an explicit apply path. The Profiles page now provides explicit onboard profile switching through a 0x10 request and 0x0E verification; live diagnostics confirmed IDs 0 and 1, but the integrated GUI path and IDs 2 and 3 remain unverified. Mouse-side macro execution and EEPROM settings remain incomplete. |
| UI and packaging | In progress | UI/UX Designer + Lead | The refreshed shell separates receiver and battery status, tightens the local-profile panel, and refines navigation and colors. The sidebar identifies Windows 11 without a prototype label, and the connection hint specifies a USB data cable. The Buttons page includes an epaint map numbered to match the six assignment rows; each combo is labelled for assistive technology and remains keyboard-selectable. The typography-refreshed app was inspected at a 1182×710 capture resolution: the Buttons page and numbered map rendered, and DPI & Polling controls became visible after navigation; muted copy now uses 14 px text and stronger contrast. The DPI & Polling disabled state now explains that edits are saved locally and device Apply is paused pending verification; this copy-only change was not visually reinspected. The first captured frame was blank; a later frame rendered and stayed visible. Minimum-size rendering and keyboard navigation remain unverified. The changed Rust files pass scoped `rustfmt --check`; repository-wide `cargo fmt --check` still reports formatting differences in the untouched `examples/read_only_probe.rs`. The corrected Windows release app and multi-endpoint probe builds pass. Inno Setup 6.7.3 compiled the latest unsigned installer; it has not been installed. Earlier single-endpoint CLI runs returned no status; the later multi-collection diagnostic confirmed wired status and 100% battery. The updated GUI after the endpoint split, including the new manual profile-switch panel, remains unverified. |

## Current implementation boundary

- The Rust GUI/DeviceMonitor uses HID feature reports for read-only setup
  (`0x03`), current-profile telemetry (`0x0E`), and battery telemetry (`0x04`);
  its configuration-write capability remains disabled. A separate developer
  diagnostic has an explicit opt-in path for one same-value `0x07` write, which
  received ACK status 0 and immediate identical readback at `0x000C`/8. A
  separate authorized read-only profile-scope check confirmed IDs 0 and 1 have
  different DPI bytes at `0x0C–0x23` and restored ID 0. A separate authorized stage-6 test at 0x001C changed 6400 to 6300 DPI, received ACK and exact changed readback, then restored and verified the original bytes. The general write capability remains disabled.
- Settings are local schema version 3. The counter for macro IDs persists so a
  deleted macro's ID is not reassigned; button-to-macro references resolve only
  against the local library. Legacy names are linked only on a unique exact-name
  match, and imported profile IDs are discarded before matching local names.
  These references do not configure or execute macros on the mouse.
- The original EEPROM read command `0x08` is statically mapped, including its
  address, length, response-status handling, and a read-modify-write call at
  `0xA0`. The GUI monitor does not yet issue `0x08`; the developer diagnostic
  has read the polling, DPI, and color ranges on the wired device, while wider
  profile/address scope remains unverified.
- An earlier dongle debug run found one endpoint whose feature-report probe
  succeeded, then recorded no matching reply to the read-only setup command
  `0x03` within 1.01 seconds. Current-profile and battery requests were not
  reached, and no EEPROM read or configuration write was sent.
- The shipped OEM `Cfg.ini` omits `[OPT] Retry`; static tracing shows the OEM
  defaults this outer query loop to three attempts. Daxa Control now mirrors
  that bounded retry count for read-only `0x03` status queries. The per-attempt
  timeout is 1,010 ms, close to the OEM wait of about 1,030 ms. A release build
  with the new retry completed against the user-confirmed USB cable path but
  still received no `0x03` report; the UI ended in “mouse did not respond.”
- A later single-endpoint debug run confirmed repeated 0x03 setup attempts
  while the same receiver stayed enumerated, but still received no reply. The
  separate multi-collection diagnostic below later captured a validated online
  response on FF01 while sending through FF02.
- The OEM startup path separately sends a 17-byte `[08, 03, ... 00]` report,
  begins input first, waits up to 100 ms, and only checks for an input signal.
  Daxa Control now mirrors this as a preflight, discards its input without
  parsing it, then relies on the checksummed three-attempt `0x03` status loop
  for connection state. A read-only diagnostic using this shared monitor path
  has now been exercised against the wired identity; the GUI startup flow
  remains unverified. A timed-out read's cancellation/drain can extend total
  cleanup beyond 100 ms.
- A developer-only console example, `examples/read_only_probe.rs`, reuses the
  same bounded monitor and prints receiver, connection, profile, and battery
  states without opening a window. `cargo check --example read_only_probe
  --target x86_64-pc-windows-gnu --locked` passed. It does not request settings
  writes and is excluded from the installer. The optimized Windows console
  executable is at `dist/tools/DaxaReadOnlyProbe-x64.exe`, size 3,387,392 bytes,
  SHA-256 `75772E044F5B47971AC6183DFDC48939AB42DCEAC566800E01527C00A6D74B39`;
  PE inspection reports the Windows CUI subsystem. It was run after Daxa Control
  and OemDrv were no longer running; the result is recorded below.
- The user confirms the current connection is the USB data cable with the
  receiver dongle removed. Windows exposes `VID_25A7&PID_FA7B`; the PID alone
  does not establish whether wired mode has a distinct device identity. The
  updated three-attempt release still showed “mouse did not respond” and
  “Battery unavailable.” That single-endpoint run did not reach profile/battery
  queries; the later multi-collection diagnostic below did. No EEPROM read or
  configuration write was sent.
- On a later user-authorized read-only diagnostic run, Daxa Control and the OEM
  app were not running. `dist/tools/DaxaReadOnlyProbe-x64.exe` detected four
  vendor HID collections for PID `0xFA7B`, selected `FF02/0002` on interface 1,
  and reported `NoResponse` after its startup preflight and bounded status
  attempts. It did not reach profile or battery queries. No EEPROM read or
  configuration write was sent. This exercises the shared `DeviceMonitor`
  preflight/status path on the wired identity; it does not validate the GUI
  startup flow or establish why the device did not answer.
- Moving/clicking the mouse to wake it did not change the observed status; the
  existing app still reported no mouse reply and unavailable battery.
- The `0x03` response parser checks command byte `raw[1]`, success status at
  `raw[2]`, and the OEM online byte at `raw[6]`. A live response on the wired
  path validated status 0 and online value 1. The offline value 0 and malformed
  response handling remain based on static reconstruction.
- No-response, offline, and unavailable checks retry on the receiver discovery
  cadence. While online, the app rechecks status, profile, and battery on the
  60-second refresh; it clears cached profile and battery readings if the mouse
  no longer reports online. These are read-only queries.
- A multi-collection read-only diagnostic on wired PID `0xFA7B` sent status,
  current-profile, and battery requests through the feature-capable `FF02/0002`
  collection while listening on all four input collections. It received online
  status and a validated 100% battery report on `FF01/0000`; the successful
  profile reply reported ID 0. OEM accepts IDs 0–3, so the Rust parser was
  corrected to preserve `Some(0)`. A probe-directed code change now pairs
  `FF02/0002` feature requests with `FF01/0000` input on the observed wired PID.
  A rebuilt `read_only_probe` using the updated DeviceMonitor then reported
  `Online`, profile ID `0`, and battery `100%` on the wired path. The GUI
  window/tray presentation remains unverified. A bounded read-only capture then
  read the polling pair at `0x00`, DPI region `0x0C–0x2B`, and DPI-color region
  `0x2C–0x4B`; raw values are recorded in `docs/RE_NOTES.md`. A separate
  opt-in diagnostic sent one same-value `0x07` write to `0x000C`, received ACK
  status 0, and read back identical bytes. A later bounded profile check used
  the corrected static `0x10` packet, validated profile ID 1 through `0x0E`,
  read the same DPI addresses, and verified restoration to ID 0. The DPI bytes
  differed between IDs 0 and 1, establishing profile scope for that region.
  No settings bytes were written in that check. The app's write capability
  remains false because other device write paths and power-cycle persistence remain unverified.
- The profile telemetry parser follows the original app's response handling:
  status must be zero, and the first data byte is at raw offset `0x06` after the
  `0x09` transport prefix. A live response had status 0 and profile ID 0; OEM
  accepts IDs 0–3, and Rust now preserves that range. The human-facing name for
  ID 0 remains unproven, so the UI labels it as a profile ID.
- The Profiles page exposes explicit onboard profile switching. DeviceMonitor
  sends only the 0x10 selector, requires a status-zero ACK, then verifies the
  active ID through 0x0E. Local profile selection never auto-switches the
  mouse or applies local settings. IDs 0 and 1 have live diagnostic
  evidence; the integrated GUI path and IDs 2 and 3 are unverified. This
  action writes no EEPROM values.
- DPI and stage colors can be edited and saved locally. The Apply action is
  disabled while the write capability flag is false.
- Dormancy is stored in seconds at 10-second increments. The local settings
  schema is now version 3 and the profile interchange schema remains version 2;
  version-1 minute values are converted exactly during load/import, preserving
  the prior 1–60 minute range as 60–3600 seconds. Static EEPROM encoding at
  address `0xAD` can represent
  0–2550 seconds; the OEM UI/firmware's accepted range and zero meaning remain
  unknown. This remains a local profile value and has not been written to the
  mouse.
- Macro input capture is opt-in, ignores injected input and Daxa Control's own
  foreground window, and appends recorded actions to the selected local macro.
  It stops when the user leaves the Macros page or closes the window; macros
  are not sent to or executed by the mouse.
- The Windows input page reads Windows' current double-click time, pointer
  speed, wheel scroll amount, and pointer-precision state only when opened.
  Changes remain drafts until the user selects **Apply to Windows**; the native
  API then persists the user-wide settings and broadcasts the change. These
  controls do not modify Daxa device memory or local mouse profiles.
- `dist/DaxaControl-Setup-x64.exe` is a development build that includes the
  profile parser, explicit onboard mouse-profile switching, macro capture, local button-to-macro selection, persistent
  macro IDs, connection-status validation and retry, no-clone macro recording
  count updates, revision-based macro edit detection, the numbered button map,
  the expanded OEM button-action labels, UI refinements, Windows input page,
  and dormancy-seconds migration, plus the read-only startup preflight. The
  Buttons page no longer clones all macro names to build its selector each
  frame. The latest installer is 3,979,095 bytes with SHA-256
  `6BBE9AACD372ABD9BCF4C2862B12B498DE0F67D21EF917B44F8BC44B77FB5219` and is
  unsigned. The staged Windows release binary is 5,380,096 bytes with SHA-256
  `9CCB712E5D191884C1AEDB0E53CD0013F1B0F4F856950F3E344398C80A259476`; the
  Inno Setup 6.7.3 compiler log identifies that exact staged file as the
  compressed input and reports a successful compile. The refreshed app UI was previewed at a 1182×710 capture
  resolution, including the Buttons map and DPI & Polling page. The first
  captured frame was blank; after navigation the UI rendered and stayed visible
  on the next capture. Minimum-size rendering, keyboard navigation, installer
  installation, and hardware behavior of the new preflight remain unverified.

## Next checkpoint

The authorized profile-0 stage-6 round trip is complete: the temporary 6400→6300
DPI value received ACK status 0 and exact immediate readback, then the saved
6400-DPI bytes were restored and verified. No value remained changed. This
proves one eight-byte DPI write/readback/restore path only; power-cycle
persistence, the other DPI chunks and profiles, color, polling, buttons,
lighting, and general production Apply remain unverified, so the write
capability stays disabled.

The next useful device checkpoint is a separately reviewed, reversible test of
another specific setting, with an exact baseline and restoration plan. The
GUI/tray presentation after the endpoint fix and dongle PID 0xFA7C layout also
remain unverified. The current installer is unsigned and has not been
installed.