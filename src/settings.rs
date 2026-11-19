//! Persistent, local mouse profiles.
//!
//! These values describe the UI configuration only. They are not sent to the
//! mouse until its configuration protocol has been verified.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

const SETTINGS_FILE: &str = "settings.json";
pub const MAX_PROFILES: usize = 12;
const MAX_PROFILE_NAME_CHARS: usize = 48;
pub const MAX_MACROS: usize = 64;
pub const MAX_MACRO_STEPS: usize = 256;
const MAX_MACRO_NAME_CHARS: usize = 64;
const MAX_MACRO_VALUE_CHARS: usize = 64;
const MAX_PROFILE_DOCUMENT_BYTES: u64 = 512 * 1024;
const PROFILE_DOCUMENT_FORMAT: &str = "daxa-control-profile";
const MACRO_DOCUMENT_FORMAT: &str = "daxa-control-macros";
const MAX_MACRO_DOCUMENT_BYTES: u64 = 4 * 1024 * 1024;
const SETTINGS_SCHEMA_VERSION: u32 = 3;
const PROFILE_DOCUMENT_SCHEMA_VERSION: u32 = 2;
const MAX_LOCAL_DORMANCY_SECONDS: u32 = 60 * 60;

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MacroStepKind {
    KeyDown,
    KeyUp,
    MouseButtonDown,
    MouseButtonUp,
    Wheel,
    #[default]
    #[serde(other)]
    Delay,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MacroStep {
    pub kind: MacroStepKind,
    /// Key name, mouse button, or wheel direction depending on `kind`.
    pub value: String,
    /// Delay after the step in milliseconds.
    pub delay_ms: u32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MacroDefinition {
    pub id: u64,
    pub name: String,
    pub steps: Vec<MacroStep>,
    pub repeat_count: u16,
    pub repeat_mode: MacroRepeatMode,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MacroRepeatMode {
    #[default]
    Count,
    UntilRelease,
    UntilAnyKey,
}

impl Default for MacroDefinition {
    fn default() -> Self {
        Self {
            id: 0,
            name: "New macro".to_owned(),
            steps: Vec::new(),
            repeat_count: 1,
            repeat_mode: MacroRepeatMode::Count,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ProfileSettings {
    pub name: String,
    pub button_bindings: [usize; 6],
    pub button_details: [String; 6],
    /// Local macro references for button assignments. Macro IDs are not device IDs.
    pub button_macro_ids: [Option<u64>; 6],
    pub dpi_stages: [u32; 6],
    pub dpi_y_stages: Option<[u32; 6]>,
    pub dpi_independent: bool,
    pub dpi_colors: [[u8; 3]; 6],
    pub debounce_ms: u32,
    pub polling_hz: u32,
    pub lift_off_distance_mm: u32,
    pub ripple_control: bool,
    pub angle_snapping: bool,
    pub lighting_effect: usize,
    pub brightness_percent: f32,
    pub speed_percent: f32,
    pub main_color: [u8; 3],
    pub logo_effect: usize,
    pub logo_brightness_percent: f32,
    pub logo_speed_percent: f32,
    pub logo_color: [u8; 3],
    pub turn_off_lighting_while_moving: bool,
    /// Local profile value in seconds, normalized to 10-second steps.
    /// The statically reconstructed EEPROM byte can represent at most 2550s,
    /// but this is an encoding bound only, not a verified accepted device
    /// range. Device writes remain disabled until that behavior is confirmed.
    pub dormancy_seconds: u32,
}

impl Default for ProfileSettings {
    fn default() -> Self {
        Self {
            name: "Default".to_owned(),
            button_bindings: [0, 1, 2, 3, 4, 5],
            button_details: Default::default(),
            button_macro_ids: [None; 6],
            dpi_stages: [400, 800, 1600, 2400, 3200, 6400],
            dpi_y_stages: None,
            dpi_independent: false,
            dpi_colors: [
                [255, 0, 0],
                [0, 0, 255],
                [0, 255, 0],
                [255, 255, 0],
                [0, 255, 255],
                [255, 0, 255],
            ],
            debounce_ms: 8,
            polling_hz: 1000,
            lift_off_distance_mm: 1,
            ripple_control: false,
            angle_snapping: false,
            lighting_effect: 0,
            brightness_percent: 70.0,
            speed_percent: 50.0,
            main_color: [255, 0, 255],
            logo_effect: 0,
            logo_brightness_percent: 70.0,
            logo_speed_percent: 50.0,
            logo_color: [255, 0, 255],
            turn_off_lighting_while_moving: false,
            dormancy_seconds: 300,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AppSettings {
    pub schema_version: u32,
    pub dark_mode: bool,
    pub profiles: Vec<ProfileSettings>,
    pub active_profile: usize,
    pub macros: Vec<MacroDefinition>,
    /// Next local macro ID. `None` means the `u64` ID space is exhausted.
    pub next_macro_id: Option<u64>,
}

impl Default for AppSettings {
    fn default() -> Self {
        let mut default = ProfileSettings::default();
        let mut gaming = default.clone();
        gaming.name = "Gaming".to_owned();
        let mut work = default.clone();
        work.name = "Work".to_owned();
        default.name = "Default".to_owned();
        Self {
            schema_version: SETTINGS_SCHEMA_VERSION,
            dark_mode: false,
            profiles: vec![default, gaming, work],
            active_profile: 0,
            macros: Vec::new(),
            next_macro_id: Some(1),
        }
    }
}

impl AppSettings {
    pub fn active(&self) -> &ProfileSettings {
        static EMPTY_SETTINGS_FALLBACK: std::sync::OnceLock<ProfileSettings> =
            std::sync::OnceLock::new();
        self.profiles
            .get(self.active_profile)
            .or_else(|| self.profiles.first())
            .unwrap_or_else(|| EMPTY_SETTINGS_FALLBACK.get_or_init(ProfileSettings::default))
    }

    pub fn active_mut(&mut self) -> &mut ProfileSettings {
        if self.profiles.is_empty() {
            self.profiles.push(ProfileSettings::default());
            self.active_profile = 0;
        }
        if self.active_profile >= self.profiles.len() {
            self.active_profile = 0;
        }
        &mut self.profiles[self.active_profile]
    }

    pub fn set_active_profile(&mut self, index: usize) -> bool {
        if index < self.profiles.len() {
            self.active_profile = index;
            true
        } else {
            false
        }
    }

    pub fn add_profile(&mut self) -> Option<usize> {
        if self.profiles.len() >= MAX_PROFILES {
            return None;
        }
        let mut profile = self.active().clone();
        profile.name = format!("Profile {}", self.profiles.len() + 1);
        self.profiles.push(profile);
        self.active_profile = self.profiles.len() - 1;
        Some(self.active_profile)
    }

    pub fn remove_active_profile(&mut self) -> bool {
        if self.profiles.len() <= 1 || self.active_profile >= self.profiles.len() {
            return false;
        }
        self.profiles.remove(self.active_profile);
        self.active_profile = self.active_profile.min(self.profiles.len() - 1);
        true
    }

    pub fn macro_by_id(&self, id: u64) -> Option<&MacroDefinition> {
        self.macros.iter().find(|macro_def| macro_def.id == id)
    }

    pub fn macro_by_id_mut(&mut self, id: u64) -> Option<&mut MacroDefinition> {
        self.macros.iter_mut().find(|macro_def| macro_def.id == id)
    }

    pub fn add_macro(&mut self, name: impl AsRef<str>) -> Option<u64> {
        if self.macros.len() >= MAX_MACROS {
            return None;
        }
        let id = self.allocate_macro_id()?;
        let name = normalize_name(name.as_ref(), "New macro", MAX_MACRO_NAME_CHARS);
        self.macros.push(MacroDefinition {
            id,
            name,
            ..MacroDefinition::default()
        });
        Some(id)
    }

    pub fn rename_macro(&mut self, id: u64, name: impl AsRef<str>) -> bool {
        let name = name.as_ref().trim();
        if name.is_empty() {
            return false;
        }
        let Some(macro_def) = self.macro_by_id_mut(id) else {
            return false;
        };
        macro_def.name = normalize_name(name, "New macro", MAX_MACRO_NAME_CHARS);
        true
    }

    pub fn remove_macro(&mut self, id: u64) -> bool {
        let Some(index) = self.macros.iter().position(|macro_def| macro_def.id == id) else {
            return false;
        };
        self.macros.remove(index);
        true
    }

    pub fn add_macro_step(&mut self, id: u64, step: MacroStep) -> bool {
        let Some(macro_def) = self.macro_by_id_mut(id) else {
            return false;
        };
        if macro_def.steps.len() >= MAX_MACRO_STEPS {
            return false;
        }
        macro_def.steps.push(normalize_step(step));
        true
    }

    pub fn remove_macro_step(&mut self, id: u64, index: usize) -> Option<MacroStep> {
        let macro_def = self.macro_by_id_mut(id)?;
        (index < macro_def.steps.len()).then(|| macro_def.steps.remove(index))
    }

    fn allocate_macro_id(&mut self) -> Option<u64> {
        let mut candidate = self.next_macro_id?.max(1);
        while self
            .macros
            .iter()
            .any(|macro_def| macro_def.id == candidate)
        {
            candidate = candidate.checked_add(1)?;
        }
        self.next_macro_id = candidate.checked_add(1);
        Some(candidate)
    }

    fn normalize(&mut self) {
        self.schema_version = SETTINGS_SCHEMA_VERSION;
        self.profiles.truncate(MAX_PROFILES);
        if self.profiles.is_empty() {
            self.profiles.push(ProfileSettings::default());
        }
        if self.active_profile >= self.profiles.len() {
            self.active_profile = 0;
        }

        for (index, profile) in self.profiles.iter_mut().enumerate() {
            profile.name = normalize_name(
                &profile.name,
                &format!("Profile {}", index + 1),
                MAX_PROFILE_NAME_CHARS,
            );

            for binding in &mut profile.button_bindings {
                *binding = (*binding).min(40);
            }
            for detail in &mut profile.button_details {
                *detail = detail.chars().take(64).collect();
            }
            for dpi in &mut profile.dpi_stages {
                let value = (*dpi).clamp(100, 26_000);
                *dpi = ((value + 50) / 100) * 100;
            }
            if profile.dpi_independent && profile.dpi_y_stages.is_none() {
                profile.dpi_y_stages = Some(profile.dpi_stages);
            }
            if let Some(stages) = &mut profile.dpi_y_stages {
                for dpi in stages {
                    *dpi = (((*dpi).clamp(100, 26_000) + 50) / 100) * 100;
                }
            }
            profile.debounce_ms = profile.debounce_ms.min(30);
            profile.polling_hz = match profile.polling_hz {
                125 | 250 | 500 | 1000 => profile.polling_hz,
                _ => 1000,
            };
            profile.lift_off_distance_mm = match profile.lift_off_distance_mm {
                1 | 2 => profile.lift_off_distance_mm,
                _ => 1,
            };
            profile.lighting_effect = profile.lighting_effect.min(6);
            if !profile.brightness_percent.is_finite() {
                profile.brightness_percent = 70.0;
            }
            profile.brightness_percent = profile.brightness_percent.clamp(0.0, 100.0);
            if !profile.speed_percent.is_finite() {
                profile.speed_percent = 50.0;
            }
            profile.speed_percent = profile.speed_percent.clamp(0.0, 100.0);
            profile.logo_effect = profile.logo_effect.min(6);
            if !profile.logo_brightness_percent.is_finite() {
                profile.logo_brightness_percent = 70.0;
            }
            if !profile.logo_speed_percent.is_finite() {
                profile.logo_speed_percent = 50.0;
            }
            profile.logo_brightness_percent = profile.logo_brightness_percent.clamp(0.0, 100.0);
            profile.logo_speed_percent = profile.logo_speed_percent.clamp(0.0, 100.0);
            profile.dormancy_seconds = ((profile
                .dormancy_seconds
                .clamp(0, MAX_LOCAL_DORMANCY_SECONDS)
                + 5)
                / 10)
                * 10;
        }

        self.macros.truncate(MAX_MACROS);
        let mut used_ids = std::collections::HashSet::with_capacity(self.macros.len());
        let mut next_id = 1_u64;
        for (index, macro_def) in self.macros.iter_mut().enumerate() {
            while macro_def.id == 0 || used_ids.contains(&macro_def.id) {
                while used_ids.contains(&next_id) {
                    next_id = next_id.saturating_add(1);
                }
                macro_def.id = next_id;
                next_id = next_id.saturating_add(1);
            }
            used_ids.insert(macro_def.id);
            macro_def.name = normalize_name(
                &macro_def.name,
                &format!("Macro {}", index + 1),
                MAX_MACRO_NAME_CHARS,
            );
            macro_def.repeat_count = macro_def.repeat_count.clamp(1, 999);
            macro_def.steps.truncate(MAX_MACRO_STEPS);
            for step in &mut macro_def.steps {
                *step = normalize_step(step.clone());
            }
        }

        // Keep macro references local and stable. Missing IDs from older
        // settings can migrate from an unambiguous macro name; an existing but
        // unresolved ID remains unavailable instead of binding to another
        // macro that later happens to reuse its display name.
        let macros = &self.macros;
        for profile in &mut self.profiles {
            for index in 0..profile.button_bindings.len() {
                if profile.button_bindings[index] != 23 {
                    profile.button_macro_ids[index] = None;
                    continue;
                }

                if let Some(id) = profile.button_macro_ids[index] {
                    if let Some(macro_def) = macros.iter().find(|item| item.id == id) {
                        profile.button_details[index] = macro_def.name.clone();
                    }
                    continue;
                }

                let detail = &profile.button_details[index];
                let mut matches = macros.iter().filter(|item| item.name == *detail);
                if let Some(macro_def) = matches.next() {
                    if matches.next().is_none() {
                        profile.button_macro_ids[index] = Some(macro_def.id);
                        profile.button_details[index] = macro_def.name.clone();
                    }
                }
            }
        }

        let max_macro_id = self.macros.iter().map(|macro_def| macro_def.id).max();
        self.next_macro_id = match (self.next_macro_id, max_macro_id) {
            (None, _) => None,
            (Some(next), Some(maximum)) => maximum
                .checked_add(1)
                .map(|minimum| next.max(minimum).max(1)),
            (Some(next), None) => Some(next.max(1)),
        };
    }
}

fn normalize_name(value: &str, fallback: &str, max_chars: usize) -> String {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        fallback.chars().take(max_chars).collect()
    } else {
        trimmed.chars().take(max_chars).collect()
    }
}

fn normalize_step(mut step: MacroStep) -> MacroStep {
    step.value = step
        .value
        .trim()
        .chars()
        .take(MAX_MACRO_VALUE_CHARS)
        .collect();
    step.delay_ms = step.delay_ms.min(60_000);
    step
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ProfileDocument {
    pub format: String,
    pub schema_version: u32,
    pub profile: ProfileSettings,
}

impl ProfileDocument {
    fn new(profile: ProfileSettings) -> Self {
        Self {
            format: PROFILE_DOCUMENT_FORMAT.to_owned(),
            schema_version: PROFILE_DOCUMENT_SCHEMA_VERSION,
            profile,
        }
    }
}

/// Read settings from either the current schema or the earlier minute-based
/// schema without treating future documents as data that can be overwritten.
fn parse_settings_document(bytes: &[u8]) -> Result<AppSettings, String> {
    let mut document: Value = serde_json::from_slice(bytes).map_err(|error| error.to_string())?;
    let schema_version = document
        .get("schema_version")
        .and_then(Value::as_u64)
        .unwrap_or(1);
    if schema_version > u64::from(SETTINGS_SCHEMA_VERSION) {
        return Err(format!(
            "Settings schema version {schema_version} is newer than supported version {SETTINGS_SCHEMA_VERSION}"
        ));
    }

    // Only v1 stored dormancy in minutes. Version 2 already stores seconds;
    // the v2 -> v3 change adds a macro-ID counter and macro references only.
    if schema_version < 2 {
        if let Some(profiles) = document.get_mut("profiles").and_then(Value::as_array_mut) {
            for profile in profiles {
                migrate_legacy_dormancy(profile)?;
            }
        }
    }

    if let Some(object) = document.as_object_mut() {
        object.insert(
            "schema_version".to_owned(),
            Value::from(SETTINGS_SCHEMA_VERSION),
        );
    }
    serde_json::from_value(document).map_err(|error| error.to_string())
}

/// Convert a version-1 profile's minute value into the new second-based field.
fn migrate_legacy_dormancy(profile: &mut Value) -> Result<(), String> {
    let Some(object) = profile.as_object_mut() else {
        return Ok(());
    };
    let Some(minutes) = object.remove("dormancy_minutes") else {
        return Ok(());
    };
    if object.contains_key("dormancy_seconds") {
        return Ok(());
    }
    let Some(minutes) = minutes.as_u64() else {
        return Err("Legacy dormancy_minutes must be an unsigned integer".to_owned());
    };
    object.insert(
        "dormancy_seconds".to_owned(),
        Value::from(minutes.saturating_mul(60)),
    );
    Ok(())
}

/// Parse profile interchange documents while preserving the original version
/// number for the caller's compatibility check.
fn parse_profile_document(bytes: &[u8]) -> Result<ProfileDocument, String> {
    let mut document: Value = serde_json::from_slice(bytes).map_err(|error| error.to_string())?;
    let schema_version = document
        .get("schema_version")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    if schema_version == 1 {
        if let Some(profile) = document.get_mut("profile") {
            migrate_legacy_dormancy(profile)?;
        }
    } else if schema_version != u64::from(PROFILE_DOCUMENT_SCHEMA_VERSION) {
        return Err(format!("Unsupported profile version {schema_version}"));
    }
    serde_json::from_value(document).map_err(|error| error.to_string())
}

// A strict interchange format keeps unknown action types from silently turning
// into delay steps. IDs are local and deliberately omitted from exports.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct MacroDocument {
    format: String,
    schema_version: u32,
    macros: Vec<MacroRecord>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct MacroRecord {
    name: String,
    steps: Vec<MacroRecordStep>,
    repeat_count: u16,
    repeat_mode: MacroRepeatMode,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct MacroRecordStep {
    kind: MacroRecordKind,
    value: String,
    delay_ms: u32,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum MacroRecordKind {
    KeyDown,
    KeyUp,
    MouseButtonDown,
    MouseButtonUp,
    Wheel,
    Delay,
}

impl From<MacroStepKind> for MacroRecordKind {
    fn from(kind: MacroStepKind) -> Self {
        match kind {
            MacroStepKind::KeyDown => Self::KeyDown,
            MacroStepKind::KeyUp => Self::KeyUp,
            MacroStepKind::MouseButtonDown => Self::MouseButtonDown,
            MacroStepKind::MouseButtonUp => Self::MouseButtonUp,
            MacroStepKind::Wheel => Self::Wheel,
            MacroStepKind::Delay => Self::Delay,
        }
    }
}

impl From<MacroRecordKind> for MacroStepKind {
    fn from(kind: MacroRecordKind) -> Self {
        match kind {
            MacroRecordKind::KeyDown => Self::KeyDown,
            MacroRecordKind::KeyUp => Self::KeyUp,
            MacroRecordKind::MouseButtonDown => Self::MouseButtonDown,
            MacroRecordKind::MouseButtonUp => Self::MouseButtonUp,
            MacroRecordKind::Wheel => Self::Wheel,
            MacroRecordKind::Delay => Self::Delay,
        }
    }
}

impl MacroRecord {
    fn from_macro(item: &MacroDefinition) -> Self {
        Self {
            name: item.name.clone(),
            steps: item
                .steps
                .iter()
                .map(|step| MacroRecordStep {
                    kind: step.kind.into(),
                    value: step.value.clone(),
                    delay_ms: step.delay_ms,
                })
                .collect(),
            repeat_count: item.repeat_count,
            repeat_mode: item.repeat_mode,
        }
    }

    fn validate(&self) -> Result<(), String> {
        let name = self.name.trim();
        if name.is_empty()
            || name.chars().count() > MAX_MACRO_NAME_CHARS
            || name.chars().any(char::is_control)
        {
            return Err(
                "Macro names must be 1–64 characters without control characters".to_owned(),
            );
        }
        if self.steps.len() > MAX_MACRO_STEPS {
            return Err(format!("A macro can have at most {MAX_MACRO_STEPS} steps"));
        }
        if !(1..=999).contains(&self.repeat_count) {
            return Err("Repeat count must be between 1 and 999".to_owned());
        }
        for (index, step) in self.steps.iter().enumerate() {
            if step.value.chars().count() > MAX_MACRO_VALUE_CHARS
                || step.value.chars().any(char::is_control)
                || (!matches!(step.kind, MacroRecordKind::Delay) && step.value.trim().is_empty())
                || step.delay_ms > 60_000
            {
                return Err(format!(
                    "Invalid value or delay in step {} of ‘{}’",
                    index + 1,
                    name
                ));
            }
        }
        Ok(())
    }
}

/// Owns a settings document and its on-disk save state.
pub struct SettingsStore {
    pub settings: AppSettings,
    last_saved: AppSettings,
    path: Option<PathBuf>,
    status: String,
    malformed_file_pending_backup: bool,
    malformed_backup: Option<PathBuf>,
}

impl SettingsStore {
    pub fn load() -> Self {
        let path = match settings_path() {
            Ok(path) => path,
            Err(error) => {
                let settings = AppSettings::default();
                return Self {
                    last_saved: settings.clone(),
                    settings,
                    path: None,
                    status: format!("Settings are in memory only · {error}"),
                    malformed_file_pending_backup: false,
                    malformed_backup: None,
                };
            }
        };

        match fs::read(&path) {
            Ok(bytes) => match parse_settings_document(&bytes) {
                Ok(mut settings) => {
                    let loaded = settings.clone();
                    settings.normalize();
                    let changed = loaded != settings;
                    Self {
                        settings: settings.clone(),
                        last_saved: if changed { loaded } else { settings.clone() },
                        path: Some(path),
                        status: if changed {
                            "Loaded and repaired saved settings".to_owned()
                        } else {
                            "Settings loaded".to_owned()
                        },
                        malformed_file_pending_backup: false,
                        malformed_backup: None,
                    }
                }
                Err(_) => {
                    let settings = AppSettings::default();
                    Self {
                        last_saved: settings.clone(),
                        settings,
                        path: Some(path),
                        status: "Saved settings could not be read; defaults loaded. The original will be backed up before saving".to_owned(),
                        malformed_file_pending_backup: true,
                        malformed_backup: None,
                    }
                }
            },
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                let settings = AppSettings::default();
                Self {
                    last_saved: settings.clone(),
                    settings,
                    path: Some(path),
                    status: "Using default settings · changes save locally".to_owned(),
                    malformed_file_pending_backup: false,
                    malformed_backup: None,
                }
            }
            Err(error) => {
                let settings = AppSettings::default();
                Self {
                    last_saved: settings.clone(),
                    settings,
                    path: Some(path),
                    status: format!("Settings could not be read · defaults loaded · {error}"),
                    malformed_file_pending_backup: false,
                    malformed_backup: None,
                }
            }
        }
    }

    pub fn status(&self) -> &str {
        &self.status
    }

    /// Export one macro or the complete local library in a versioned document.
    pub fn export_macros(
        &self,
        destination: impl AsRef<Path>,
        selected: Option<u64>,
    ) -> Result<(), String> {
        let records: Vec<MacroRecord> = match selected {
            Some(id) => vec![MacroRecord::from_macro(
                self.settings
                    .macro_by_id(id)
                    .ok_or_else(|| "Selected macro no longer exists".to_owned())?,
            )],
            None => self
                .settings
                .macros
                .iter()
                .map(MacroRecord::from_macro)
                .collect(),
        };
        if records.is_empty() {
            return Err("There are no macros to export".to_owned());
        }
        for record in &records {
            record.validate()?;
        }
        let document = MacroDocument {
            format: MACRO_DOCUMENT_FORMAT.to_owned(),
            schema_version: 1,
            macros: records,
        };
        let bytes = serde_json::to_vec_pretty(&document)
            .map_err(|error| format!("Macros could not be encoded · {error}"))?;
        if bytes.len() as u64 > MAX_MACRO_DOCUMENT_BYTES {
            return Err("Macro document exceeds the 4 MiB size limit".to_owned());
        }
        atomic_write_bytes(destination.as_ref(), &bytes)
            .map_err(|error| format!("Macros could not be exported · {error}"))
    }

    /// Validate the entire import before adding new IDs; roll back if saving fails.
    pub fn import_macros(
        &mut self,
        source: impl AsRef<Path>,
        single: bool,
    ) -> Result<Vec<u64>, String> {
        let mut file = File::open(source.as_ref())
            .map_err(|error| format!("Macro document could not be opened · {error}"))?;
        let size = file
            .metadata()
            .map_err(|error| format!("Macro document could not be inspected · {error}"))?
            .len();
        if size > MAX_MACRO_DOCUMENT_BYTES {
            return Err("Macro document exceeds the 4 MiB size limit".to_owned());
        }
        let mut bytes = Vec::with_capacity(size as usize);
        Read::take(&mut file, MAX_MACRO_DOCUMENT_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|error| format!("Macro document could not be read · {error}"))?;
        if bytes.len() as u64 > MAX_MACRO_DOCUMENT_BYTES {
            return Err("Macro document exceeds the 4 MiB size limit".to_owned());
        }
        let document: MacroDocument = serde_json::from_slice(&bytes)
            .map_err(|error| format!("Macro document is invalid · {error}"))?;
        if document.format != MACRO_DOCUMENT_FORMAT || document.schema_version != 1 {
            return Err("Unsupported Daxa Control macro format or version".to_owned());
        }
        if document.macros.is_empty()
            || document.macros.len() > MAX_MACROS
            || self.settings.macros.len() + document.macros.len() > MAX_MACROS
        {
            return Err(format!(
                "Import must contain at least one macro and fit the {MAX_MACROS}-macro library limit"
            ));
        }
        if single && document.macros.len() != 1 {
            return Err(
                "Choose a document containing exactly one macro, or use Import library".to_owned(),
            );
        }
        for record in &document.macros {
            record.validate()?;
        }

        let previous = self.settings.clone();
        let mut ids = Vec::with_capacity(document.macros.len());
        for record in document.macros {
            let Some(id) = self.settings.allocate_macro_id() else {
                self.settings = previous;
                return Err("Could not assign a local macro ID".to_owned());
            };
            self.settings.macros.push(MacroDefinition {
                id,
                name: record.name.trim().to_owned(),
                steps: record
                    .steps
                    .into_iter()
                    .map(|step| MacroStep {
                        kind: step.kind.into(),
                        value: step.value.trim().to_owned(),
                        delay_ms: step.delay_ms,
                    })
                    .collect(),
                repeat_count: record.repeat_count,
                repeat_mode: record.repeat_mode,
            });
            ids.push(id);
        }
        if let Err(error) = self.save_if_dirty() {
            self.settings = previous;
            return Err(error);
        }
        self.status = "Macros imported and saved locally".to_owned();
        Ok(ids)
    }

    /// Export only the active mouse profile. The global macro library remains
    /// in the local settings file and is not bundled into profile documents.
    pub fn export_active_profile(&self, destination: impl AsRef<Path>) -> Result<(), String> {
        let document = ProfileDocument::new(self.settings.active().clone());
        let bytes = serde_json::to_vec_pretty(&document)
            .map_err(|error| format!("Profile could not be encoded · {error}"))?;
        if bytes.len() as u64 > MAX_PROFILE_DOCUMENT_BYTES {
            return Err("Profile document exceeds the size limit".to_owned());
        }
        atomic_write_bytes(destination.as_ref(), &bytes)
            .map_err(|error| format!("Profile could not be exported · {error}"))
    }

    /// Import a bounded, versioned profile document, add it as a new profile,
    /// select it, and persist the updated settings store.
    pub fn import_profile(&mut self, source: impl AsRef<Path>) -> Result<usize, String> {
        if self.settings.profiles.len() >= MAX_PROFILES {
            return Err(format!("Profile limit reached ({MAX_PROFILES})"));
        }

        let mut file = File::open(source.as_ref())
            .map_err(|error| format!("Profile could not be opened · {error}"))?;
        let size = file
            .metadata()
            .map_err(|error| format!("Profile could not be inspected · {error}"))?
            .len();
        if size > MAX_PROFILE_DOCUMENT_BYTES {
            return Err("Profile document exceeds the 512 KiB size limit".to_owned());
        }
        let mut bytes = Vec::with_capacity(size as usize);
        Read::take(&mut file, MAX_PROFILE_DOCUMENT_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|error| format!("Profile could not be read · {error}"))?;
        if bytes.len() as u64 > MAX_PROFILE_DOCUMENT_BYTES {
            return Err("Profile document exceeds the 512 KiB size limit".to_owned());
        }

        let mut document: ProfileDocument = parse_profile_document(&bytes)
            .map_err(|error| format!("Profile document is invalid · {error}"))?;
        if document.format != PROFILE_DOCUMENT_FORMAT {
            return Err("This is not a Daxa Control profile".to_owned());
        }
        if document.schema_version != 1
            && document.schema_version != PROFILE_DOCUMENT_SCHEMA_VERSION
        {
            return Err(format!(
                "Unsupported profile version {}",
                document.schema_version
            ));
        }

        let mut normalized = AppSettings {
            schema_version: SETTINGS_SCHEMA_VERSION,
            dark_mode: self.settings.dark_mode,
            profiles: vec![document.profile],
            active_profile: 0,
            macros: Vec::new(),
            next_macro_id: Some(1),
        };
        // Profile exports can carry IDs from a different local macro library.
        // Keep the visible names and re-link only through normalize's unique
        // exact-name migration against this installation's macros.
        normalized.profiles[0].button_macro_ids = [None; 6];
        normalized.normalize();
        document.profile = normalized.profiles.remove(0);

        let previous = self.settings.clone();
        self.settings.profiles.push(document.profile);
        self.settings.active_profile = self.settings.profiles.len() - 1;
        let imported_index = self.settings.active_profile;
        if let Err(error) = self.save_if_dirty() {
            self.settings = previous;
            return Err(error);
        }
        self.status = "Profile imported and saved locally".to_owned();
        Ok(imported_index)
    }

    /// Write only when edited settings differ from the last successful save.
    pub fn save_if_dirty(&mut self) -> Result<bool, String> {
        self.settings.normalize();
        if self.settings == self.last_saved {
            return Ok(false);
        }
        let Some(path) = self.path.as_ref() else {
            let message = "Settings path is unavailable".to_owned();
            self.status = format!("Settings are not saved · {message}");
            return Err(message);
        };

        if self.malformed_file_pending_backup && self.malformed_backup.is_none() {
            match backup_malformed_file(path) {
                Ok(backup) => self.malformed_backup = Some(backup),
                Err(error) => {
                    self.status =
                        format!("Settings are not saved · recovery copy failed · {error}");
                    return Err(error.to_string());
                }
            }
        }

        match save_atomic(path, &self.settings) {
            Ok(()) => {
                self.last_saved = self.settings.clone();
                self.status = if let Some(backup) = self.malformed_backup.take() {
                    self.malformed_file_pending_backup = false;
                    let filename = backup
                        .file_name()
                        .and_then(|name| name.to_str())
                        .unwrap_or("recovery copy");
                    format!("Saved locally · original kept as {filename}")
                } else {
                    "Saved locally".to_owned()
                };
                Ok(true)
            }
            Err(error) => {
                self.status = format!("Settings are not saved · {error}");
                Err(error.to_string())
            }
        }
    }
}

/// Copy an unreadable settings file before the first user edit replaces it.
fn backup_malformed_file(path: &PathBuf) -> std::io::Result<PathBuf> {
    let parent = path
        .parent()
        .ok_or_else(|| std::io::Error::other("settings path has no parent directory"))?;
    let filename = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(SETTINGS_FILE);
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let backup_path = parent.join(format!("{filename}.corrupt-{}-{nonce}", std::process::id()));

    let mut source = File::open(path)?;
    let mut backup = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&backup_path)?;
    let result = (|| {
        std::io::copy(&mut source, &mut backup)?;
        backup.sync_all()?;
        Ok(())
    })();

    if result.is_err() {
        let _ = fs::remove_file(&backup_path);
    }
    result.map(|()| backup_path)
}

fn settings_path() -> Result<PathBuf, String> {
    #[cfg(target_os = "windows")]
    let root = std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .ok_or_else(|| "Windows did not provide the AppData folder".to_owned())?;

    #[cfg(not(target_os = "windows"))]
    let root = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))
        .ok_or_else(|| "No user settings folder is available".to_owned())?;

    Ok(root.join("Daxa Control").join(SETTINGS_FILE))
}

fn save_atomic(path: &PathBuf, settings: &AppSettings) -> std::io::Result<()> {
    let bytes = serde_json::to_vec_pretty(settings)
        .map_err(|error| std::io::Error::other(error.to_string()))?;
    atomic_write_bytes(path, &bytes)
}

fn atomic_write_bytes(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)?;
    let filename = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(SETTINGS_FILE);
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let temp_path = parent.join(format!(".{filename}.{}.{}.tmp", std::process::id(), nonce));

    let mut temp = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temp_path)?;
    let result = (|| {
        temp.write_all(bytes)?;
        temp.sync_all()?;
        drop(temp);
        fs::rename(&temp_path, path)?;
        if let Ok(directory) = File::open(parent) {
            let _ = directory.sync_all();
        }
        Ok(())
    })();

    if result.is_err() {
        let _ = fs::remove_file(temp_path);
    }
    result
}
