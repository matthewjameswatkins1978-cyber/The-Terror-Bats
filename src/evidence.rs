//! Content-addressed evidence store, operation log, and execution records.
//!
//! Layout (default `%LOCALAPPDATA%\TerrorBat`, override with `--store`):
//!
//! ```text
//! <root>\
//!   objects\sha256\ab\cdef...      content-addressed evidence
//!   runs\<execution-id>\
//!     manifest.json                run metadata
//!     operations.jsonl             append-only operation log
//!     receipt.json                 final receipt (M6)
//!   temp\                          scratch for atomic writes
//! ```
//!
//! Durability claim (deliberately narrow): evidence successfully finalised by
//! Terror Bats survives failure of supervised child processes. Power-loss and
//! disk-corruption survival are NOT claimed.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::error::{Error, Result};

/// A content identity for stored bytes: `evidence:sha256:<lowercase hex>`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct EvidenceRef(pub String);

impl EvidenceRef {
    pub fn from_hex(hex: &str) -> EvidenceRef {
        EvidenceRef(format!("evidence:sha256:{hex}"))
    }

    pub fn hex(&self) -> Option<&str> {
        self.0.strip_prefix("evidence:sha256:")
    }
}

impl std::fmt::Display for EvidenceRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut hex = String::with_capacity(digest.len() * 2);
    for byte in digest {
        hex.push_str(&format!("{byte:02x}"));
    }
    hex
}

/// Current unix time in milliseconds (human context only; causal ordering
/// uses monotonic elapsed durations recorded alongside).
pub fn unix_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
}

/// Minimal UTC RFC 3339 rendering of a unix-millisecond timestamp.
pub fn rfc3339_utc(unix_millis: u128) -> String {
    let secs = (unix_millis / 1000) as i64;
    let millis = (unix_millis % 1000) as u32;
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let (hour, minute, second) = (rem / 3600, (rem % 3600) / 60, rem % 60);
    // civil_from_days (Howard Hinnant's algorithm): days since 1970-01-01.
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y_raw = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if month <= 2 { y_raw + 1 } else { y_raw };
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}.{millis:03}Z")
}

/// The local evidence store.
#[derive(Debug, Clone)]
pub struct EvidenceStore {
    root: PathBuf,
}

impl EvidenceStore {
    /// Default per-user Windows location: `%LOCALAPPDATA%\TerrorBat`.
    /// Evidence is never stored inside the target repository by default:
    /// testing a repository must not mutate it.
    pub fn default_root() -> Result<PathBuf> {
        let base = dirs::data_local_dir().ok_or_else(|| {
            Error::store(
                Path::new("<store>"),
                "cannot determine the local application data directory; pass --store <path>",
            )
        })?;
        Ok(base.join("TerrorBat"))
    }

