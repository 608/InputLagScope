//! Settings the PC keeps for the tester. The tester keeps both tables in RAM only,
//! so InputLagScope stores them as JSON in the application data directory
//! and writes them to the tester at the start of every session.
//!
//! * vendor-code table: shared by all testers (it mirrors the Windows
//!   registry `usbflags`, which is per PC), at most 64 entries, keyed by
//!   (VID, PID, bcdDevice), newest entries last.
//! * calibration: per tester UUID.

use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

use super::protocol::{Calibration, VendorEntry, MAX_VENDOR_ENTRIES};

pub const APP_IDENTIFIER: &str = "dev.inputlagscope.latency";
pub const SETTINGS_FILE_NAME: &str = "tester_settings.json";
const SETTINGS_VERSION: u32 = 1;

/// c1_uncertainty remains the FS value for compatibility with old settings.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct StoredCalibration {
    #[serde(flatten)]
    pub calibration: Calibration,
    #[serde(default = "unknown_uncertainty")]
    pub c1_uncertainty_hs: u32,
}

fn unknown_uncertainty() -> u32 {
    super::protocol::C1_UNCERTAINTY_UNKNOWN
}

impl From<Calibration> for StoredCalibration {
    fn from(calibration: Calibration) -> Self {
        Self {
            calibration,
            c1_uncertainty_hs: unknown_uncertainty(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct TesterSettings {
    #[serde(default = "settings_version")]
    pub version: u32,
    #[serde(default)]
    pub vendor_table: Vec<VendorEntry>,
    #[serde(default)]
    pub calibrations: BTreeMap<String, StoredCalibration>,
}

fn settings_version() -> u32 {
    SETTINGS_VERSION
}

impl Default for TesterSettings {
    fn default() -> Self {
        Self {
            version: SETTINGS_VERSION,
            vendor_table: Vec::new(),
            calibrations: BTreeMap::new(),
        }
    }
}

/// Application data directory, the same place Tauri's `app_data_dir()`
/// resolves to (`%APPDATA%\dev.inputlagscope.latency` on Windows). It is
/// computed without the Tauri runtime so the headless CLI uses the same file.
pub fn default_settings_dir() -> PathBuf {
    let base = std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("XDG_DATA_HOME").map(PathBuf::from))
        .or_else(|| {
            std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local").join("share"))
        })
        .unwrap_or_else(|| PathBuf::from("."));
    base.join(APP_IDENTIFIER)
}

pub fn default_settings_path() -> PathBuf {
    default_settings_dir().join(SETTINGS_FILE_NAME)
}

pub struct SettingsStore {
    path: PathBuf,
    data: Mutex<TesterSettings>,
    load_error: Option<String>,
}

impl SettingsStore {
    /// Loads the file; a missing file gives empty settings. A broken file
    /// is not overwritten until the next successful save and the error is
    /// kept in `load_error`.
    pub fn open(path: impl Into<PathBuf>) -> Self {
        let path = path.into();
        let (data, load_error) = match fs::read_to_string(&path) {
            Ok(text) => match serde_json::from_str::<TesterSettings>(&text) {
                Ok(settings) => (settings, None),
                Err(error) => (
                    TesterSettings::default(),
                    Some(format!("failed to parse {}: {error}", path.display())),
                ),
            },
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                (TesterSettings::default(), None)
            }
            Err(error) => (
                TesterSettings::default(),
                Some(format!("failed to read {}: {error}", path.display())),
            ),
        };
        Self {
            path,
            data: Mutex::new(data),
            load_error,
        }
    }

    pub fn open_default() -> Self {
        Self::open(default_settings_path())
    }

    pub fn load_error(&self) -> Option<&str> {
        self.load_error.as_deref()
    }

    pub fn vendor_table(&self) -> Vec<VendorEntry> {
        self.data.lock().vendor_table.clone()
    }

    /// Merges entries learned by the tester (GET_VENDOR_TABLE) into the
    /// stored table and saves it when something changed. Returns whether
    /// the table changed.
    pub fn merge_vendor_table(&self, learned: &[VendorEntry]) -> Result<bool> {
        let changed = {
            let mut data = self.data.lock();
            let merged = merge_vendor_entries(&data.vendor_table, learned);
            let changed = merged != data.vendor_table;
            data.vendor_table = merged;
            changed
        };
        if changed {
            self.save()?;
        }
        Ok(changed)
    }

    pub fn save(&self) -> Result<()> {
        let json = serde_json::to_string_pretty(&*self.data.lock())?;
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create {}", parent.display()))?;
        }
        let tmp = self.path.with_extension("json.tmp");
        fs::write(&tmp, json).with_context(|| format!("failed to write {}", tmp.display()))?;
        fs::rename(&tmp, &self.path)
            .with_context(|| format!("failed to replace {}", self.path.display()))?;
        Ok(())
    }
}

/// Valid entry states of the vendor table: 1 vendor code present,
/// 2 no MS OS descriptor.
fn valid_entry(entry: &VendorEntry) -> bool {
    matches!(entry.state, 1 | 2)
}

/// Merges `learned` into `stored`: entries with the same key are replaced
/// and moved to the end (newest), new keys are appended, invalid states are
/// dropped, and the oldest entries are dropped beyond 64.
pub fn merge_vendor_entries(stored: &[VendorEntry], learned: &[VendorEntry]) -> Vec<VendorEntry> {
    let mut merged = dedupe_vendor_entries(stored);
    for entry in learned.iter().filter(|entry| valid_entry(entry)) {
        if let Some(position) = merged.iter().position(|item| item.key() == entry.key()) {
            if merged[position] == *entry {
                continue;
            }
            merged.remove(position);
        }
        merged.push(entry.clone());
    }
    if merged.len() > MAX_VENDOR_ENTRIES {
        let excess = merged.len() - MAX_VENDOR_ENTRIES;
        merged.drain(0..excess);
    }
    merged
}

/// Removes invalid states and duplicate keys (the last occurrence wins),
/// because SET_VENDOR_TABLE answers BAD_PARAM for a duplicate key.
pub fn dedupe_vendor_entries(entries: &[VendorEntry]) -> Vec<VendorEntry> {
    let mut out: Vec<VendorEntry> = Vec::with_capacity(entries.len());
    for entry in entries.iter().filter(|entry| valid_entry(entry)) {
        if let Some(position) = out.iter().position(|item| item.key() == entry.key()) {
            out.remove(position);
        }
        out.push(entry.clone());
    }
    if out.len() > MAX_VENDOR_ENTRIES {
        let excess = out.len() - MAX_VENDOR_ENTRIES;
        out.drain(0..excess);
    }
    out
}
