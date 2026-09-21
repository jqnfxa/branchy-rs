//! Vaults: one folder per graph, and the short list of recent ones.
//!
//! A vault is a folder. Its name is the folder's name and nothing else, so
//! renaming the folder renames the vault and the two can never disagree. Inside
//! it sits the document and a `.branchy` folder of state that belongs to this
//! device alone, which today is the undo stack:
//!
//! ```text
//! Work/
//! ├── graph.json
//! └── .branchy/
//!     └── undo/
//! ```
//!
//! The folder is also the unit phase 3 syncs. Syncthing shares a vault folder
//! and leaves `.branchy` out.
//!
//! Which vaults exist is not recorded anywhere, only which were opened
//! recently. That list is per device and lives in the user's configuration
//! directory, never inside a vault. It is read here rather than in the
//! interface so that the terminal and the window read the same one: `branchy`
//! works in the vault the window opened last.

use std::fs;
use std::path::{Path, PathBuf};

use branchy_core::Graph;
use serde::{Deserialize, Serialize};

use crate::store::{self, StoreError};

/// The document inside every vault.
pub const DOCUMENT: &str = "graph.json";

/// How many vaults the recent list remembers.
pub const RECENT_LIMIT: usize = 5;

/// Bumped when the list's shape changes in a way an older reader could not
/// cope with.
const LIST_VERSION: u32 = 1;

const LIST_FILE: &str = "vaults.json";

/// A folder holding one graph.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Vault {
    dir: PathBuf,
}

impl Vault {
    /// Creates `parent/name` holding an empty graph.
    ///
    /// An empty folder of that name is taken over; one with anything in it is
    /// refused rather than having a graph dropped among someone's files.
    ///
    /// # Errors
    ///
    /// [`StoreError::BadVaultName`] for a name that cannot be a folder name on
    /// every supported platform, [`StoreError::VaultExists`] when the folder is
    /// already in use, otherwise [`StoreError::Io`].
    pub fn create(parent: &Path, name: &str) -> Result<Self, StoreError> {
        let name = check_name(name)?;
        let dir = parent.join(name);
        match fs::read_dir(&dir) {
            Ok(mut inside) => {
                if inside.next().is_some() {
                    return Err(StoreError::VaultExists(dir));
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => fs::create_dir_all(&dir)?,
            // something that is not a folder already has the name
            Err(_) if dir.exists() => return Err(StoreError::VaultExists(dir)),
            Err(e) => return Err(e.into()),
        }
        let vault = Self::open(&dir)?;
        // written straight away, so the folder is recognisably a vault even
        // before anything is added to it
        store::save(&vault.document(), &Graph::new())?;
        Ok(vault)
    }

    /// Opens a folder as a vault.
    ///
    /// Any folder will do. One without a graph starts empty, and nothing is
    /// written to it until the first change.
    ///
    /// # Errors
    ///
    /// [`StoreError::NotAFolder`] when `dir` is missing or is a file.
    pub fn open(dir: &Path) -> Result<Self, StoreError> {
        if !dir.is_dir() {
            return Err(StoreError::NotAFolder(dir.to_path_buf()));
        }
        Ok(Self {
            dir: canonical(dir)?,
        })
    }

    /// The vault's folder, absolute.
    #[must_use]
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// The folder's name.
    #[must_use]
    pub fn name(&self) -> String {
        name_of(&self.dir)
    }

    /// The document inside it.
    #[must_use]
    pub fn document(&self) -> PathBuf {
        self.dir.join(DOCUMENT)
    }
}

/// The recent vaults on this device, newest first, and whether the window
/// should reopen the newest one on start.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Vaults {
    path: PathBuf,
    recent: Vec<PathBuf>,
    open_last: bool,
    /// Where an older version kept its graph, if this list may still adopt it.
    legacy: Option<PathBuf>,
}

/// One line of the recent list, as a front end shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Entry {
    /// The folder's name.
    pub name: String,
    /// The folder, absolute.
    pub path: String,
    /// The folder is gone: moved, renamed or deleted since it was opened.
    pub missing: bool,
}

