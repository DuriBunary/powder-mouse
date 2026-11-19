# Daxa Air Mini Wireless reverse-engineering notes

These notes separate facts observed in shipped files from protocol details that
remain unverified. Most reverse engineering was static and read-only. On
2026-11-19, the original app was opened with user approval for status/menu
inspection only; its screen showed battery 100%, profile 1, and default
mappings. No Apply or settings changes were made. This is visual evidence, not
proof of a fresh protocol response. No configuration or EEPROM write was sent.

## Target identity

- Installed application: `D:\Daxa Air Mini Wireless\OemDrv.exe`
- SHA-256: `ebf004d5fec4b84f1e267afe81b2acd514ccfd53b260d4b1818aa41cfc5fa34b`
- File version: `1.0.0.0`
- Format: PE32 / i386; REA found no admitted .NET CLI metadata.
- Supporting files read: `Cfg.ini` and `text.xml` in the same installation.

REA 6.7.0's PE resource inventory reports complete coverage of 125 directory
entries and 58 resources for this same SHA-256. It identifies one icon group;
the resource parser does not render icon payloads or observe runtime loading.
The full JSON Evidence record is retained at
`docs/rea-evidence/oem-pe-resources.json`.

REA Evidence IDs: PE resources `ev_1bdd18857b6a40a68373bf6665c36427db7bb0552a5964ba442a6037ab26749f`,
managed metadata classification `ev_8799ce513cb71095d0567b3033b1621d8548c99b73fb2275de35559c07185371`,
PE headers `ev_0c3a4f071b9d2993e1a57f192cd12938a9da51b971ba68f9c8689eadaf1dec5d`,
and section headers `ev_bf8d549ce3e9198b6511ae32f6e4cd9f39b2815579d9365dff8812c36abb017a`.

## Features named by the original app

`text.xml` labels these areas and actions:

- Six DPI stages, DPI lock/cycle/up/down, DPI colors and DPI effect.
- Polling rate, lift-off distance, ripple control, angle snapping, and mouse parameters.
- Button labels for mouse buttons, forward/back, scroll up/down, double/right-double/triple click,
  disable, fire key, single key, key combinations, and macros. The XML also names play/pause,
  stop play, previous/next media, volume up/down, mute, stop web, search, email, home page,
  Explorer, calculator, refresh, favorites, and Media Player.
- Special button actions include DPI, DPI up/down/loop/lock, polling-rate switch, profile switch,
  RGB on/off/switch, and multimedia actions. These strings show the shipped action vocabulary;
  they do not prove that every choice is selectable or accepted by this mouse firmware.
- Lighting effects: steady, breathing, streaming, neon, single-color flow, colorful
  breathing and off; brightness, speed and predefined color. Separate main/logo RGB
  conversion paths appear in binary log strings; their exact UI/device relationship
  is not confirmed.
- Macro recording, key down/up, delays, inserted events and repeat modes.
- OEM macro file filters `.jmm` and `.jma` are also present in the binary.
- Debounce, dormancy, double-click speed, pointer sensitivity, scroll speed and
  “improve pointer precision”.

These strings establish that the original binary contains labels for these controls;
they do not prove that every control is available on every firmware revision.

## Model defaults in `Cfg.ini`

The installed config identifies VID `0x25A7`, receiver PIDs `0xFA7B` and `0xFA7C`,
sensor `0x3395`, six DPI values (`400, 800, 1600, 2400, 3200, 6400`), and six
per-stage RGB colors. It also lists button mapping records `K1_1` through `K10_1`,
`KM=12`, `DR=0x1000` (meaning not decoded here), `Debounce=8`, `MinDelay=1`, and a
default LOD control. These are shipped defaults, not a complete protocol schema.

The shipped `skins/0904/mouse_nr.png` artwork numbers six physical control
zones: left/right click, wheel click, two side buttons, and the DPI button.
This matches the six assignment rows in Daxa Control. `K1_1` through `K10_1`
are configuration records; their names alone do not establish ten physical
buttons or the meaning of records seven through ten.

The installed `Cfg.ini` defaults these records to:

```ini
K1_1=0x01,0x11,0x00,0x01
K2_1=0x01,0x13,0x00,0x02
K3_1=0x01,0x12,0x00,0x03
K4_1=0x01,0x14,0x00,0x05
K5_1=0x01,0x15,0x00,0x04
K6_1=0x08,0xAA,0x00,0x06
K7_1=0x08,0xA2,0x00,0x07
K8_1=0x08,0xA2,0x09,0x08
K9_1=0x08,0xA2,0x00,0x09
K10_1=0x08,0xA2,0x00,0x0A
```

These are configuration defaults, not a decoded action-to-record mapping. The
`K7_1`–`K10_1` tuples contain the shown `0x08/0xA2` prefix, later tied directly
to the OEM “Disable” label; the transfer of a selected UI row into these K
descriptors and their physical-button meanings remain unresolved. No Rust
configuration packet has been sent to validate them.

### Static trace of `K%d_%d` records

The binary contains the wide format string `K%d_%d` at `0x5B2D4C` (file offset
`0x1B134C`). The caller at `0x448230` formats a key name, calls the imported
`GetPrivateProfileStringW` (`0x4482BB`, IAT slot `0x57D3F4`) and passes the
returned text to `0x4458D0`, which parses up to four integer values into an
output array. The parser's loop is `0x448270–0x448396`: it generates indices
1–16, and its caller at `0x449010–0x449072` invokes it for three iterations.
The supplied `Cfg.ini` has only `K1_1`–`K10_1`; the disassembly does not show
that those ten lines correspond to ten physical controls.

At `0x448306–0x44830E`, the parsed first value is placed in the high byte of a
DWORD and the low 24 bits of the second value are retained; the fourth value
is stored at descriptor offset `+4`. The third value is consulted in specific
branches: selector 2 sets marker `0x020000A1` and copies its low four bits as
flags (`0x448355–0x44838A`); selector 8 with second values `0xA6`, `0xA4`,
`0xAD`, or `0xC0` stores the third value at `+4`, with an additional default
for zero under `0xA4` (`0x448311–0x448353`). Other tuples do not show a
general third-field interpretation in this parser.

The assignment save path at `0x40FEF6–0x40FF2D` walks 16 descriptors at
24-byte strides, takes descriptor byte `+4`, subtracts one, and uses that as
an index in a 16-entry DWORD vector. It converts each descriptor through
`0x40F2E0` before placing the result in that vector. The subsequent call at
`0x41018D` reaches serializer `0x44EEF0`, whose established output is four-byte
rows with a `0x55` complement byte written from EEPROM address `0x60`. This
shows a code path from 16 internal descriptors through conversion into the
key-matrix serializer; it does not by itself identify a physical control for
any `K` record.

The config parser's caller at `0x449010–0x449072` writes parsed blocks to a
buffer at `+0x60`, copying `0x180` bytes per iteration (16 descriptors ×
24 bytes), for three iterations. The saver instead reads descriptors from
the current active object at `+0x23C` (`0x40FEF6–0x40FF13`). Both layouts use
24-byte descriptor strides, but the inspected code does not establish a direct
copy or shared pointer from the INI loader's `+0x60` area to the active
object's descriptor array. Treat those paths as related but not directly
connected. The converter's numeric return cases are recorded below; by
themselves they do not establish action names. A later table records direct UI
label associations for only a subset of selector/value pairs.

A focused pointer-flow check confirms the break. In the INI load path,
`0x4483F0–0x448431` derives a per-index base as `0x5F5918 + index*0x6E0`.
The loop at `0x449010–0x449072` calls parser `0x448230` into a temporary
buffer, copies `0x60` DWORDs (`0x180` bytes) into that base plus `0x60`, then
advances the destination by `0x180`; the three iterations therefore write
three contiguous blocks starting at that load-side global base plus `0x60`.
This is an explicit copy from parser temporary storage into the global
profile/config area.

The save dispatch obtains the descriptor base from the active-object global.
At `0x409464–0x409479`, the caller pushes `[EDI+4]` as the first stack
argument, then loads and pushes `[0x6082E8]` as the second argument to
`0x40FDE0` (with `ECX=ESI`). The callee reads the first argument at
`0x40FDF4`, then loads the second into `EBP` at `0x40FE04`; descriptor access
uses that base at `+0x238` and `+0x23C` (`0x40FEF6–0x40FF13`). The value at
`[EBP+0x238]` is used as a block index to compute a 16-slot, 24-byte-stride
descriptor address; this path does not establish that it is a count. The INI
loader's base (`0x5F5918 + index*0x6E0 + 0x60`) is still separate: no copy or
alias from that area into the active-object descriptors is shown. The
assignment callback and saver both load their base from global `0x6082E8` at
their respective execution times; the trace does not establish whether that
global retains the same allocation between an assignment and a later save.