    /// Open (creating if needed) a store at `root`.
    pub fn open(root: &Path) -> Result<EvidenceStore> {
        for sub in ["objects", "runs", "campaigns", "temp"] {
            fs::create_dir_all(root.join(sub)).map_err(|e| {
                Error::store(root, format!("cannot create store directory `{sub}`: {e}"))
            })?;
        }
        Ok(EvidenceStore {
            root: root.to_path_buf(),
        })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    fn object_path(&self, hex: &str) -> PathBuf {
        self.root
            .join("objects")
            .join("sha256")
            .join(&hex[..2])
            .join(&hex[2..])
    }

    /// Store bytes content-addressed. Atomic: temp write → hash → rename.
    /// An existing object is verified, never blindly overwritten; a mismatch
    /// is corruption and an error.
    pub fn put(&self, bytes: &[u8]) -> Result<EvidenceRef> {
        let hex = sha256_hex(bytes);
        let dest = self.object_path(&hex);
        if dest.exists() {
            let existing = fs::read(&dest).map_err(|e| {
                Error::store(&dest, format!("cannot read existing evidence object: {e}"))
            })?;
            if existing != bytes {
                return Err(Error::store(
                    &dest,
                    format!(
                        "CORRUPT evidence object: stored bytes for {hex} do not match their digest; \
                         refusing to overwrite (code TB-EVIDENCE-CORRUPT)"
                    ),
                ));
            }
            return Ok(EvidenceRef::from_hex(&hex));
        }
        let tmp = self.root.join("temp").join(format!(
            "{}-{}-{}.tmp",
            std::process::id(),
            unix_ms(),
            &hex[..12]
        ));
        {
            let mut file = fs::File::create(&tmp)
                .map_err(|e| Error::store(&tmp, format!("cannot create temp object: {e}")))?;
            file.write_all(bytes)
                .and_then(|_| file.flush())
                .and_then(|_| file.sync_all())
                .map_err(|e| Error::store(&tmp, format!("cannot write temp object: {e}")))?;
        }
        // Verify the temp bytes hash to the expected digest before publishing.
        let written =
            fs::read(&tmp).map_err(|e| Error::store(&tmp, format!("cannot re-read temp: {e}")))?;
        if sha256_hex(&written) != hex {
            let _ = fs::remove_file(&tmp);
            return Err(Error::store(
                &tmp,
                "temp object failed hash verification before publish (code TB-EVIDENCE-WRITE)",
            ));
        }
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| Error::store(parent, format!("cannot create object dir: {e}")))?;
        }
        fs::rename(&tmp, &dest)
            .map_err(|e| Error::store(&dest, format!("cannot publish object: {e}")))?;
        Ok(EvidenceRef::from_hex(&hex))
    }

    /// Read an object and verify its digest. Corrupt objects are errors,
    /// never trusted bytes.
    pub fn get(&self, evidence: &EvidenceRef) -> Result<Vec<u8>> {
        let hex = evidence.hex().ok_or_else(|| {
            Error::store(
                self.root(),
                format!("malformed evidence reference `{evidence}`"),
            )
        })?;
        if hex.len() != 64 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(Error::store(
                self.root(),
                format!("malformed evidence digest in `{evidence}`"),
            ));
        }
        let path = self.object_path(hex);
        let bytes = fs::read(&path).map_err(|e| {
            Error::store(
                &path,
                format!("evidence object not found or unreadable ({evidence}): {e}"),
            )
        })?;
        if sha256_hex(&bytes) != hex {
            return Err(Error::store(
                &path,
                format!(
                    "CORRUPT evidence object: bytes at {evidence} no longer match their digest \
                     (code TB-EVIDENCE-CORRUPT)"
                ),
            ));
        }
        Ok(bytes)
    }

    pub fn exists(&self, evidence: &EvidenceRef) -> bool {
        evidence
            .hex()
            .is_some_and(|hex| self.object_path(hex).exists())
    }

    pub fn run_dir(&self, execution_id: &str) -> PathBuf {
        self.root.join("runs").join(execution_id)
    }

    /// Create a fresh run directory. Execution IDs are UUIDs: collisions are
    /// a stop-the-world error, never a silent overwrite.
    pub fn create_run_dir(&self, execution_id: &str) -> Result<PathBuf> {
        let dir = self.run_dir(execution_id);
        if dir.exists() {
            return Err(Error::store(
                &dir,
                format!("run directory for execution `{execution_id}` already exists"),
            ));
        }
        fs::create_dir_all(&dir)
            .map_err(|e| Error::store(&dir, format!("cannot create run directory: {e}")))?;
        Ok(dir)
    }

    pub fn campaign_dir(&self, campaign_uuid: &str) -> PathBuf {
        self.root.join("campaigns").join(campaign_uuid)
    }

    /// Create a fresh campaign directory. Campaign directories are UUID-named
    /// like run directories: a collision is a stop-the-world error, never a
    /// silent overwrite. Child evidence is never duplicated here — the
    /// campaign receipt only references child runs by id.
    pub fn create_campaign_dir(&self, campaign_uuid: &str) -> Result<PathBuf> {
        let dir = self.campaign_dir(campaign_uuid);
        if dir.exists() {
            return Err(Error::store(
                &dir,
                format!("campaign directory for `{campaign_uuid}` already exists"),
            ));
        }
        fs::create_dir_all(&dir)
            .map_err(|e| Error::store(&dir, format!("cannot create campaign directory: {e}")))?;
        Ok(dir)
    }

    pub fn list_campaigns(&self) -> Vec<String> {
        let mut ids = Vec::new();
        if let Ok(entries) = fs::read_dir(self.root.join("campaigns")) {
            for entry in entries.flatten() {
                if entry.path().is_dir()
                    && let Some(name) = entry.file_name().to_str()
                {
                    ids.push(name.to_string());
                }
            }
        }
        ids.sort();
        ids
    }

    pub fn list_runs(&self) -> Vec<String> {
        let mut ids = Vec::new();
        if let Ok(entries) = fs::read_dir(self.root.join("runs")) {
            for entry in entries.flatten() {
                if entry.path().is_dir()
                    && let Some(name) = entry.file_name().to_str()
                {
                    ids.push(name.to_string());
                }
            }
        }
        ids.sort();
        ids
    }

    /// True when the store can round-trip a known object (doctor probe).
    pub fn self_test(&self) -> Result<()> {
        let probe = b"terrorbat-store-self-test";
        let r = self.put(probe)?;
        let back = self.get(&r)?;
        if back != probe {
            return Err(Error::store(
                self.root(),
                "store self-test round trip mismatch",
            ));
        }
        Ok(())
    }
}

/// Append-only JSONL operation log. Entries are flushed as they happen so a
/// later crash cannot erase earlier operations.
pub struct OperationLog {
    file: fs::File,
    seq: u64,
}

impl OperationLog {
    pub fn create(run_dir: &Path) -> Result<OperationLog> {
        let path = run_dir.join("operations.jsonl");
        let file = fs::File::create(&path)
            .map_err(|e| Error::store(&path, format!("cannot create operation log: {e}")))?;
        Ok(OperationLog { file, seq: 0 })
    }

    pub fn open_existing(run_dir: &Path) -> Result<OperationLog> {
        let path = run_dir.join("operations.jsonl");
        let file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .map_err(|e| Error::store(&path, format!("cannot open operation log: {e}")))?;
        // Continue sequencing after existing lines (replay/recovery paths).
        let existing = fs::read_to_string(&path).unwrap_or_default();
        let seq = existing.lines().filter(|l| !l.trim().is_empty()).count() as u64;
        Ok(OperationLog { file, seq })
    }

    /// Append one structured record. `fields` is merged into the envelope.
    pub fn record(&mut self, event: &str, fields: serde_json::Value) -> Result<()> {
        self.seq += 1;
        let mut entry = serde_json::Map::new();
        entry.insert("seq".into(), serde_json::Value::Number(self.seq.into()));
        entry.insert("unix_ms".into(), serde_json::json!(unix_ms() as u64));
        entry.insert("event".into(), serde_json::json!(event));
        if let serde_json::Value::Object(m) = fields {
            for (k, v) in m {
                entry.insert(k, v);
            }
        }
        let line = serde_json::to_string(&serde_json::Value::Object(entry))
            .map_err(|e| Error::store(Path::new("<oplog>"), format!("cannot serialise: {e}")))?;
        writeln!(self.file, "{line}")
            .and_then(|_| self.file.flush())
            .map_err(|e| {
                Error::store(
                    Path::new("<oplog>"),
                    format!("cannot append operation log entry: {e}"),
                )
            })?;
        Ok(())
    }
}