impl Entry {
    /// Describes a vault's folder, whether or not it is on the list.
    #[must_use]
    pub fn of(dir: &Path) -> Self {
        Self {
            name: name_of(dir),
            path: dir.display().to_string(),
            missing: !dir.is_dir(),
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct ListRecord {
    version: u32,
    #[serde(default)]
    open_last: bool,
    #[serde(default)]
    recent: Vec<PathBuf>,
}

impl Vaults {
    /// The list every front end on this device reads.
    ///
    /// `BRANCHY_VAULTS` names the file when set. Otherwise it is
    /// `vaults.json` in the per-user configuration directory, and the first
    /// time there is none, a graph left by a version from before vaults
    /// becomes the first entry. With the variable set nothing is adopted:
    /// whoever set it has said where the list lives, and tests set it so that
    /// they never go looking in a real user's data.
    ///
    /// # Errors
    ///
    /// [`StoreError::NoHome`] when the platform offers no configuration
    /// directory, otherwise as [`Vaults::load`].
    pub fn user() -> Result<Self, StoreError> {
        if let Some(given) = std::env::var_os("BRANCHY_VAULTS") {
            return Self::load(Path::new(&given));
        }
        let dirs = store::project_dirs()?;
        Self::load_or_adopt(&dirs.config_dir().join(LIST_FILE), dirs.data_dir())
    }

    /// Reads a list, or starts an empty one if the file is not there yet.
    ///
    /// # Errors
    ///
    /// [`StoreError::Io`], [`StoreError::Json`], or [`StoreError::Version`]
    /// for a list written by a newer build.
    pub fn load(path: &Path) -> Result<Self, StoreError> {
        let record = match fs::read_to_string(path) {
            Ok(text) => serde_json::from_str::<ListRecord>(&text)?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => ListRecord {
                version: LIST_VERSION,
                open_last: false,
                recent: Vec::new(),
            },
            Err(e) => return Err(e.into()),
        };
        if record.version > LIST_VERSION {
            return Err(StoreError::Version(record.version));
        }
        let mut recent = record.recent;
        recent.truncate(RECENT_LIMIT);
        Ok(Self {
            path: path.to_path_buf(),
            recent,
            open_last: record.open_last,
            legacy: None,
        })
    }

    /// Reads a list; if there is none yet, `legacy` becomes its first entry
    /// when it holds a graph.
    ///
    /// Before vaults, the one document lived in the per-user data directory.
    /// Someone upgrading would otherwise find an empty list and conclude their
    /// graph was gone.
    ///
    /// # Errors
    ///
    /// As [`Vaults::load`].
    pub fn load_or_adopt(path: &Path, legacy: &Path) -> Result<Self, StoreError> {
        let exists = path.exists();
        let mut list = Self::load(path)?;
        list.legacy = Some(legacy.to_path_buf());
        if !exists && legacy.join(DOCUMENT).is_file() {
            if let Ok(vault) = Vault::open(legacy) {
                list.opened(&vault);
            }
        }
        Ok(list)
    }

    /// Reads the list again from where this one came from.
    ///
    /// The terminal changes the list too. Anything that holds on to one for
    /// longer than a command, as the window does, rereads it before changing
    /// it, or it would save its stale copy over whatever the terminal did.
    ///
    /// # Errors
    ///
    /// As [`Vaults::load`].
    pub fn reload(&self) -> Result<Self, StoreError> {
        match &self.legacy {
            Some(legacy) => Self::load_or_adopt(&self.path, legacy),
            None => Self::load(&self.path),
        }
    }

    /// Writes the list back where it was read from.
    ///
    /// # Errors
    ///
    /// [`StoreError::Io`], or [`StoreError::Json`] for a folder whose path is
    /// not valid Unicode.
    pub fn save(&self) -> Result<(), StoreError> {
        let record = ListRecord {
            version: LIST_VERSION,
            open_last: self.open_last,
            recent: self.recent.clone(),
        };
        let text = serde_json::to_string_pretty(&record)?;
        store::replace_file(&self.path, &text)?;
        Ok(())
    }

    /// Where the list is kept.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The recent vaults' folders, newest first.
    #[must_use]
    pub fn recent(&self) -> &[PathBuf] {
        &self.recent
    }

    /// The vault opened last, which is the one the terminal works in.
    #[must_use]
    pub fn current(&self) -> Option<&Path> {
        self.recent.first().map(PathBuf::as_path)
    }

    /// The recent vaults as a front end shows them.
    #[must_use]
    pub fn entries(&self) -> Vec<Entry> {
        self.recent.iter().map(|dir| Entry::of(dir)).collect()
    }

    /// Whether the window reopens the last vault on start instead of offering
    /// the list.
    #[must_use]
    pub fn open_last(&self) -> bool {
        self.open_last
    }

    /// Sets [`Vaults::open_last`].
    pub fn set_open_last(&mut self, on: bool) {
        self.open_last = on;
    }

    /// Puts a vault at the front, which also makes it current.
    ///
    /// A vault already on the list moves rather than appearing twice, and the
    /// oldest falls off once there are more than [`RECENT_LIMIT`]. Falling off
    /// the list does nothing to the folder.
    pub fn opened(&mut self, vault: &Vault) {
        self.recent.retain(|dir| dir != vault.dir());
        self.recent.insert(0, vault.dir().to_path_buf());
        self.recent.truncate(RECENT_LIMIT);
    }

    /// Finds a vault named on the command line: a recent vault's name, or any
    /// folder.
    ///
    /// # Errors
    ///
    /// [`StoreError::AmbiguousVault`] when two recent vaults share the name,
    /// [`StoreError::VaultMissing`] when the one it names is gone, and
    /// [`StoreError::UnknownVault`] when it is neither a name nor a folder.
    pub fn resolve(&self, given: &str) -> Result<Vault, StoreError> {
        match self.position(given) {
            Ok(index) => {
                let dir = &self.recent[index];
                if dir.is_dir() {
                    Vault::open(dir)
                } else {
                    Err(StoreError::VaultMissing(dir.clone()))
                }
            }
            Err(StoreError::UnknownVault(_)) if Path::new(given).is_dir() => {
                Vault::open(Path::new(given))
            }
            Err(e) => Err(e),
        }
    }

    /// Takes a vault off the list, by name or folder, and returns its folder.
    /// The folder itself is left exactly as it is.
    ///
    /// # Errors
    ///
    /// [`StoreError::AmbiguousVault`] or [`StoreError::UnknownVault`].
    pub fn forget(&mut self, given: &str) -> Result<PathBuf, StoreError> {
        let index = self.position(given)?;
        Ok(self.recent.remove(index))
    }

    fn position(&self, given: &str) -> Result<usize, StoreError> {
        let named: Vec<usize> = (0..self.recent.len())
            .filter(|&i| name_of(&self.recent[i]) == given)
            .collect();
        match named.as_slice() {
            [one] => return Ok(*one),
            [] => {}
            _ => return Err(StoreError::AmbiguousVault(given.to_string())),
        }
        let spelled = Path::new(given);
        let resolved = canonical(spelled).ok();
        self.recent
            .iter()
            .position(|dir| dir == spelled || resolved.as_deref() == Some(dir.as_path()))
            .ok_or_else(|| StoreError::UnknownVault(given.to_string()))
    }
}

/// Refuses a name that would not make a folder everywhere Branchy runs.
///
/// A vault made on Linux can later be synced to Windows, so the stricter rules
/// apply on every platform: no characters Windows reserves, no trailing dot,
/// and none of its reserved device names.
fn check_name(name: &str) -> Result<&str, StoreError> {
    let trimmed = name.trim();
    let stem = trimmed
        .split('.')
        .next()
        .unwrap_or_default()
        .to_ascii_uppercase();
    let reserved = matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || ((stem.starts_with("COM") || stem.starts_with("LPT"))
            && stem.len() == 4
            && stem.as_bytes()[3].is_ascii_digit());
    let bad = trimmed.is_empty()
        || trimmed.ends_with('.')
        || reserved
        || trimmed
            .chars()
            .any(|c| c.is_control() || r#"/\:*?"<>|"#.contains(c));
    if bad {
        Err(StoreError::BadVaultName(name.to_string()))
    } else {
        Ok(trimmed)
    }
}

fn name_of(dir: &Path) -> String {
    dir.file_name().map_or_else(
        || dir.display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    )
}

/// `fs::canonicalize`, without the `\\?\` prefix it adds on Windows, which
/// would otherwise end up in the list and on screen.
fn canonical(path: &Path) -> Result<PathBuf, StoreError> {
    let full = fs::canonicalize(path)?;
    #[cfg(windows)]
    {
        if let Some(rest) = full.to_str().and_then(|text| text.strip_prefix(r"\\?\")) {
            // only a plain drive path; UNC and device paths keep their prefix
            if rest.as_bytes().get(1) == Some(&b':') {
                return Ok(PathBuf::from(rest));
            }
        }
    }
    Ok(full)
}