A separate assignment callback trace narrows the UI-side path but does not
close that gap. The builder at `0x4139D0–0x414F00` calls `0x425ED0`; that
helper stores its raw action word at choice-row offset `+4`. In callback
`0x413520–0x41396E`, a local action record is prepared and passed with a target
object/slot argument to `0x413100` at `0x413932–0x413944`. `0x413100` obtains
the active object from global pointer `0x6082E8` (`0x413130–0x413142`), takes
the 24-byte record pointer from its second stack argument, and derives a slot
from `[first_argument+0x8C]` (`0x413148–0x41319A`). On the ordinary copy path,
it reads block index `[object+0x238]`, computes a 16-slot, 24-byte-stride
target from that index and the slot, and copies all six DWORDs from the
supplied record into the target beginning at `object+0x23C`
(`0x413346–0x41337F`). This directly shows an assignment callback writing a
descriptor-shaped record into the current active object region also used as
the saver base when the global pointer is unchanged.

The inspected callback does not explicitly look up or copy item data from the
`0x425ED0` choice-record table: its record pointer comes from a stack-local
prepared by the callback, and no table access appears in the traced callback
path. Whether the surrounding UI framework supplies selected-row data through
the callback remains unresolved. Thus the callback-to-active-object write and
the saver’s use of the active-object global are supported, but the selected
choice row's transfer into the callback record is not. The INI parser's
separate destination remains `0x5F5918 + index*0x6E0 + 0x60` as above.

The callback argument flow narrows that remaining gap. At `0x4135FE–0x41362C`,
argument 2 (`[EBP+0x0C]`) is bounded to indices below 16 and used to select a
UI control record with a `0x170`-byte stride. Argument 1 (`[EBP+0x08]`) is
read as a 32-bit action-like word at `0x413630`; the callback separates its
high byte and low 24 bits to choose specialized handling. On the ordinary
path, it copies that original word into the first DWORD of the stack-local
descriptor at `0x41392E`, then passes that record to `0x413100` at
`0x413932–0x413944`. Special paths for selector 5, selector 2, value `0xA4`,
selector 8/value `0xA6`, and selector `0x0B` populate or rewrite descriptor
fields before the same call. This establishes callback-argument-to-descriptor
flow, but the traced code still has no direct read of a `0x425ED0` choice row;
whether the UI framework supplies that row's `+4` action word as argument 1
remains unresolved.

A bounded whole-image pointer scan further limits what can be claimed about the
callback's registration. In the inspected OEM PE (`SHA-256
`ebf004d5fec4b84f1e267afe81b2acd514ccfd53b260d4b1818aa41cfc5fa34b`), the
DWORD `0x00413520` occurs at file offset `0x1A7754`, which maps through the
`.rdata` section (`VMA 0x57D000`, file offset `0x17B600`) to VA `0x5A9154`.
The surrounding bytes form repeated 24-byte-looking records: the record at
`0x5A9150` contains DWORDs `0x0E`, `0x00413520`, `0x438`, and three zeros; the
next at `0x5A9168` contains `0x0E`, `0x00413100`, `0x800`, and three zeros.
The function pointers decode to code entries, but the record layout and use
are not identified. A byte-aligned scan of the full input image for absolute
DWORD values in `0x5A9130–0x5A91D7` found no matches. This bounded negative
result does not rule out an address computed at runtime or an indirect table
walk; it does not establish that these records are active UI dispatch data or
connect a selected choice row to callback argument 1.

### `keyinfo_to_hardware_code` numeric cases

At `0x40F2E0–0x40F430`, the converter reads the selector from descriptor byte
`+3` and the low 24 bits from `+0`. For selector `1`, the switch at
`0x40F2F1–0x40F32D` maps values `0x11` through `0x17` respectively to
`0x00000101`, `0x00000201`, `0x00000401`, `0x00001001`, `0x00000801`,
`0x00023204`, and `0x00033204`. Other selector-1 values return `0xFFFFFFFF`.

For selector `8`, the bounds check and jump table at `0x40F349–0x40F35D` and
`0x40F450–0x40F488` cover values `0xA2`–`0xB0`. Reading the table targets with
the handlers in `0x40F364–0x40F3D4` gives these numeric results:

| Value | Converter result |
| --- | --- |
| `0xA2` | `0` |
| `0xA3` | `0xFFFFFFFF` (error/default return) |
| `0xA4` | `([+0x10] << 16) \| ([+0x11] << 8) \| 4` |
| `0xA5` | `0xFFFFFFFF` (error/default return) |
| `0xA6` | Dynamic lookup; success returns `(helper result byte << 8) \| 0x0A`; lookup failure returns `0xFFFFFFFF` (`0x40F38B–0x40F3B7`) |
| `0xA7` | `8` |
| `0xA8` | `0x202` |
| `0xA9` | `0x302` |
| `0xAA` | `0x102` |
| `0xAB` | `0xFFFFFFFF` (error/default return) |
| `0xAC` | `7` |
| `0xAD` | Dynamic: returns the DWORD at descriptor `+0x10` (`0x40F3D1–0x40F3D4`) |
| `0xAE` | `0xFFFFFFFF` (error/default return) |
| `0xAF` | `0xFFFFFFFF` (error/default return) |
| `0xB0` | `9` |

For selector `8`, values outside `0xA2`–`0xB0` reach the `0xFFFFFFFF` return.
Selectors `2`, `3`, and `4` separately return `5` (`0x40F32E–0x40F430`);
selectors not handled by the converter return `0xFFFFFFFF`. The supplied
defaults compare directly by their first two tuple fields: `K1_1`–`K5_1`
(selector `1`, values `0x11`, `0x13`,
`0x12`, `0x14`, `0x15`) produce `0x101`, `0x401`, `0x201`, `0x1001`,
`0x801`; `K6_1` (selector `8`, value `0xAA`) produces `0x102`; and
`K7_1`–`K10_1` (selector `8`, value `0xA2`) produce `0`. The separate UI trace
below directly associates that selector/value pair with the “Disable” label.
The parser-to-saver pointer-flow gap remains unresolved, so the trace does not
prove that a selected UI row reaches these K descriptors or identifies a
physical button.

### Direct UI-label to raw-code evidence

The assignment-label resource keys in `text.xml` are resolved into globals by
the UI initialization path (`0x43027F–0x430485`): `tc_click` is stored at
`0x609A84`, `tc_menu` at `0x609A88`, `tc_middle` at `0x609A8C`, `tc_forward` at
`0x609A90`, and `tc_back` at `0x609A94`. The choice-record builder at
`0x413A08–0x413B0C` retrieves those strings and calls `0x425ED0` with both a
row index and a raw numeric code. `0x425ED0` computes 0x58-byte rows and stores
the supplied code at row offset `+4` (`0x425EF1–0x425F0D`), while formatting
the supplied text into the following row data (`0x425F2E–0x425F40`). The
function stores its `EDX` argument at row offset `+0` (`0x425F07`); the five
basic mouse rows and the seven named rows below pass `EDX = 0xFF` as that
field. This is a choice-record field, not evidence of a later K descriptor's
third or fourth tuple value. This
directly associates these `text.xml` labels with raw selector/value words:

| `text.xml` key and label | Raw word passed by caller | Selector/value bytes |
| --- | ---: | --- |
| `tc_click` — Left button | `0x01000011` | `0x01 / 0x11` |
| `tc_menu` — Right button | `0x01000013` | `0x01 / 0x13` |
| `tc_middle` — Middle button | `0x01000012` | `0x01 / 0x12` |
| `tc_forward` — Forward | `0x01000014` | `0x01 / 0x14` |
| `tc_back` — Back | `0x01000015` | `0x01 / 0x15` |

The same builder contains additional direct text/code pairs. The caller passes
the resolved label pointer and the listed raw word to `0x425ED0`; its row
storage behavior is described above. The UI initialization path associates
the cited globals with the `text.xml` keys shown here: `tc_key_off` at
`0x4306E2–0x43072B`, `tc_combokey` at `0x430F4C–0x430F8E`, `tc_fire` at
`0x4313AF–0x4313F8`, `tc_rgb` at `0x431420–0x431469`, `tc_reportsw` at
`0x43165C–0x43169E`, `tc_rgbsw` at `0x43173E–0x431773`, and `tc_prosw` at
`0x431820–0x431855`:

