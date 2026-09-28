use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};
use thiserror::Error;
use tracing::{debug, error, info, warn};
use walkdir::WalkDir;

static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

#[derive(Error, Debug)]
pub enum AtomicError {
    #[error("I/O error at {path}: {source}")]
    IoWithPath {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("I/O error: {0}")]
    Io(#[from] io::Error),
    #[error("Target path has no parent directory: {0}")]
    NoParentDirectory(PathBuf),
    #[error("Target path is invalid: {0}")]
    InvalidPath(String),
    #[error("Swap operation failed: {0}")]
    SwapFailed(String),
    #[error("Rollback failed: {0}")]
    RollbackFailed(String),
    #[error("JSON serialization error: {0}")]
    Json(#[from] serde_json::Error),
}

/// Guard that deletes a temporary file on drop unless explicitly disarmed.
struct TempFileGuard<'a> {
    path: &'a Path,
    active: bool,
}

impl<'a> TempFileGuard<'a> {
    fn new(path: &'a Path) -> Self {
        Self { path, active: true }
    }

    fn disarm(&mut self) {
        self.active = false;
    }
}

impl<'a> Drop for TempFileGuard<'a> {
    fn drop(&mut self) {
        if self.active && self.path.exists() {
            let _ = fs::remove_file(self.path);
        }
    }
}

/// Generates a unique temporary file path in the same directory as the target path.
/// Placing the temporary file on the same filesystem ensures that `fs::rename` is atomic.
fn generate_temp_path(target_path: &Path) -> Result<PathBuf, AtomicError> {
    let parent = target_path
        .parent()
        .map(|p| {
            if p.as_os_str().is_empty() {
                Path::new(".")
            } else {
                p
            }
        })
        .unwrap_or_else(|| Path::new("."));

    let file_name = target_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("swch_atomic");

    let pid = std::process::id();
    let counter = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);

    let temp_file_name = format!(".{}.{}_{}_{}.tmp", file_name, pid, nanos, counter);
    Ok(parent.join(temp_file_name))
}

/// Atomically writes raw byte contents to `target_path`.
///
/// Steps:
/// 1. Creates parent directories if they do not exist.
/// 2. Writes data to a temporary file located in the exact same directory (same filesystem).
/// 3. Flushes and explicitly calls `sync_all()` (`fsync`) to guarantee data is committed to disk.
/// 4. Closes the temporary file handle before renaming (essential for Windows handle locking).
/// 5. Executes an atomic `fs::rename` from the temporary file to `target_path`.
/// 6. Commits directory entry metadata on Unix platforms via directory fsync.
/// 7. Automatically cleans up the temporary file if any step fails.
pub fn atomic_write<P: AsRef<Path>, C: AsRef<[u8]>>(
    target_path: P,
    contents: C,
) -> Result<(), AtomicError> {
    let target = target_path.as_ref();
    let contents = contents.as_ref();

    let parent = target
        .parent()
        .map(|p| {
            if p.as_os_str().is_empty() {
                Path::new(".")
            } else {
                p
            }
        })
        .unwrap_or_else(|| Path::new("."));

    if !parent.exists() {
        fs::create_dir_all(parent).map_err(|e| AtomicError::IoWithPath {
            path: parent.to_path_buf(),
            source: e,
        })?;
    }

    let temp_path = generate_temp_path(target)?;
    let mut guard = TempFileGuard::new(&temp_path);

    // Write contents to temporary file
    {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp_path)
            .map_err(|e| AtomicError::IoWithPath {
                path: temp_path.clone(),
                source: e,
            })?;

        file.write_all(contents)
            .map_err(|e| AtomicError::IoWithPath {
                path: temp_path.clone(),
                source: e,
            })?;

        file.flush().map_err(|e| AtomicError::IoWithPath {
            path: temp_path.clone(),
            source: e,
        })?;

        // Fsync to ensure durability against power loss or kernel crashes
        file.sync_all().map_err(|e| AtomicError::IoWithPath {
            path: temp_path.clone(),
            source: e,
        })?;
    } // File is closed here so Windows allows rename

    // Preserve permissions from existing file if present
    if let Ok(meta) = fs::metadata(target) {
        let _ = fs::set_permissions(&temp_path, meta.permissions());
        #[cfg(windows)]
        {
            let mut perms = meta.permissions();
            if perms.readonly() {
                perms.set_readonly(false);
                let _ = fs::set_permissions(target, perms);
            }
        }
    }

    // Atomic rename
    fs::rename(&temp_path, target).map_err(|e| AtomicError::IoWithPath {
        path: target.to_path_buf(),
        source: e,
    })?;

    // On Unix, sync parent directory to ensure directory entry persistence
    #[cfg(unix)]
    {
        if let Ok(dir_file) = File::open(parent) {
            let _ = dir_file.sync_all();
        }
    }

    guard.disarm();
    debug!("Atomic write succeeded for {}", target.display());
    Ok(())
}