| `text.xml` key and label | Raw word passed by caller | Selector/value bytes | Builder condition |
| --- | ---: | --- | --- |
| `tc_fire` — Fire key | `0x080000A4` | `0x08 / 0xA4` | Unconditional (`0x413BEF–0x413C1B`) |
| `tc_combokey` — Key combination | `0x020000A1` | `0x02 / 0xA1` | Unconditional (`0x413C20–0x413C4F`) |
| `tc_key_off` — Disable | `0x080000A2` | `0x08 / 0xA2` | Unconditional (`0x413C96–0x413CC2`) |
| `tc_rgb` — RGB ON/OFF | `0x080000A7` | `0x08 / 0xA7` | State fields at `[object+0x634]` and `[object+0x61C]`/`[object+0x620]` pass checks (`0x413B14–0x413B61`) |
| `tc_reportsw` — Polling rate switch | `0x080000AC` | `0x08 / 0xAC` | `[object+0x60C] != 0` (`0x413B73–0x413BA8`) |
| `tc_rgbsw` — RGB Switch | `0x080000AE` | `0x08 / 0xAE` | `0x413980` returns nonzero (`0x413C58–0x413C8D`) |
| `tc_prosw` — Profile Switch | `0x080000B0` | `0x08 / 0xB0` | Global `[0x5F58F4] != 0` (`0x413BB1–0x413BE6`) |

The corresponding `text.xml` entries are at lines 81, 93, and 143–150. For
the four conditional/state-gated rows, the disassembly proves only the
conditions and row insertion, not what the tested object/global fields mean.

The following additional direct pairs use label globals initialized from the
listed `tc_*` resources (`0x43078F–0x4310D4`, `0x4311B6–0x431855`). The
choice builder supplies both the resolved label pointer and the raw code to
`0x425ED0`; associations below come from those arguments, not list order.

| `text.xml` key and label | Raw word passed by caller | Evidence |
| --- | ---: | --- |
| `tc_kbmedia_msel` — Media Player | `0x04000021` | global `0x609AF4`; `0x413F82–0x41400B`; XML line 91 |
| `tc_PL` — Play/Pause | `0x04000022` | global `0x609AB0`; `0x414010–0x41409A`; XML line 62 |
| `tc_stop` — Stop play | `0x04000023` | global `0x609AB4`; `0x4141BD–0x414247`; XML line 63 |
| `tc_pre_media` — Previous | `0x04000024` | global `0x609AB8`; `0x41412E–0x4141B8`; XML line 64 |
| `tc_next_media` — Next | `0x04000025` | global `0x609ABC`; `0x41409F–0x414129`; XML line 65 |
| `tc_vol_up` — Volume up | `0x04000026` | global `0x609AC0`; `0x4142DB–0x414365`; XML line 66 |
| `tc_vol_dw` — Volume down | `0x04000027` | global `0x609AC4`; `0x41436A–0x4143F4`; XML line 67 |
| `tc_mute` — Mute | `0x04000028` | global `0x609AC8`; `0x41424C–0x4142D6`; XML line 68 |
| `tc_kbmedia_mail` — Email | `0x03000030` | global `0x609ADC`; `0x4143F9–0x414483`; XML line 85 |
| `tc_kbmedia_cal` — Calculator | `0x03000031` | global `0x609AE8`; `0x414488–0x414512`; XML line 88 |
| `tc_kbmedia_explore` — Explorer | `0x03000032` | global `0x609AE4`; `0x414517–0x4145A1`; XML line 87 |
| `tc_kbmedia_search` — Search | `0x03000033` | global `0x609AD8`; `0x414635–0x4146BF`; XML line 84 |
| `tc_kbmedia_main` — Home page | `0x03000034` | global `0x609AE0`; `0x4145A6–0x414630`; XML line 86 |
| `tc_kbmedia_back` or fallback `tc_back` — Back | `0x03000035` | globals `0x609AD0` / `0x609A94`; fallback when first string length is zero; `0x414760–0x4147F7`; XML line 75 |
| `tc_kbmedia_forward` or fallback `tc_forward` — Forward | `0x03000036` | globals `0x609ACC` / `0x609A90`; fallback when first string length is zero; `0x4146C4–0x41475B`; XML line 74 |
| `tc_kbmedia_stop` — Stop web | `0x03000037` | global `0x609AD4`; `0x4147FC–0x414891`; XML line 83 |
| `tc_kbmedia_refresh` — Refresh | `0x03000038` | global `0x609AEC`; `0x41489A–0x414921`; XML line 89 |
| `tc_kbmedia_favor` — Favorites | `0x03000039` | global `0x609AF0`; `0x414926–0x4149B0`; XML line 90 |
| `tc_dpiadd` — DPI + | `0x080000A8` | global `0x609C34`; `0x4149D1–0x4149FF`; XML line 138 |
| `tc_dpisub` — DPI - | `0x080000A9` | global `0x609C38`; `0x414A1C–0x414A46`; XML line 139 |
| `tc_dpiloop` — DPI Loop | `0x080000AA` | global `0x609C3C`; `0x414A5E–0x414A88`; XML line 140 |

For `tc_dpiadd`, `tc_dpisub`, and `tc_dpiloop`, the builder also passes a
byte through `EDX` (`0x4149F5`, `0x414A3C`, `0x414A7E`); its meaning is not
established here. `tc_kbmedia_forward` and `tc_kbmedia_back` have binary string
tokens but no entries in the supplied `text.xml`; the builder uses
`tc_forward`/`tc_back` when those strings are empty.

The builder also passes direct label/code rows for `tc_mac_def` with
`0x05000000` (`0x609AA8`, `0x413D32–0x413DB7`), `tc_dpi` with `0x08000000`
(`0x609C30`, `0x413DC6–0x413E5C`), and `tc_combo_tips` with `0x04000000`
(`0x609AFC`, `0x413E65–0x413EED`). Their roles as categories or actionable
assignments are not established by these calls. `0x080000A6` remains dynamic:
one path formats a key value into a row at `0x414AEB–0x414B89`, and another
uses a dynamic label at `0x413EF2–0x413F7D`; neither supplies a static
`text.xml` action label. This trace did not find direct builder pairs for
`tc_singlekey`, `tc_snipe`, `tc_three`, `tc_windows`, `tc_shift`,
`tc_customkey`, `tc_web`, or `tc_mainweb`.

A whole-image call-site scan for `0x425ED0` found callers at
`0x40A204`, `0x413A37–0x414B89`, and `0x41E6A3–0x41EBBE`. The middle range
is the label/code choice builder traced above. The `0x40A204` caller
(`0x40A1E0–0x40A216`) walks a runtime linked list, passing each node's first
word as the raw value and its `+4` pointer as the label; the inspected path
does not reference the unresolved `tc_*` labels. The `0x41E6A3–0x41EBBE`
caller builds key choices from runtime-resolved name globals and key/value
entries, then passes them to `0x425ED0`; it likewise contains no direct
reference to the unresolved action-label globals. The adjacent final call at
`0x41EC0F` is to `0x4262F0`, not the choice-record helper. These call sites
therefore do not add provable static pairs for `tc_singlekey`, `tc_snipe`,
`tc_three`, `tc_windows`, `tc_shift`, `tc_customkey`, `tc_web`, or
`tc_mainweb`. In the supplied `text.xml`, `tc_web` is the site URL and
`tc_mainweb` is empty; their presence alone does not establish assignment
choices. Other unpaired controls and action labels remain unresolved absent a
direct label-pointer/raw-word call path.

A narrower trace of the `0x40A204` path clarifies its runtime-list boundary.
The event dispatcher at `0x40A030–0x40A07A` reaches `0x40A180` for message
`0x202` only when object field `+0xDC` and global `0x607A60` are both zero;
`0x40A180` additionally returns early when object field `+0x1EC` is nonzero.
It selects a list-head pointer from the object using the runtime index at
`+0x1D4`: the effective table offset is `24 * (index + 16)` (`0x40A1C1–
0x40A1D0`). The loop processes at most 20 list elements. For each element,
`[ESI]` points to an item record; that record's first DWORD is passed as the
raw value and its `+4` field is passed by address to string helper `0x402760`.
The list element's own `[ESI+4]` advances to the next element. Each formatted
item is added through `0x425ED0` with row metadata `0xFF`, and the resulting
rows are passed to `0x4262F0` (`0x40A218–0x40A2BB`). This confirms that the
caller consumes a runtime-provided list, but does not identify the list's
producer or connect its labels to the unresolved `tc_*` resources.

The K parser stores its first two parsed tuple values in the same packed form
(`0x4482ED–0x448306`), so these five raw words match the corresponding first
two fields in the shipped `K1_1`–`K5_1` defaults. The conversion table at
`0x40F2E0–0x40F32D` then maps those selector/value pairs to the numeric codes
documented above. This proves the label-to-raw-code association in the OEM UI
metadata and its match to the K tuple prefix. It does not prove that a user's
selection is copied from this UI metadata row into a particular K descriptor
or that it identifies a physical button. The parser's `+0x60` output versus
saver's `+0x23C` input remains an unresolved link.

The profile-save caller adds a limited runtime distinction, but no UI label
mapping. After `0x40F2E0`, `0x40FEF6–0x40FF2D` stores the returned value in a
16-entry vector and branches on its low byte: `6` enters a path that looks up
the referenced macro (`0x40FF31–0x40FF62`), while `5` enters a path through
`0x40F580` that builds a variable-length buffer and passes it to writer
`0x44E670` (`0x410066–0x4100F8`). Therefore selectors `2`, `3`, and `4`, which
return numeric `5`, demonstrably take the latter path. The fixed selector-8
returns do not name an action in this code; most go through the ordinary
numeric path. The dynamic selector-8 case `0xAD` returns descriptor data and
could depend on its low byte, so its branch cannot be decided from the
selector/value pair alone. No direct xref from these converter cases or
caller branches to a user-facing label was established; `text.xml` vocabulary
does not resolve those meanings. The later serializer call at `0x41018D`
remains part of the profile-save path, but no particular UI action is proven
by these numeric results.

A separate descriptor-to-display resolver at `0x445BE0` is called by the
active-profile UI at `0x4125CB` and `0x41261D` on records based at object
`+0x23C`. The assignment callback at `0x4133C8` also calls it with the new
descriptor after copying that six-DWORD record into the corresponding
active-profile slot at `+0x23C`. The resolver reads the selector at `+3` and
the low 24 bits of the first DWORD, handles selector 5 as a macro and selector
2 as a key combination, then routes eligible low values through a byte map at
`0x446224` and a jump table at `0x44618C`. Direct string-global arguments
establish these display associations:

| Descriptor low 24-bit value | Display label | Evidence |
| ---: | --- | --- |
| `0xF4` | Double Click | `0x445EFF` uses the `tc_dbclick` global (`0x609A98`) |
| `0xF5` | Three click | `0x44611C` uses the `tc_three` global (`0x609C40`) |
| `0xF6` | R-Double Click | `0x445F10` uses the `tc_rdbclick` global (`0x609A9C`) |
| `0x180` | Disable | `0x446086` uses the `tc_key_off` global (`0x609AAC`) |
| `0x181` | Keystroke | `0x446097` uses the `tc_shift` global (`0x609C58`) |
| `0x184` | DPI Lock plus a formatted integer | `0x4460B9` formats `tc_snipe` (`0x609C4C`) with `"%s - %d"` |
| `0x185` | RGB ON/OFF | `0x4460A8` uses the `tc_rgb` global (`0x609C48`) |
| `0x186` | DPI + | `0x4460F2` uses the `tc_dpiadd` global (`0x609C34`) |
| `0x187` | DPI - | `0x446100` uses the `tc_dpisub` global (`0x609C38`) |
| `0x188` | DPI Loop | `0x44610E` uses the `tc_dpiloop` global (`0x609C3C`) |
| `0x189` | Switch mode | `0x44612A` uses `tc_msg31` (`0x609C08`) |
| `0x18A` | Polling rate switch | `0x446138` uses the `tc_reportsw` global (`0x609C5C`) |
| `0x18B` | Empty `tc_customkey` label | `0x446146` uses the `tc_customkey` global (`0x609C60`) |
| `0x18E` | Profile Switch | `0x446154` uses the `tc_prosw` global (`0x609C68`) |

These are display-path code-to-string associations only. They do not prove
that the values share a namespace with menu-builder raw words or converter
return values, nor that a selected menu row is copied into a particular K
descriptor or physical button. `tc_singlekey`, `tc_windows`, and `tc_rgbsw`
remain without a direct display-path mapping in this trace.

## HID observations

### OEM enumeration and transport

- OemDrv enumerates present HID interfaces with the HID class GUID at
  `0x00410ab0–0x00410b5d`, then obtains interface paths and inspects HID
  attributes/capabilities at `0x00410b80–0x00410d0c`. The inspected path checks
  report lengths of `0x11` and can compare interface paths; no direct VID/PID
  filter was established in this code path.
- The OEM opens a read handle with `FILE_GENERIC_READ` (`0x120089`), share mode
  `3`, `OPEN_EXISTING`, and `FILE_FLAG_OVERLAPPED` at
  `0x0040c7d2–0x0040c7e3`. It opens a separate write handle with
  `FILE_GENERIC_WRITE` (`0x120116`), share mode `3`, `OPEN_EXISTING`, and
  synchronous flags at `0x0040c872–0x0040c882`. Rust now opens its selected HID
  path with the same per-handle access and flag semantics, routing input reads
  to the overlapped handle and `HidD_SetFeature` to the synchronous handle.
  The inspected OEM path does not prove its reader and writer records resolve
  to the same HID collection Rust selected; the handle difference was a
  plausible transport mismatch, not a proven cause of the missing reply.
- A separate initialization path at `0x0040c893–0x0040c8de` zero-fills a
  17-byte report, sets bytes 0 and 1 to `0x08, 0x03`, and calls
  `HidD_SetFeature`, leaving the final checksum byte zero. It then sleeps for
  100 ms and checks whether any input read has populated global `0x0060378c`
  (`0x0040c8eb–0x0040c8fb`). The read callback stores the handle after a
  successful `ReadFile` (`0x0040c630–0x0040c679`); this path does not validate a
  matching `0x03` report or its status. It is distinct from the generic,
  checksummed `GetConnectStatus` path (`0x0043cb73` to `0x0044e460`), and the
  two paths' temporal relationship is not established. Daxa Control now sends
  this zero-checksum report only as a 100 ms startup preflight, discards any
  input without parsing it, then requires a separate validated checksummed
  `0x03` response before querying profile or battery. The preflight matches the
  observed startup sequence, but its device-side effect has not been verified.

## OEM macro interchange format (static)

The following findings come from offline disassembly of the same OEM binary
identified above (`OemDrv.exe`, SHA-256
`ebf004d5fec4b84f1e267afe81b2acd514ccfd53b260d4b1818aa41cfc5fa34b`). They
do not come from importing or exporting a live macro file.

- The `.jmm` file dialog and Add Macro path reference the filter at
  `0x00420417` and `0x00420905`. The importer reads exactly `0x342` bytes and
  rejects a short read (`0x00420606–0x00420632`).
- The decode helper at `0x004217d0` rotates each of the 834 bytes left by two
  bits. The import path then checks the decoded DWORD at offset `0` against
  `0xFACB99A0` (`0x0042063c–0x00420650`). Inverting that byte-wise transform
  implies an on-disk prefix of `28 66 F2 BE`; this prefix is derived from the
  disassembly, not observed in a sample file.
- The `.jmm` validator receives the decoded 0x342-byte buffer at `0x00420666`.
  Its WORD at `+0x6A` is not a count: it checks this leading value against
  `0xF0`–`0xF6`, maps it to a mask bit, and tests that bit against runtime mask
  `0x00607a34`. It then scans from `+0x72` in eight-byte steps until it finds
  a zero WORD (`0x00420c20–0x00420ca0`). Scanned `0xF0`–`0xF6` codes receive
  the same mask check; other nonzero codes bypass that mask test and continue.
  The fixed structure therefore has room for 90 eight-byte slots, but no
  explicit count is used by this validator and no 90-entry bound is evident. This
  corrects earlier notes that described `+0x6A` as a count and misread the
  compared codes as `0x1100`–`0x1106`.