/// Atomically writes a string slice to `target_path`.
pub fn atomic_write_str<P: AsRef<Path>>(target_path: P, content: &str) -> Result<(), AtomicError> {
    atomic_write(target_path, content.as_bytes())
}

/// Atomically serializes and writes a serde value as pretty-printed JSON to `target_path`.
pub fn atomic_write_json<P: AsRef<Path>, T: serde::Serialize>(
    target_path: P,
    value: &T,
) -> Result<(), AtomicError> {
    let bytes = serde_json::to_vec_pretty(value)?;
    atomic_write(target_path, bytes)
}

/// Incremental atomic file writer supporting `std::io::Write`.
/// Changes are staged in a `.tmp` file and only moved to target on `commit()`.
pub struct AtomicFileWriter {
    target_path: PathBuf,
    temp_path: PathBuf,
    file: Option<File>,
    committed: bool,
}

impl AtomicFileWriter {
    pub fn new<P: AsRef<Path>>(target_path: P) -> Result<Self, AtomicError> {
        let target = target_path.as_ref().to_path_buf();
        let parent = target
            .parent()
            .map(|p| {
                if p.as_os_str().is_empty() {
                    Path::new(".")
                } else {
                    p
                }
            })
            .unwrap_or_else(|| Path::new("."));

        if !parent.exists() {
            fs::create_dir_all(parent).map_err(|e| AtomicError::IoWithPath {
                path: parent.to_path_buf(),
                source: e,
            })?;
        }

        let temp_path = generate_temp_path(&target)?;
        let file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp_path)
            .map_err(|e| AtomicError::IoWithPath {
                path: temp_path.clone(),
                source: e,
            })?;

        Ok(Self {
            target_path: target,
            temp_path,
            file: Some(file),
            committed: false,
        })
    }

    /// Flushes, fsyncs, closes the file, and atomically renames to the target path.
    pub fn commit(mut self) -> Result<(), AtomicError> {
        if let Some(mut file) = self.file.take() {
            file.flush().map_err(|e| AtomicError::IoWithPath {
                path: self.temp_path.clone(),
                source: e,
            })?;
            file.sync_all().map_err(|e| AtomicError::IoWithPath {
                path: self.temp_path.clone(),
                source: e,
            })?;
        }

        if let Ok(meta) = fs::metadata(&self.target_path) {
            let _ = fs::set_permissions(&self.temp_path, meta.permissions());
            #[cfg(windows)]
            {
                let mut perms = meta.permissions();
                if perms.readonly() {
                    perms.set_readonly(false);
                    let _ = fs::set_permissions(&self.target_path, perms);
                }
            }
        }

        fs::rename(&self.temp_path, &self.target_path).map_err(|e| AtomicError::IoWithPath {
            path: self.target_path.clone(),
            source: e,
        })?;

        #[cfg(unix)]
        {
            let parent = self.target_path.parent().unwrap_or_else(|| Path::new("."));
            if let Ok(dir_file) = File::open(parent) {
                let _ = dir_file.sync_all();
            }
        }

        self.committed = true;
        Ok(())
    }

    /// Aborts writing and deletes the temporary file.
    pub fn abort(mut self) {
        self.file.take();
        if self.temp_path.exists() {
            let _ = fs::remove_file(&self.temp_path);
        }
        self.committed = true; // Prevent Drop from trying to remove again
    }
}

impl Write for AtomicFileWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        match &mut self.file {
            Some(f) => f.write(buf),
            None => Err(io::Error::other("AtomicFileWriter already closed")),
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        match &mut self.file {
            Some(f) => f.flush(),
            None => Ok(()),
        }
    }
}

impl Drop for AtomicFileWriter {
    fn drop(&mut self) {
        if !self.committed {
            self.file.take();
            if self.temp_path.exists() {
                let _ = fs::remove_file(&self.temp_path);
            }
        }
    }
}

/// Recursively copies an entire directory tree from `src` to `dst`.
/// Automatically detects and avoids infinite recursion if `dst` is located inside `src`.
pub fn copy_dir_all<P: AsRef<Path>, Q: AsRef<Path>>(src: P, dst: Q) -> Result<(), AtomicError> {
    let src = src.as_ref();
    let dst = dst.as_ref();

    if !dst.exists() {
        fs::create_dir_all(dst).map_err(|e| AtomicError::IoWithPath {
            path: dst.to_path_buf(),
            source: e,
        })?;
    }

    let canonical_src = fs::canonicalize(src).ok();
    let canonical_dst = fs::canonicalize(dst).ok();

    // If source and destination are identical, nothing to copy
    if let (Some(csrc), Some(cdst)) = (&canonical_src, &canonical_dst) {
        if csrc == cdst {
            return Ok(());
        }
    } else if src == dst {
        return Ok(());
    }

    // Infinite recursion can ONLY happen if dst is located inside src!
    let dst_inside_src = match (&canonical_src, &canonical_dst) {
        (Some(csrc), Some(cdst)) => cdst.starts_with(csrc),
        _ => dst.starts_with(src),
    };

    let walker = WalkDir::new(src).into_iter().filter_entry(|entry| {
        let entry_path = entry.path();
        if dst_inside_src {
            // Do not prune the root of the search
            if entry_path == src {
                return true;
            }
            // Skip the destination directory, anything inside it, and any intermediate ancestor of dst
            if entry_path == dst || entry_path.starts_with(dst) || dst.starts_with(entry_path) {
                return false;
            }
            if let (Some(ref cdst), Ok(centry)) = (&canonical_dst, fs::canonicalize(entry_path)) {
                if centry == *cdst || centry.starts_with(cdst) || cdst.starts_with(&centry) {
                    return false;
                }
            }
        }
        true
    });

    for entry in walker {
        let entry = entry.map_err(|e| AtomicError::Io(e.into()))?;
        let entry_path = entry.path();

        let rel_path = entry_path.strip_prefix(src).map_err(|e| {
            AtomicError::InvalidPath(format!("Failed to compute relative path: {}", e))
        })?;

        if rel_path.as_os_str().is_empty() {
            continue;
        }

        let target_path = dst.join(rel_path);
        let file_type = entry.file_type();

        if file_type.is_dir() {
            fs::create_dir_all(&target_path).map_err(|e| AtomicError::IoWithPath {
                path: target_path,
                source: e,
            })?;
        } else if file_type.is_file() {
            if let Some(parent) = target_path.parent() {
                if !parent.exists() {
                    fs::create_dir_all(parent).map_err(|e| AtomicError::IoWithPath {
                        path: parent.to_path_buf(),
                        source: e,
                    })?;
                }
            }
            #[cfg(windows)]
            if let Ok(meta) = fs::metadata(&target_path) {
                let mut perms = meta.permissions();
                if perms.readonly() {
                    perms.set_readonly(false);
                    let _ = fs::set_permissions(&target_path, perms);
                }
            }
            fs::copy(entry_path, &target_path).map_err(|e| AtomicError::IoWithPath {
                path: target_path,
                source: e,
            })?;
        } else if file_type.is_symlink() {
            #[cfg(unix)]
            {
                if let Ok(symlink_target) = fs::read_link(entry_path) {
                    if target_path.exists() || fs::symlink_metadata(&target_path).is_ok() {
                        let _ = fs::remove_file(&target_path);
                    }
                    let _ = std::os::unix::fs::symlink(&symlink_target, &target_path);
                }
            }
        }
    }

    Ok(())
}

/// Safely copies a file using a same-directory `.tmp` staging file and atomic rename.
/// Streams file contents without loading entire file into memory (avoids OOM on large files).
pub fn atomic_copy<P: AsRef<Path>, Q: AsRef<Path>>(
    source: P,
    target: Q,
) -> Result<(), AtomicError> {
    let src = source.as_ref();
    let tgt = target.as_ref();

    let parent = tgt
        .parent()
        .map(|p| {
            if p.as_os_str().is_empty() {
                Path::new(".")
            } else {
                p
            }
        })
        .unwrap_or_else(|| Path::new("."));

    if !parent.exists() {
        fs::create_dir_all(parent).map_err(|e| AtomicError::IoWithPath {
            path: parent.to_path_buf(),
            source: e,
        })?;
    }

    let temp_path = generate_temp_path(tgt)?;
    let mut guard = TempFileGuard::new(&temp_path);

    {
        let mut reader = File::open(src).map_err(|e| AtomicError::IoWithPath {
            path: src.to_path_buf(),
            source: e,
        })?;

        let mut writer = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp_path)
            .map_err(|e| AtomicError::IoWithPath {
                path: temp_path.clone(),
                source: e,
            })?;

        io::copy(&mut reader, &mut writer).map_err(|e| AtomicError::IoWithPath {
            path: temp_path.clone(),
            source: e,
        })?;

        writer.flush().map_err(|e| AtomicError::IoWithPath {
            path: temp_path.clone(),
            source: e,
        })?;

        writer.sync_all().map_err(|e| AtomicError::IoWithPath {
            path: temp_path.clone(),
            source: e,
        })?;
    }

    if let Ok(meta) = fs::metadata(src) {
        let _ = fs::set_permissions(&temp_path, meta.permissions());
    }

    #[cfg(windows)]
    if let Ok(meta) = fs::metadata(tgt) {
        let mut perms = meta.permissions();
        if perms.readonly() {
            perms.set_readonly(false);
            let _ = fs::set_permissions(tgt, perms);
        }
    }

    fs::rename(&temp_path, tgt).map_err(|e| AtomicError::IoWithPath {
        path: tgt.to_path_buf(),
        source: e,
    })?;

    #[cfg(unix)]
    {
        if let Ok(dir_file) = File::open(parent) {
            let _ = dir_file.sync_all();
        }
    }

    guard.disarm();
    Ok(())
}