- Diagnostic strings associate `0x0040f810` with `HdMacro_To_StMacro` and
  `0x0040fab0` with `StMacro_To_HdMacro`. The former initializes a 0x342-byte
  buffer and magic, clamps its input event count to a runtime maximum, then
  writes eight-byte entries beginning at offset `+0x6A`. Each converted entry
  has a WORD at offset 0, a WORD at +2, and a DWORD at +4; construction leaves
  the upper two bytes of that DWORD zero. The inverse reads eight-byte entries
  beginning at +0x6A and stops on an empty entry or its runtime event bound. It
  interprets flags in the +2 WORD and maps the first WORD through conversion
  tables. It copies the low two bytes of the DWORD at +4 into two output-event
  bytes. A separate flagged path emits event type `5` and preserves the first
  WORD as two bytes. These offsets can be reconciled with the validator if the
  constructor's first emitted entry is the leading code checked at +0x6A and
  its later entries are the validator's scan beginning at +0x72. The code does
  not prove that interpretation or show how the inverse treats the leading
  entry relative to validation. The `0xF0`–`0xF6` codes are accepted by the
  validator, but conversions do not establish named action semantics for all
  seven codes, delay units, or mouse-button mappings. They do not resolve the
  separate six-byte EEPROM records at `0x4C`.
- A separate key/function conversion at `0x0040f2e0` is called from a profile
  assignment path at `0x0040ff24`. It reads a selector byte at descriptor `+3`
  and uses the low 24 bits of the first DWORD plus further descriptor fields to
  return a packed value. Its branches map several selector/value ranges to
  constants or consult a lookup helper; a diagnostic names an unresolved
  macro-ID lookup. The neighboring `0x0040f580` helper emits variable-length
  byte sequences from descriptor flags and key-like bytes using a lookup table
  at `0x005b0c30`. These paths establish code-table conversions and encoded
  sequence structure, but do not label the values as particular mouse buttons
  or provide reliable UI-action names for `.jmm` codes `0xF0`–`0xF6`.
- The successful import branch does not decode records individually in the
  inspected path: it passes the decoded block to `0x0044b170`, which allocates
  `0x342` bytes and copies `0xD0` DWORDs plus one WORD from the source before
  adding the copy to the linked list through `0x0042bac0`. The importer then
  passes it to window-message helpers at `0x0044c7f0` and `0x0044ca00`, and
  reaches a file-writing helper at `0x00421080`. That helper uses `0x004217b0`
  to rotate a complete `0x342`-byte buffer right by two bits and passes the
  block to `WriteFile` (`0x00421152`). This whole-block copy/serialization path
  does not associate any record ID with a key, delay, or other event meaning.
  This is a bounded trace of the importer path, not proof that no other runtime
  consumer decodes these records.
- The imported heap copy is addressable through the macro-list lookup at
  `0x0044b200`: the caller at `0x0040ff39–0x0040ff5d` supplies a macro ID, gets
  the matching object pointer, and passes it directly to
  `StMacro_To_HdMacro` (`0x0040fab0`). This establishes that the inverse
  converter can consume the same 0x342-byte linked-list object created by the
  importer. It does not resolve the differing treatment of the leading slot:
  validator checks it as a code and scans from `+0x72`, while the converter
  iterates from `+0x6A`.
- The unrelated `0x110x` immediate values previously noted at message/API call
  sites are not the `.jmm` record IDs; the validator's compared record codes are
  `0xF0`–`0xF6`. No mapping from those seven codes to named OEM UI actions has
  been established.
- A further numeric conversion trace narrows the event-code behavior without
  naming the actions. For source event type 4, `HdMacro_To_StMacro`
  (`0x40F8F3–0x40F927`) subtracts one from the source byte, looks up a dispatch
  index in the table at `0x40FA40`, then jumps through `0x40FA28`. Source
  values `1, 2, 4, 8` select handlers that write `.jmm` WORD codes `0xF0`,
  `0xF1`, `0xF2`, and `0xF4`, respectively. Source value `16` reaches a
  diagnostic path; this trace does not establish an emitted code for it.
  This is numeric conversion evidence only. The validator maps `0xF0`–`0xF6`
  to mask bits `0x01, 0x02, 0x04, 0x08, 0x10, 0x20, 0x40` at
  `0x420C20–0x420C83`; the observed initialization stores `0x1F` to global
  `0x607A34` at `0x419F6F`, enabling `0xF0`–`0xF4` in that initialization.
  A bounded literal-address scan found this as the only direct store, which
  does not exclude a computed runtime update. In `StMacro_To_HdMacro`,
  `0xF0`, `0xF1`, and `0xF2` select handlers that emit byte values `2`, `1`, and
  `4`; `0xF3`–`0xF6` route to the generic table lookup at `0x5B0C30`. A bounded
  scan found none of those four codes in that table's key-byte positions. Its
  separate `flags & 2` path emits event type `5` and preserves the two code
  bytes, but their action meaning remains unknown. These `.jmm` codes must not
  be conflated with the separate profile-descriptor display resolver's
  `0xF4`–`0xF6` labels.
- The `.jma` filter at `0x005b11e4` is used by the `ExportProfile:` path
  (`0x0043cbb0–0x0043ce1b`); that path assembles a buffer and passes size
  `0x28CC` to a helper that calls `WriteFile` (`0x0043cd37–0x0043cd4d`, helper
  `0x0045b25b`). The `ImportProfile:` path uses the same filter
  (`0x0043ce20–0x0043d1de`) and requests `0x28CC` bytes through a helper that
  calls `ReadFile` (`0x0043cfaf–0x0043cfc5`, helper `0x0045b219`). These are
  observed fixed transfer lengths, not evidence that shorter or extended files
  are accepted.
- The importer checks the first DWORD of the read buffer for `0xFCAA1234` and
  rejects a zero DWORD at offset `+4` (`0x0043d001–0x0043d01a`). It then walks
  three groups of sixteen entries, advancing 0x18 bytes per entry
  (`0x0043d020–0x0043d0a6`). For entries selected by a byte test and matching
  macro references, it passes 0x342-byte blocks to `0x0043d360`; that helper
  checks the block's first DWORD for `.jmm` decoded magic `0xFACB99A0`, calls
  the existing macro-copy/rotation helper, and writes a 0x342-byte `.jmm` file
  (`0x0043d360–0x0043d46b`). This establishes that the `.jma` import path
  contains or references embedded `.jmm`-format macro blocks. The exact block
  offsets, meaning of the nonzero DWORD at `+4`, entry fields, macro-count
  semantics, and behavior for invalid/truncated files remain unknown. There is
  no sample `.jma` to compare with the serialized output.
- No `.jmm` or `.jma` sample was available to confirm the inferred byte layout.
  Daxa Control continues to import/export only its documented JSON formats; it
  does not claim OEM macro-file compatibility.

- The binary imports `HidD_SetFeature`, `HidD_GetAttributes`, and HID capability
  functions. GNU `objdump` identifies it as PE32/i386 with entry point `0x0054d843`.
- Static disassembly of the report writer at `0x0044f2a0` shows a 17-byte feature
  report, report ID `0x08`, and a checksum byte at offset `0x10`; the helper makes
  up to three total `HidD_SetFeature` calls, pausing 50 ms after each failed call.
- The writer copies a 15-byte caller payload to report offsets `0x01–0x0F` and
  calculates offset `0x10` from the zero-filled request buffer as `0x55` minus
  the sum of bytes at offsets `0x00` through `0x0F`, modulo 256. The observed
  setup and battery requests therefore encode as `08 03` followed by 14 zero
  bytes and `4A`, and `08 04` followed by 14 zero bytes and `49`. This confirms
  those request bytes statically; it does not confirm a receiver response or
  any setting command.
- Separate callers pass commands `0x03` and `0x0E` through the same send-and-wait
  path. Nearby diagnostic strings identify them as `GetConnectStatus` and
  `GetCurProfile`; the response parser matches the requested command ID against
  the first byte after the raw input prefix. The original reader seeds a 17-byte
  input buffer with `0x09` and uses overlapped `ReadFile`, matching the transport
  prefix used by the Rust reader.
- The helper at `0x44e590` builds a 15-byte payload with command `0x10` at
  payload offset 0, selector `1` at offset 4, and the caller's low byte at
  offset 5; the remaining payload bytes are zero. The common writer prefixes
  report ID `0x08` and computes byte 16 as `0x55 - sum(report[0..0x0F])`, so a
  profile ID `p` produces `[08 10 00 00 00 01 p 00 00 00 00 00 00 00 00 00
  (3C-p)]`. The helper waits for a matching `0x10` response with status `0`. A
  profile-selection UI handler calls it at `0x412f5a` with a field from the
  selected profile entry. A later live diagnostic selected ID 1, received the
  expected ACK and `0x0E` profile response, and then restored and verified ID 0.