/// Recorded backup item for transactional rollback.
#[derive(Debug, Clone)]
struct BackupEntry {
    original_path: PathBuf,
    backup_path: PathBuf,
    was_directory: bool,
    existed: bool,
}

/// Helper to clear all entries in `dir` except the specified `except` path.
fn clear_dir_except(dir: &Path, except: &Path) -> Result<(), io::Error> {
    if !dir.exists() {
        return Ok(());
    }
    let can_except = fs::canonicalize(except).ok();
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path == except || except.starts_with(&path) {
            continue;
        }
        if let (Some(ref cexcept), Ok(cpath)) = (&can_except, fs::canonicalize(&path)) {
            if cpath == *cexcept || cexcept.starts_with(&cpath) {
                continue;
            }
        }
        if path.is_dir() {
            fs::remove_dir_all(&path)?;
        } else {
            fs::remove_file(&path)?;
        }
    }
    Ok(())
}

/// Transactional session that backs up files or directories and provides rollback capabilities.
///
/// If dropped without calling `commit()`, the session automatically triggers `rollback()`,
/// restoring all registered files and directories to their pre-swap state.
pub struct RollbackSession {
    backup_root: PathBuf,
    entries: Vec<BackupEntry>,
    committed: bool,
    rolled_back: bool,
}

impl RollbackSession {
    /// Creates a new rollback session with an isolated temporary backup directory.
    pub fn new() -> Result<Self, AtomicError> {
        let sys_temp = std::env::temp_dir();
        let pid = std::process::id();
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let counter = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);

        let backup_root = sys_temp.join(format!("swch_backup_{}_{}_{}", pid, nanos, counter));
        fs::create_dir_all(&backup_root).map_err(|e| AtomicError::IoWithPath {
            path: backup_root.clone(),
            source: e,
        })?;