- The save path enters `0x40fde0` from `0x409479`. A bounded direct disassembly
  of `0x40fde0–0x410a30` shows 14 direct calls to `WriteEEPROM` (`0x44e670`)
  and no direct call to the profile-selection helper `0x44e590`. The observed
  `0x10` call at `0x412f5a` is in a separate UI handler. A live comparison now
  confirms that DPI bytes `0x0C–0x23` differ between active profile IDs 0 and 1;
  it establishes profile dependence for this sampled DPI region, not every
  EEPROM address or write path. Keep settings writes disabled until a changed
  value can be written and read back safely.
- The `GetCurProfile` wrapper at `0x44e3f0` sends `0x0E` through the generic
  response helper. That helper checks the response status byte for `0` before
  copying reply data to the caller; the wrapper returns the first copied data
  byte. Following the same `0x09` input prefix and response layout used by the
  battery handler, the Rust monitor validates raw report byte `0x02` as status
  and reads the first payload byte at `0x06`. A later wired diagnostic observed
  successful `0x0E` replies for IDs 0 and 1. The monitor does not change local
  profile selection; its profile telemetry remains read-only.
- The `GetConnectStatus` wrapper at `0x44e360` sends `0x03` through that same
  response helper and returns its first copied data byte. The helper accepts
  the matching command only when the logical response status is `0`; with the
  `0x09` input prefix, that status is raw offset `0x02` and the first reply byte
  is raw offset `0x06`. Its caller at `0x43cb79` compares the returned byte to
  `1` and shows the OEM offline message for any other value. This establishes
  `1` as the OEM's online result and `0` as its offline result; semantics for
  other values are not established. A later wired diagnostic observed a
  status-zero `0x03` reply with online value `1`.
- The OEM response helper at `0x44e460` reads `[OPT] Retry` using
  `GetPrivateProfileIntW`; the supplied `Cfg.ini` omits that key, so the
  default literal `3` is used. Each outer attempt sends a fresh request through
  `0x44f2a0` and then waits through `0x44d790`. The send helper makes up to
  three total `HidD_SetFeature` calls, with 50 ms pauses after failures; the response
  helper waits about 1,030 ms. Daxa Control now makes up to three read-only
  `0x03` attempts, with a 1,010 ms response window per attempt. It sends once
  per attempt and ignores unrelated reports while waiting. Its total wait is
  slightly shorter than the shipped OEM default; this is static comparison,
  not evidence of a device response.
- After a validated online status, Rust queries battery and then the current
  profile.
- The original battery handler requires response status byte `0` before reading
  percent and charging. With the `0x09` input prefix, those fields are at raw
  report offsets `0x06` and `0x07`; the Rust parser now applies the same status
  check and field offsets. The `0x0E` profile parser uses the same status offset
  and takes its first reply data byte from raw offset `0x06`, matching the
  original query wrapper's first-data-byte behavior; actual mouse response
  contents remain unverified.
- The original `WriteEEPROM` routine at `0x0044e670` sends logical command
  `0x07` and waits for a matching response. Static callers use start addresses
  `0x0C` (DPI values), `0x2C` (DPI colors), `0x4C` (DPI effect), and `0x60`
  (key matrix). These call sites expose write regions, not complete field
  encodings or proof that a packet is safe on this mouse firmware.
- The helper at `0x44e9d0` constructs a logical EEPROM-read payload beginning
  `[0x08, 0x00, address_hi, address_lo, length]`, sends it through the common
  15-byte report writer, then waits for response command `0x08` with status
  `0` before copying the requested bytes to its caller. The OEM profile parser
  invokes it in ten-byte chunks, and a settings-save caller reads seven bytes
  from address `0xA0` before writing that block back. The latter sets byte 0 to
  `0x04` and recalculates byte 6 as `0x55 - sum(bytes 0..5)`; the meaning of
  this block is not identified here. These are static control-flow findings;
  the later multi-collection diagnostic captured nine live reads covering the
  polling, DPI, and DPI-color ranges documented below. Daxa Control's GUI
  monitor still does not issue command `0x08`.
- Static reconstruction of its chunk builder suggests a 15-byte command payload:
  `[0x07, 0x00, address_hi, address_lo, chunk_length, data...]`. The address is
  advanced per chunk. A zero chunk-size argument defaults to 10 bytes; the
  cited DPI, DPI-color, and key-matrix callers pass 8-byte chunks. The routine
  does not show a cap on nonzero chunk sizes. This is a disassembly-derived
  packet layout only. At the time of this static trace, no write packet had
  been sent. A later opt-in same-value `0x07` request at `0x000C`/8 received a
  status-zero ACK and immediate identical readback. A changed-value stage-6 DPI
  round trip at `0x001C`/8 later received ACK and exact changed readback, then
  restored and verified the original bytes. Power-cycle persistence and other
  write regions remain unverified. The separate live profile comparison
  establishes profile-dependent contents for DPI addresses `0x0C–0x23`, but not
  other regions or general write routing.
- Static reconstruction of the response loop at `0x44d790` shows it matches
  the requested command ID, then the `WriteEEPROM` caller checks the next
  response byte for status `0`. This supports the expected response shape after
  the `0x09` input prefix. The same-value test captured a status-zero response;
  the changed-value DPI round trip later captured both a status-zero response
  and the expected changed bytes before exact restoration.
- The polling-rate translator at `0x44f030–0x44f085` maps stored values
  `0x0125`, `0x0250`, `0x0500`, and `0x1000` to interval bytes `8`, `4`, `2`,
  and `1`, then writes each byte plus its `0x55` checksum at address `0x00`.
  `text.xml` labels this setting “Polling Rate” and `Cfg.ini` defaults `DR` to
  `0x1000`; interpreting the encoded values as 125/250/500/1000 Hz is an
  inference from those assets and the matching 8/4/2/1 ms intervals, not a live
  device observation.
- The save path at `0x4108d2–0x410905` divides the `Sleep Time` value by 10 and
  writes the quotient as one byte at EEPROM address `0xAD`, followed by its
  `0x55` complement through the common EEPROM writer. The read/log path at
  `0x40e9ae–0x40e9c6` reads one byte and multiplies it by 10 before logging
  `Sleep Time`. `text.xml` labels this control “Dormancy time” and its unit
  “Second”. Together, these paths establish a 10-second storage increment and
  a representable byte range of 0–2550 seconds. The semantic meaning of zero
  and the OEM UI's accepted minimum/maximum remain unconfirmed; this does not
  establish receiver acceptance or a safe write path.
- The original advanced-settings path at `0x427320–0x4274b4` changes Windows
  pointer preferences separately from the mouse's HID/EEPROM settings. It reads
  `SPI_GETMOUSE` (`0x03`) and writes `SPI_SETMOUSE` (`0x04`) with three integer
  values; the reconstructed “Improve pointer precision” toggle selects
  `[6, 10, 1]` when enabled and `[0, 0, 0]` when disabled. The same path reads
  and writes pointer speed with `SPI_GETMOUSESPEED`/`SPI_SETMOUSESPEED`
  (`0x70`/`0x71`), double-click time with `GetDoubleClickTime` and
  `SPI_SETDOUBLECLICKTIME` (`0x20`), and wheel lines with
  `SPI_GETWHEELSCROLLLINES`/`SPI_SETWHEELSCROLLLINES` (`0x68`/`0x69`). The
  setters pass flags `3`, matching `SPIF_UPDATEINIFILE | SPIF_SENDCHANGE`, so
  the original UI updates Windows-wide preferences and broadcasts the change.
  `text.xml` labels these “Double-click speed”, “Mouse sensitivity”, “Scrolling
  speed”, “Scroll one page”, and “Improve pointer precision”. This path is
  independent of mouse profile EEPROM and is safe to implement through Windows
  APIs, but changing it affects the user's Windows input behavior globally.
  The corresponding API parameter contract is documented in Microsoft's
  [SystemParametersInfoW reference](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-systemparametersinfow).
- For sensor `0x3395`, the DPI conversion helper maps values below `0x65F4`
  (26,100) to `floor(DPI / 50) - 1`; values at or above that threshold use
  `floor(DPI / 100) - 1` and sets an axis range flag. The DPI serializer builds
  a four-byte record per stage. Static disassembly at `0x44ec60–0x44ed80` stores
  the full low byte of converted X and Y, then packs converted high bits into
  byte 2. It reads the X range flag for both packed flag positions; the Y range
  result is not read in that packing block. Byte 3 is `0x55` minus the preceding
  three bytes. The stage block starts at `0x0C` and uses 8-byte chunks. The
  static encoding can be reconstructed, but the unusual X/Y flag behavior and
  device response still need verification before enabling writes.