        Ok(Self {
            backup_root,
            entries: Vec::new(),
            committed: false,
            rolled_back: false,
        })
    }

    /// Creates a new rollback session inside a specified base directory (e.g. next to targets).
    pub fn new_in<P: AsRef<Path>>(base_dir: P) -> Result<Self, AtomicError> {
        let base = base_dir.as_ref();
        let pid = std::process::id();
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let counter = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);

        let backup_root = base.join(format!(".swch_backup_{}_{}_{}", pid, nanos, counter));
        fs::create_dir_all(&backup_root).map_err(|e| AtomicError::IoWithPath {
            path: backup_root.clone(),
            source: e,
        })?;

        Ok(Self {
            backup_root,
            entries: Vec::new(),
            committed: false,
            rolled_back: false,
        })
    }

    /// Backs up a file or directory before a swap operation.
    /// If the path does not exist, marks it as non-existent so rollback will clean it up if created.
    pub fn backup<P: AsRef<Path>>(&mut self, path: P) -> Result<(), AtomicError> {
        let orig = path.as_ref().to_path_buf();

        // Avoid duplicate backups of the same path in one session
        if self.entries.iter().any(|e| e.original_path == orig) {
            return Ok(());
        }

        // Avoid backing up the backup directory itself
        if orig == self.backup_root || orig.starts_with(&self.backup_root) {
            return Ok(());
        }
        if let (Ok(co), Ok(cb)) = (fs::canonicalize(&orig), fs::canonicalize(&self.backup_root)) {
            if co == cb || co.starts_with(&cb) {
                return Ok(());
            }
        }

        let idx = self.entries.len();
        let backup_item_name = format!("item_{}", idx);
        let backup_path = self.backup_root.join(backup_item_name);

        if !orig.exists() {
            self.entries.push(BackupEntry {
                original_path: orig,
                backup_path,
                was_directory: false,
                existed: false,
            });
            return Ok(());
        }

        let is_dir = orig.is_dir();
        if is_dir {
            copy_dir_all(&orig, &backup_path)?;
        } else {
            if let Some(parent) = backup_path.parent() {
                if !parent.exists() {
                    fs::create_dir_all(parent)?;
                }
            }
            fs::copy(&orig, &backup_path).map_err(|e| AtomicError::IoWithPath {
                path: backup_path.clone(),
                source: e,
            })?;
        }

        debug!("Backed up {} -> {}", orig.display(), backup_path.display());

        self.entries.push(BackupEntry {
            original_path: orig,
            backup_path,
            was_directory: is_dir,
            existed: true,
        });

        Ok(())
    }

    /// Performs the rollback: restores all backed-up items to their original locations.
    pub fn rollback(&mut self) -> Result<(), AtomicError> {
        if self.committed || self.rolled_back {
            return Ok(());
        }

        info!("Initiating rollback of {} items...", self.entries.len());
        let mut first_error = None;

        // Rollback entries in reverse order of backup
        for entry in self.entries.iter().rev() {
            if !entry.existed {
                // If it didn't exist prior to swap, remove it if it exists now
                if entry.original_path.exists() {
                    let res = if entry.original_path.is_dir() {
                        fs::remove_dir_all(&entry.original_path)
                    } else {
                        fs::remove_file(&entry.original_path)
                    };
                    if let Err(e) = res {
                        error!(
                            "Rollback failed to remove newly created path {}: {}",
                            entry.original_path.display(),
                            e
                        );
                        if first_error.is_none() {
                            first_error = Some(AtomicError::RollbackFailed(format!(
                                "Failed to remove created path {}: {}",
                                entry.original_path.display(),
                                e
                            )));
                        }
                    }
                }
            } else if entry.was_directory {
                // Clean current target directory if exists
                if entry.original_path.exists() {
                    let is_nested = self.backup_root.starts_with(&entry.original_path)
                        || match (
                            fs::canonicalize(&self.backup_root),
                            fs::canonicalize(&entry.original_path),
                        ) {
                            (Ok(cb), Ok(co)) => cb.starts_with(&co),
                            _ => false,
                        };
                    let res = if is_nested {
                        clear_dir_except(&entry.original_path, &self.backup_root)
                    } else {
                        fs::remove_dir_all(&entry.original_path)
                    };
                    if let Err(e) = res {
                        error!(
                            "Rollback failed to clear corrupted dir {}: {}",
                            entry.original_path.display(),
                            e
                        );
                    }
                }
                // Restore from backup
                if let Err(e) = copy_dir_all(&entry.backup_path, &entry.original_path) {
                    error!(
                        "Rollback failed to restore directory {}: {}",
                        entry.original_path.display(),
                        e
                    );
                    if first_error.is_none() {
                        first_error = Some(e);
                    }
                }
            } else {
                // File restore using streaming atomic_copy (avoids large RAM allocation)
                if entry.backup_path.exists() {
                    if let Err(e) = atomic_copy(&entry.backup_path, &entry.original_path) {
                        error!(
                            "Rollback failed to restore file {}: {}",
                            entry.original_path.display(),
                            e
                        );
                        if first_error.is_none() {
                            first_error = Some(e);
                        }
                    }
                }
            }
        }

        self.rolled_back = true;
        // Clean up backup directory only if rollback fully succeeded;
        // preserve backup if errors occurred so data is not lost!
        if let Some(err) = first_error {
            error!(
                "Rollback completed with errors. Preserving backup directory at {}",
                self.backup_root.display()
            );
            Err(err)
        } else {
            let _ = fs::remove_dir_all(&self.backup_root);
            info!("Rollback completed successfully.");
            Ok(())
        }
    }

    /// Commits the session: swap was successful, deletes the temporary backup files.
    pub fn commit(mut self) -> Result<(), AtomicError> {
        if self.rolled_back {
            return Err(AtomicError::SwapFailed(
                "Cannot commit an already rolled-back session".into(),
            ));
        }

        self.committed = true;
        if self.backup_root.exists() {
            fs::remove_dir_all(&self.backup_root).map_err(|e| AtomicError::IoWithPath {
                path: self.backup_root.clone(),
                source: e,
            })?;
        }
        debug!("Rollback session committed and backup data cleaned up.");
        Ok(())
    }
}

impl Drop for RollbackSession {
    fn drop(&mut self) {
        if !self.committed && !self.rolled_back {
            warn!("RollbackSession dropped without commit. Triggering automatic rollback!");
            if let Err(e) = self.rollback() {
                error!("Automatic rollback during drop encountered error: {}", e);
            }
        }
    }
}

/// Executes a transactional swap operation with automatic backup and rollback.
///
/// If `swap_fn` returns an `Ok(R)` value, the backup is committed and cleaned up.
/// If `swap_fn` returns an `Err(E)` or panics, the target path is restored to its exact pre-swap state.
pub fn backup_and_swap<P, F, R, E>(target: P, swap_fn: F) -> Result<R, AtomicError>
where
    P: AsRef<Path>,
    F: FnOnce(&Path) -> Result<R, E>,
    E: std::fmt::Display,
{
    let target_ref = target.as_ref();
    let mut session = RollbackSession::new()?;
    session.backup(target_ref)?;

    match swap_fn(target_ref) {
        Ok(result) => {
            session.commit()?;
            Ok(result)
        }
        Err(e) => {
            warn!("Swap operation returned error: {}. Rolling back...", e);
            session.rollback()?;
            Err(AtomicError::SwapFailed(e.to_string()))
        }
    }
}

/// Safely replaces a target file with contents from a source file, with automatic backup and rollback.
pub fn atomic_swap_files<P: AsRef<Path>, Q: AsRef<Path>>(
    target: P,
    source: Q,
) -> Result<(), AtomicError> {
    let target = target.as_ref();
    let source = source.as_ref();

    let new_data = fs::read(source).map_err(|e| AtomicError::IoWithPath {
        path: source.to_path_buf(),
        source: e,
    })?;

    backup_and_swap(target, |path| atomic_write(path, new_data))
}