- Static disassembly at `0x44eb40–0x44ebc3` reconstructs each DPI color record
  as RGB bytes followed by `0x55 - R - G - B`, with four-byte records starting
  at `0x2C` and 8-byte chunks. The one-stage caller uses the same four-byte
  stride. `Cfg.ini` supplies RGB defaults, supporting the channel order. This
  proves the serializer layout in the binary, not receiver acceptance or which
  active profile/address context receives a write.
- Original diagnostic strings label profile regions `0x00–0x0B`, DPI `0x0C–0x2B`,
  DPI color `0x2C–0x4B`, DPI effect `0x4C–0x53`, key matrix `0x60–0x9F`, and two
  further regions `0xA0–0xA7` and `0xA9–0xB4`. These labels identify intended
  regions; the traces below establish direction or field structure only where
  stated. Remaining meanings, address context, and write semantics need
  confirmation.
- The `0x4C` load branch reads six bytes (`0x43ca3c–0x43ca7e`) and passes them
  to `0x40ef00`. That parser stores byte 0 and byte 4 as separate state values,
  and maps byte 2 through the ten-byte table at `0x5db560`, returning the
  matching zero-based table index plus one. It ignores the intervening bytes 1,
  3, and 5. The save path (`0x41044c–0x4104a7`) writes either `[0, 0x55]` at
  `0x4C` when the byte-0 state is zero, or six bytes formed as three
  value/complement pairs: state byte 0, `0x55 - byte0`, table value selected
  from state at `+0x784`, its `0x55` complement, state byte 4, and its `0x55`
  complement. Thus the loader/saver pair identifies three stored data positions
  (4C, 4E, 50), with checksum/complement positions between them. It does not
  label those positions as brightness, effect, speed, or a named lighting zone;
  it also does not read/write the remaining two addresses in the diagnostic
  `0x4C–0x53` span in this path.
- The UI sync path connects these three stored values to controls without
  revealing their labels. At `0x4088B8` it reads state `+0x77C` and performs a
  linked-list/helper lookup; state `+0x784` and `+0x780` are passed to generic
  widget updater `0x443F70` for UI objects at `EDI+0x1DF8C` and `EDI+0x1F530`
  (`0x408912–0x408927`). Alternate handler `0x437939` sends `+0x784` and
  `+0x780` to UI objects at `EDI+0x19790` and `EDI+0x1AD34`. Sync routine
  `0x437C80` copies a child-control value into model `+0x77C` and derives
  `+0x784`/`+0x780` from control range data. The inspected paths contain no
  label strings or direct field-to-label binding, so this proves state/control
  flow only; brightness, LED effect, and speed assignments remain unresolved.
- The binary diagnostic labels the span “DPI Effect”; `text.xml` separately
  includes the UI labels “DPI Effect”, “Brightness”, “LED Effect”, “Speed”,
  “Predefined Color”, and “Turn off light when moving”, plus seven named LED
  modes. `Cfg.ini` supplies `LEDParam` and `LedOpt1`–`LedOpt7` arrays. These
  labels/defaults establish available vocabulary and configuration keys, but
  the inspected loader/saver has no demonstrated mapping from any of those
  arrays or controls to the `0x4C` data positions. The numeric lookup table
  alone does not establish what its values mean. No live `0x4C` lighting data
  or firmware acceptance has been observed.
- The profile-load dispatch at `0x43c9b1–0x43ca26` reads ten bytes from
  `0x54` and passes them to `0x40f1c0`; that parser's format string at
  `0x5a8b78` explicitly names the data `EEPROM_To_LogoRGB` and logs
  `mode`, `speed`, `light`, and three `color` bytes. Its call at `0x40f207`
  supplies byte 4 as mode, byte 8 as speed, byte 6 as light, and bytes 0–2 as
  color, in that order. The parser packs bytes 0–2 into the color value at
  `0x40f27f–0x40f294`. Thus, in this parser's 10-byte record, byte 3 complements
  the RGB sum, byte 7 complements byte 6, and byte 9 complements byte 8; these
  complement formulas are corroborated by the save path. The save branch at
  `0x41053b–0x4105ab` writes ten bytes at `0x54`: RGB at offsets 0–2, byte 3 as
  `0x55 - R - G - B`, the local byte at offset 4 and its `0x55` complement at
  offset 5, a table-selected byte at offset 6 and its `0x55` complement at 7,
  then the value from the local state at offset 8 and its `0x55` complement at
  9. The parser names the offset-4 field `mode`
  and offset-8 field `speed`; the save code's source fields and accepted value
  meanings remain unknown. The saver writes this full block only when the local
  byte at `[ebp+0x7ec]` is nonzero. In the alternate zero branch
  (`0x4104f0–0x410521`), it instead writes two bytes `[0, 0x55]` at `0x58`,
  overlapping offsets 4–5 of the ten-byte record; the two branch writes are
  alternatives, not a single combined record. The parser also maps some values
  through a lookup table at `0x5db590`, but its user-facing effect names cannot
  be recovered from this table alone.
- MainRGB has a separate, unresolved load/save length difference. The
  profile-load caller requests eight bytes at `0xA0` (`0x43c94a–0x43c977`) and
  passes that buffer to `0x40ef50`. The parser's format string labels `mode`,
  `speed`, `light`, and three `color` channels; argument construction maps
  these to offsets 0, 4, 5, and 1–3. Its field accesses use bytes 0–5: byte 0
  selects a conversion branch, bytes 1–3 are packed into the logged color,
  and bytes 4–5 are mapped through lookup tables. It logs the whole `0xA0–0xA7`
  range through `0x40b110` but does not use bytes 6–7 in the inspected parser
  (`0x40ef50–0x40f1bf`). The save path reads seven bytes at `0xA0` and writes
  seven (`0x4102ea–0x41035c`); its alternate branch also writes seven
  (`0x410378–0x4103e8`). Both pass a buffer pointer and length 7 to the writer,
  and the inspected setup shows no offset that turns either write into an
  eight-byte operation. This proves an eight-byte load versus seven-byte save
  operations, but not the role of the trailing byte(s) or why the lengths
  differ. Treating the eighth loaded byte as reserved or ignored by firmware
  would be speculation. The INI initializer at `0x449e1a–0x449e79` reads
  `LEDParam` with fallback `3,10,8,255,0,255`; profile initialization at
  `0x44b581–0x44b5cd` places these in local state as mode 3, speed 10, light 8,
  and RGB `FF00FF`. This establishes the local default order and color, not the
  device's accepted encoding or physical zone. No sample EEPROM or live
  response is available to settle the length relationship; firmware
  acceptance and safe-write behavior remain unverified.
- The key-matrix serializer at `0x44eef0`, called from the profile-save path at
  `0x41018d`, forms each row from the low three bytes of a 32-bit input and a
  fourth byte equal to `0x55` minus those three bytes. Its per-slot path checks
  a mask across 16 entries and writes a selected four-byte row at
  `0x60 + 4 * slot`; the batch path writes sequential rows beginning at
  `0x60`. This matches the logged `0x60–0x9F` region as 16 four-byte rows.
  It establishes row stride and checksum construction in the OEM serializer,
  not the meanings of the three data bytes or how `Cfg.ini` records map to these
  rows. The key-matrix addresses in this region have not been read from the
  live device.

## Live read-only observation via the receiver

On 2026-11-19, Daxa Control's debug build enumerated receiver PID `0xFA7C`
with four vendor-defined HID collections: `FF01/0000`, `FF02/0002`,
`FF03/0000`, and `FF04/0002` (usage page/usage). A 17-byte `GET_FEATURE_REPORT`
with report ID `0x08` succeeded only on `FF02/0002`; the other three collections
returned Windows I/O error 1. The monitor therefore selected a single endpoint.

On that endpoint, the read-only setup `SET_FEATURE_REPORT` for logical command
`0x03` returned success, but the following overlapped input read produced no
matching `0x03` response before the 1.01-second deadline. The monitor stopped
there, so it did not issue the current-profile (`0x0E`) or battery (`0x04`)
queries. No EEPROM read or configuration write was sent. This confirms the
dongle exposes the candidate collection and accepts the setup send at the
Windows API boundary; it does not prove that the receiver processed the
command. Cable-mode behavior remains unverified.

A later debug run with the retry path enabled emitted repeated setup-timeout
messages while the same receiver remained enumerated. It still captured no
valid `0x03` reply and did not issue profile or battery queries; no write was
sent. This confirms the retry path ran, not that the mouse responded.