/// Safely swaps the contents of a target directory with the contents of a source directory,
/// staging changes first and providing full rollback if any error occurs.
pub fn atomic_swap_dirs<P: AsRef<Path>, Q: AsRef<Path>>(
    target: P,
    source: Q,
) -> Result<(), AtomicError> {
    let target = target.as_ref();
    let source = source.as_ref();

    let temp_staging = generate_temp_path(target)?;
    copy_dir_all(source, &temp_staging)?;

    let res = backup_and_swap(target, |tgt| {
        if tgt.exists() {
            fs::remove_dir_all(tgt).map_err(|e| AtomicError::IoWithPath {
                path: tgt.to_path_buf(),
                source: e,
            })?;
        }
        copy_dir_all(&temp_staging, tgt)
    });

    if temp_staging.exists() {
        let _ = fs::remove_dir_all(&temp_staging);
    }

    res
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::{Deserialize, Serialize};
    use tempfile::tempdir;

    #[test]
    fn test_atomic_write_basic() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("session.json");

        let content = b"{\"user\": \"test_pilot\", \"active\": true}";
        atomic_write(&file_path, content).unwrap();

        let read_back = fs::read(&file_path).unwrap();
        assert_eq!(read_back, content);
    }

    #[test]
    fn test_atomic_write_overwrite() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("config.vdf");

        atomic_write(&file_path, b"initial_data").unwrap();
        assert_eq!(fs::read(&file_path).unwrap(), b"initial_data");

        atomic_write(&file_path, b"new_overwritten_data").unwrap();
        assert_eq!(fs::read(&file_path).unwrap(), b"new_overwritten_data");
    }

    #[test]
    fn test_atomic_write_creates_nested_directories() {
        let dir = tempdir().unwrap();
        let nested_file = dir
            .path()
            .join("deeply")
            .join("nested")
            .join("sub")
            .join("data.txt");

        atomic_write(&nested_file, b"deep_content").unwrap();
        assert!(nested_file.exists());
        assert_eq!(fs::read(&nested_file).unwrap(), b"deep_content");
    }

    #[test]
    fn test_atomic_write_str_and_json() {
        let dir = tempdir().unwrap();
        let text_file = dir.path().join("hello.txt");
        atomic_write_str(&text_file, "Hello, Star Wars Chronology!").unwrap();
        assert_eq!(
            fs::read_to_string(&text_file).unwrap(),
            "Hello, Star Wars Chronology!"
        );

        #[derive(Serialize, Deserialize, PartialEq, Debug)]
        struct TestData {
            account: String,
            games: Vec<String>,
        }

        let json_file = dir.path().join("data.json");
        let test_obj = TestData {
            account: "luke_skywalker".into(),
            games: vec!["Jedi: Fallen Order".into(), "Jedi: Survivor".into()],
        };

        atomic_write_json(&json_file, &test_obj).unwrap();
        let loaded: TestData =
            serde_json::from_str(&fs::read_to_string(&json_file).unwrap()).unwrap();
        assert_eq!(loaded, test_obj);
    }

    #[test]
    fn test_atomic_file_writer_commit() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("stream.txt");

        let mut writer = AtomicFileWriter::new(&file_path).unwrap();
        writer.write_all(b"chunk1 ").unwrap();
        writer.write_all(b"chunk2").unwrap();
        writer.commit().unwrap();

        assert_eq!(fs::read_to_string(&file_path).unwrap(), "chunk1 chunk2");
    }

    #[test]
    fn test_atomic_file_writer_abort() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("aborted.txt");

        let mut writer = AtomicFileWriter::new(&file_path).unwrap();
        writer.write_all(b"doomed content").unwrap();
        writer.abort();

        assert!(!file_path.exists());
    }

    #[test]
    fn test_atomic_file_writer_drop_cleanup() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("dropped.txt");

        {
            let mut writer = AtomicFileWriter::new(&file_path).unwrap();
            writer.write_all(b"never committed").unwrap();
            // drops here without commit
        }

        assert!(!file_path.exists());
        // Verify no leftover .tmp files
        let entries: Vec<_> = fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap())
            .collect();
        assert!(
            entries.is_empty(),
            "Expected directory to be clean after dropped writer"
        );
    }

    #[test]
    fn test_copy_dir_all() {
        let src_dir = tempdir().unwrap();
        let dst_dir = tempdir().unwrap();

        let sub = src_dir.path().join("subdir");
        fs::create_dir_all(&sub).unwrap();
        fs::write(src_dir.path().join("root.txt"), b"root_file").unwrap();
        fs::write(sub.join("nested.txt"), b"nested_file").unwrap();

        let target_sub = dst_dir.path().join("copied");
        copy_dir_all(src_dir.path(), &target_sub).unwrap();

        assert_eq!(fs::read(target_sub.join("root.txt")).unwrap(), b"root_file");
        assert_eq!(
            fs::read(target_sub.join("subdir").join("nested.txt")).unwrap(),
            b"nested_file"
        );
    }

    #[test]
    fn test_rollback_session_file_success() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("user.vdf");
        fs::write(&file_path, b"original_steam_data").unwrap();

        let mut session = RollbackSession::new().unwrap();
        session.backup(&file_path).unwrap();

        // Simulate successful file update
        atomic_write(&file_path, b"updated_steam_data").unwrap();
        session.commit().unwrap();

        assert_eq!(fs::read(&file_path).unwrap(), b"updated_steam_data");
    }

    #[test]
    fn test_rollback_session_file_rollback() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("user.vdf");
        fs::write(&file_path, b"original_data").unwrap();

        let mut session = RollbackSession::new().unwrap();
        session.backup(&file_path).unwrap();

        // Corrupt or overwrite file
        fs::write(&file_path, b"corrupted_swap_data").unwrap();
        assert_eq!(fs::read(&file_path).unwrap(), b"corrupted_swap_data");

        // Execute rollback
        session.rollback().unwrap();

        // Must be restored to original data
        assert_eq!(fs::read(&file_path).unwrap(), b"original_data");
    }

    #[test]
    fn test_rollback_session_file_drop_aborts() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("session_drop.vdf");
        fs::write(&file_path, b"safe_unmodified_state").unwrap();

        {
            let mut session = RollbackSession::new().unwrap();
            session.backup(&file_path).unwrap();

            fs::write(&file_path, b"partial_broken_write").unwrap();
            // session drops here without commit() -> must trigger automatic rollback
        }

        assert_eq!(
            fs::read(&file_path).unwrap(),
            b"safe_unmodified_state",
            "Drop must automatically revert changes to pre-swap state"
        );
    }

    #[test]
    fn test_rollback_session_nonexistent_file() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("created_during_swap.txt");
        assert!(!file_path.exists());

        let mut session = RollbackSession::new().unwrap();
        session.backup(&file_path).unwrap();

        // File gets created during a faulty swap
        fs::write(&file_path, b"temporary_garbage").unwrap();
        assert!(file_path.exists());

        // Rollback should delete it because it didn't exist before
        session.rollback().unwrap();
        assert!(
            !file_path.exists(),
            "Rollback must remove files that didn't exist before swap"
        );
    }

    #[test]
    fn test_rollback_session_directory_tree_rollback() {
        let base = tempdir().unwrap();
        let config_dir = base.path().join("launcher_config");
        let sub = config_dir.join("profiles");
        fs::create_dir_all(&sub).unwrap();

        let file1 = config_dir.join("global.json");
        let file2 = sub.join("profile1.dat");

        fs::write(&file1, b"{\"volume\": 100}").unwrap();
        fs::write(&file2, b"binary_profile_data").unwrap();

        let mut session = RollbackSession::new().unwrap();
        session.backup(&config_dir).unwrap();

        // Simulate corrupted swap: modify file1, delete file2, add malicious file3
        fs::write(&file1, b"corrupted").unwrap();
        fs::remove_file(&file2).unwrap();
        let file3 = config_dir.join("unwanted.tmp");
        fs::write(&file3, b"junk").unwrap();

        // Rollback!
        session.rollback().unwrap();

        // Verify full restoration
        assert_eq!(fs::read(&file1).unwrap(), b"{\"volume\": 100}");
        assert_eq!(fs::read(&file2).unwrap(), b"binary_profile_data");
        assert!(
            !file3.exists(),
            "Unwanted files created during swap must be purged"
        );
    }

    #[test]
    fn test_backup_and_swap_helper() {
        let dir = tempdir().unwrap();
        let target = dir.path().join("active_session.txt");
        fs::write(&target, b"session_alpha").unwrap();

        // Case 1: Swap function succeeds
        let res = backup_and_swap(&target, |p| fs::write(p, b"session_beta"));
        assert!(res.is_ok());
        assert_eq!(fs::read(&target).unwrap(), b"session_beta");

        // Case 2: Swap function returns an error
        let err_res = backup_and_swap(&target, |p| -> Result<(), io::Error> {
            fs::write(p, b"session_gamma_half_written")?;
            Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "simulated network or disk error",
            ))
        });

        assert!(err_res.is_err());
        // Must have rolled back to session_beta!
        assert_eq!(
            fs::read(&target).unwrap(),
            b"session_beta",
            "Failed swap must rollback to last known good state"
        );
    }

    #[test]
    fn test_atomic_swap_files() {
        let dir = tempdir().unwrap();
        let target = dir.path().join("current.txt");
        let source = dir.path().join("new_version.txt");

        fs::write(&target, b"version 1.0").unwrap();
        fs::write(&source, b"version 2.0").unwrap();

        atomic_swap_files(&target, &source).unwrap();
        assert_eq!(fs::read(&target).unwrap(), b"version 2.0");
    }

    #[test]
    fn test_atomic_swap_dirs() {
        let dir = tempdir().unwrap();
        let target_dir = dir.path().join("current_dir");
        let source_dir = dir.path().join("source_dir");

        fs::create_dir_all(&target_dir).unwrap();
        fs::write(target_dir.join("f1.txt"), b"old_f1").unwrap();

        fs::create_dir_all(&source_dir).unwrap();
        fs::write(source_dir.join("f2.txt"), b"new_f2").unwrap();

        atomic_swap_dirs(&target_dir, &source_dir).unwrap();

        assert!(!target_dir.join("f1.txt").exists());
        assert_eq!(fs::read(target_dir.join("f2.txt")).unwrap(), b"new_f2");
    }

    #[test]
    fn test_atomic_copy() {
        let dir = tempdir().unwrap();
        let src = dir.path().join("source.dat");
        let dst = dir.path().join("destination.dat");

        fs::write(&src, b"binary_streaming_data_content").unwrap();
        atomic_copy(&src, &dst).unwrap();

        assert_eq!(fs::read(&dst).unwrap(), b"binary_streaming_data_content");
    }

    #[test]
    fn test_rollback_session_new_in_nested() {
        // Critical test: verify that backup_root inside base_dir does NOT cause infinite recursion!
        let base = tempdir().unwrap();
        let target_dir = base.path().join("game_profiles");
        fs::create_dir_all(&target_dir).unwrap();
        fs::write(target_dir.join("profile.vdf"), b"profile_content").unwrap();

        let mut session = RollbackSession::new_in(&target_dir).unwrap();
        session.backup(&target_dir).unwrap();

        // Overwrite profile
        fs::write(target_dir.join("profile.vdf"), b"corrupted_content").unwrap();

        // Trigger rollback
        session.rollback().unwrap();

        assert_eq!(
            fs::read(target_dir.join("profile.vdf")).unwrap(),
            b"profile_content",
            "Rollback inside nested directory must restore original content without infinite recursion"
        );
    }

    #[test]
    fn test_atomic_swap_dirs_failure_rollback() {
        let dir = tempdir().unwrap();
        let target_dir = dir.path().join("live_config");
        let source_dir = dir.path().join("broken_source");

        fs::create_dir_all(&target_dir).unwrap();
        fs::write(target_dir.join("app.cfg"), b"live_app_configuration").unwrap();

        // Source exists
        fs::create_dir_all(&source_dir).unwrap();
        fs::write(source_dir.join("app.cfg"), b"new_app_configuration").unwrap();

        // Swap succeeds
        atomic_swap_dirs(&target_dir, &source_dir).unwrap();
        assert_eq!(
            fs::read(target_dir.join("app.cfg")).unwrap(),
            b"new_app_configuration"
        );
    }
}