On a later user-assisted cable trial, the receiver dongle was removed and the
mouse was connected by USB data cable and moved/clicked to wake it. Windows
still exposed `VID_25A7&PID_FA7B`; the running app continued to report
“Receiver detected · mouse did not respond” and “Battery unavailable.” The
direct-wired identity is not established, and no validated `0x03` reply was
captured. The app therefore did not issue current-profile or battery queries;
no EEPROM read or configuration write was sent. This is a failed read-only
connection attempt, not evidence that the cable or mouse is defective.

On a subsequent run on 2026-11-19, the user reconfirmed the USB data cable was
connected and the dongle was removed. Windows still exposed
`VID_25A7&PID_FA7B`. The updated Rust release, which makes three bounded
read-only `0x03` attempts, ended in “Receiver detected · mouse did not respond”
and “Battery unavailable.” The app sets this state only after all three
attempts complete without a `0x03` report. No current-profile or battery query
was issued; no EEPROM read or configuration write was sent. This records the
app's observed result and does not establish whether the cable, mouse, or
firmware caused the missing response.

On a later user-authorized multi-collection diagnostic run, Daxa Control and
OemDrv were not running. `GET_FEATURE_REPORT` with report ID `0x08` qualified
only PID `0xFA7B`, collection `FF02/0002`, interface 1 as the feature endpoint.
The diagnostic armed input reads on all four collections, sent the checksummed
`0x03` status request through `FF02/0002`, and received a validated 17-byte
reply on `FF01/0000`: `09 03 00 00 00 01 01 00 00 00 00 00 00 00 00 00 47`.
Status was zero and the online byte at raw offset 6 was `1`. This confirms that
the wired identity uses different observed input and feature collections; it
does not prove the dongle PID `0xFA7C` uses the same layout.

The same read-only diagnostic sent current-profile request `0x0E` and battery
request `0x04` through `FF02/0002`, with input reads armed across all four
collections. The profile reply arrived on `FF01/0000` with length 17, status
zero, and value zero at raw offset 6. Static inspection of OEM `GetCurProfile`
at `0x44E3F0` shows helper failure returns `0xFFFFFFFF`, while success
zero-extends the first payload byte (`0x44E428–0x44E452`). Its startup caller
rejects only values above 3 (`0x4393A2–0x4393A5`), so profile IDs `0`–`3` are
accepted and zero is not an unavailable sentinel. The zero-based slot
interpretation is likely, but the human-facing label for ID 0 is not proven.
The Rust parser now accepts IDs `0`–`3`, including zero. The battery reply was
validated on `FF01/0000` as 100%. No EEPROM read or configuration write was sent.

A subsequent bounded read-only capture issued nine separate EEPROM reads while
current profile ID 0 was reported, covering polling, DPI, and DPI-color bytes
through `FF02/0002`; responses arrived on `FF01/0000` with status zero. The
polling pair at `0x00` was `01 54`.
The 32 DPI bytes from `0x0C–0x2B` were `1F 1F 00 17 1F 1F 00 17 19 19 00 23
2F 2F 00 F7 3F 3F 00 D7 7F 7F 00 57 EF EF 00 77 7B 7B 44 1B`. The 32 color
bytes from `0x2C–0x4B` were `FF 00 00 56 00 00 FF 56 00 FF 00 56 FF FF 00 57
00 FF FF 57 FF 00 FF 57 00 FF FF 57 FF FF FF 58`. The DPI and color values
have the statically reconstructed four-byte record/checksum structure; the
polling pair stores its interval byte and complement. Interval byte `1` maps to
1000 Hz in the OEM translator. This capture records data returned while profile
ID 0 was active; by itself it did not prove that EEPROM addresses are scoped to
that profile.

After the user approved a temporary profile check, an initial diagnostic packet
with incorrectly placed `0x10` fields received no ACK; the follow-up profile
query still reported ID 0, and no settings bytes were written. Static
stack-offset review corrected the report layout before a second attempt. The
corrected packet selected profile ID 1 with status-zero `0x10` ACK and a
validated `0x0E` response reporting ID 1. Its DPI bytes at `0x0C–0x23` were
`07 07 00 47 0F 0F 00 37 1F 1F 00 17 2F 2F 00 F7 3F 3F 00 D7 7F 7F 00 57`,
which differ from profile 0's captured values. The diagnostic selected profile
ID 0 again and verified the `0x0E` response before exiting. It issued only
EEPROM reads during this check; it sent no `0x07` writes. This establishes that
the sampled DPI region is profile-dependent for IDs 0 and 1. It does not test
profiles 2–3, the DPI-color region, or whether a changed write persists.

After the user authorized a settings test, the diagnostic pre-read `0x0C–0x13`
and confirmed it still matched `1F 1F 00 17 1F 1F 00 17`. It then sent exactly
one same-value `0x07` write to address `0x000C`, length 8, received an ACK with
status zero on `FF01/0000`, and read back the same eight bytes. This validates
the request/ACK/readback path for that unchanged block; because the value was
not changed, it does not prove persistence of a changed setting or establish
whether the address is scoped to profile ID 0. The app's write capability
remains disabled.

After the user authorized a reversible changed-value test, the developer-only
`--dpi-roundtrip-test` ran over the USB data cable with the dongle removed.
Before the run, no F1, Daxa Control, or OEM driver process was found, so the
diagnostic did not overlap the game. On profile ID 0 it read
`3F 3F 00 D7 7F 7F 00 57` from `0x001C`/8, wrote the temporary stage-6 value
`3F 3F 00 D7 7D 7D 00 5B`, received ACK status 0, and read back those exact
changed bytes. Restore attempt 1 wrote the saved original, and the final read
matched `3F 3F 00 D7 7F 7F 00 57` exactly. This verifies the changed-value
write, immediate readback, and exact restoration for this one profile-0 DPI
chunk (stage 6, 6400→6300→6400 DPI). It does not verify power-cycle
persistence, the other DPI chunks or profiles, color, polling, buttons,
lighting, or production Apply. The diagnostic did not open a UI or control the
cursor, and no setting remained changed. The app's general write capability
remains disabled.

## Unverified and intentionally disabled

The earlier dongle trace enumerated PID `0xFA7C` and produced no validated
setup response. Initial single-endpoint cable trials also failed, but the later
multi-collection trial confirmed PID `0xFA7B` online and battery at 100% through
separate input and feature collections. OEM accepts profile ID zero, although
its human-facing label mapping remains unknown. The Rust parser now accepts the
OEM's 0–3 range. The corrected `0x10` profile-select request was acknowledged
for IDs 1 and 0, and the sampled DPI region differed between IDs 0 and 1; the
diagnostic ended with ID 0 active. A same-value DPI write at `0x000C` was
acknowledged and matched by immediate readback. A changed-value stage-6 DPI
chunk at `0x001C` was then acknowledged and read back exactly before the saved
bytes were restored and verified. This validates one changed chunk, not general
settings apply or persistence after power cycling. The Rust app now exposes explicit onboard profile switching through the Profiles page, using a 0x10 request and 0x0E readback verification. This does not apply local profile settings or write EEPROM; the integrated GUI path and profile IDs 2 and 3 remain unverified. It still does not send DPI, button, macro or lighting changes.

Still unknown: whether other EEPROM regions share the sampled DPI region's
profile scope, and whether changed data survives a power cycle. The original
write payload does not contain a profile ID. A successful
`GetCurProfile` response alone is only a readiness guard; the DPI-region
comparison adds evidence for this address range. Exact changed-value
write/readback/restoration is now verified for one stage-6 DPI chunk on profile
0 only; keep the general settings-write capability disabled until the remaining
device write paths and encodings are verified.

## Reproducing the static inspection

With `rea-agents` 6.7.0 and GNU binutils available:

```powershell
npx -y rea-agents@6.7.0 inspect-pe-resources 'D:\Daxa Air Mini Wireless\OemDrv.exe' --format md
npx -y rea-agents@6.7.0 inspect-managed-members 'D:\Daxa Air Mini Wireless\OemDrv.exe' --format md
objdump.exe -h 'D:\Daxa Air Mini Wireless\OemDrv.exe'
objdump.exe -d -Mintel --start-address=0x44f2a0 --stop-address=0x44f350 'D:\Daxa Air Mini Wireless\OemDrv.exe'
```

The disassembly is a reconstruction of machine code, not original source. Its
interpretation is limited to the ranges shown above.
