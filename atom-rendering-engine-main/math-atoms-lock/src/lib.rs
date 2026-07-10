//! Kernel-owned cross-process leases with auditable owner-token files.

use std::fs::{self, OpenOptions};
use std::io::{self, Write};
#[cfg(unix)]
use std::io::{Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
#[cfg(unix)]
use std::time::Instant;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

static LEASE_SEQUENCE: AtomicU64 = AtomicU64::new(1);

#[derive(Debug)]
pub struct FileLease {
    path: PathBuf,
    owner_token: String,
    guard: Option<PlatformGuard>,
}

impl Drop for FileLease {
    fn drop(&mut self) {
        remove_owner_file(&self.path, &self.owner_token);
        self.guard.take();
    }
}

pub fn acquire_file_lease(
    path: impl AsRef<Path>,
    timeout: Duration,
    _stale_age: Duration,
) -> io::Result<FileLease> {
    let path = absolute_path(path.as_ref())?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let owner_token = owner_token()?;
    let mut guard = acquire_platform_guard(&path, timeout)?;
    guard.write_owner(&path, &owner_token)?;
    Ok(FileLease {
        path,
        owner_token,
        guard: Some(guard),
    })
}

fn absolute_path(path: &Path) -> io::Result<PathBuf> {
    if path.is_absolute() {
        Ok(path.to_path_buf())
    } else {
        Ok(std::env::current_dir()?.join(path))
    }
}

fn remove_owner_file(path: &Path, owner_token: &str) {
    if fs::read_to_string(path)
        .map(|value| value == owner_token)
        .unwrap_or(false)
    {
        let _ = fs::remove_file(path);
    }
}

fn owner_token() -> io::Result<String> {
    let pid = std::process::id();
    let start = process_start_id(pid)
        .ok_or_else(|| io::Error::other("could not determine current process start identity"))?;
    Ok(format!(
        "pid={pid} start={start} time_ms={} sequence={}",
        now_ms(),
        LEASE_SEQUENCE.fetch_add(1, Ordering::Relaxed)
    ))
}

#[cfg(windows)]
#[derive(Debug)]
struct PlatformGuard {
    handle: *mut std::ffi::c_void,
}

#[cfg(windows)]
impl PlatformGuard {
    fn write_owner(&mut self, path: &Path, owner_token: &str) -> io::Result<()> {
        let mut file = OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(path)?;
        file.write_all(owner_token.as_bytes())?;
        file.flush()?;
        file.sync_all()
    }
}

#[cfg(windows)]
impl Drop for PlatformGuard {
    fn drop(&mut self) {
        unsafe {
            let _ = ReleaseMutex(self.handle);
            let _ = CloseHandle(self.handle);
        }
    }
}

#[cfg(windows)]
unsafe impl Send for PlatformGuard {}

#[cfg(windows)]
#[link(name = "kernel32")]
extern "system" {
    fn CreateMutexW(
        attributes: *const std::ffi::c_void,
        initial_owner: i32,
        name: *const u16,
    ) -> *mut std::ffi::c_void;
    fn WaitForSingleObject(handle: *mut std::ffi::c_void, milliseconds: u32) -> u32;
    fn ReleaseMutex(handle: *mut std::ffi::c_void) -> i32;
    fn CloseHandle(handle: *mut std::ffi::c_void) -> i32;
    fn OpenProcess(access: u32, inherit: i32, process_id: u32) -> *mut std::ffi::c_void;
    fn GetProcessTimes(
        handle: *mut std::ffi::c_void,
        creation: *mut FileTime,
        exit: *mut FileTime,
        kernel: *mut FileTime,
        user: *mut FileTime,
    ) -> i32;
}

#[cfg(windows)]
fn acquire_platform_guard(path: &Path, timeout: Duration) -> io::Result<PlatformGuard> {
    const WAIT_OBJECT_0: u32 = 0;
    const WAIT_ABANDONED: u32 = 0x80;
    const WAIT_TIMEOUT: u32 = 258;
    use math_atoms_hash::sha256_hex;
    use std::os::windows::ffi::OsStrExt;

    let mut identity = Vec::new();
    for unit in path.as_os_str().encode_wide() {
        identity.extend_from_slice(&unit.to_le_bytes());
    }
    let name = format!("Global\\MathAtomsLease-{}", sha256_hex(&identity));
    let wide = name
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let handle = unsafe { CreateMutexW(std::ptr::null(), 0, wide.as_ptr()) };
    if handle.is_null() {
        return Err(io::Error::last_os_error());
    }
    let timeout_ms = timeout.as_millis().min(u128::from(u32::MAX - 1)) as u32;
    match unsafe { WaitForSingleObject(handle, timeout_ms) } {
        WAIT_OBJECT_0 | WAIT_ABANDONED => Ok(PlatformGuard { handle }),
        WAIT_TIMEOUT => {
            unsafe {
                let _ = CloseHandle(handle);
            }
            Err(io::Error::new(
                io::ErrorKind::WouldBlock,
                format!("timed out acquiring file lease at {}", path.display()),
            ))
        }
        _ => {
            let error = io::Error::last_os_error();
            unsafe {
                let _ = CloseHandle(handle);
            }
            Err(error)
        }
    }
}

#[cfg(unix)]
#[derive(Debug)]
struct PlatformGuard {
    file: std::fs::File,
}

#[cfg(unix)]
impl PlatformGuard {
    fn write_owner(&mut self, _path: &Path, owner_token: &str) -> io::Result<()> {
        self.file.set_len(0)?;
        self.file.seek(SeekFrom::Start(0))?;
        self.file.write_all(owner_token.as_bytes())?;
        self.file.flush()?;
        self.file.sync_all()
    }
}

#[cfg(unix)]
impl Drop for PlatformGuard {
    fn drop(&mut self) {
        use std::os::fd::AsRawFd;
        unsafe {
            let _ = flock(self.file.as_raw_fd(), LOCK_UN);
        }
    }
}

#[cfg(unix)]
const LOCK_EX: i32 = 2;
#[cfg(unix)]
const LOCK_NB: i32 = 4;
#[cfg(unix)]
const LOCK_UN: i32 = 8;

#[cfg(unix)]
extern "C" {
    fn flock(fd: i32, operation: i32) -> i32;
}

#[cfg(unix)]
fn acquire_platform_guard(path: &Path, timeout: Duration) -> io::Result<PlatformGuard> {
    use std::os::fd::AsRawFd;
    use std::os::unix::fs::MetadataExt;
    use std::thread;

    let deadline = Instant::now() + timeout;
    loop {
        let file = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .open(path)?;
        if unsafe { flock(file.as_raw_fd(), LOCK_EX | LOCK_NB) } == 0 {
            let descriptor = file.metadata()?;
            let current = fs::metadata(path);
            if current.as_ref().is_ok_and(|value| {
                value.dev() == descriptor.dev() && value.ino() == descriptor.ino()
            }) {
                return Ok(PlatformGuard { file });
            }
        } else {
            let error = io::Error::last_os_error();
            if !matches!(error.kind(), io::ErrorKind::WouldBlock) {
                return Err(error);
            }
        }
        if Instant::now() >= deadline {
            return Err(io::Error::new(
                io::ErrorKind::WouldBlock,
                format!("timed out acquiring file lease at {}", path.display()),
            ));
        }
        thread::sleep(Duration::from_millis(20));
    }
}

#[cfg(windows)]
#[repr(C)]
struct FileTime {
    low: u32,
    high: u32,
}

#[cfg(windows)]
fn process_start_id(pid: u32) -> Option<u64> {
    const PROCESS_QUERY_LIMITED_INFORMATION: u32 = 0x1000;
    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if handle.is_null() {
            return None;
        }
        let mut creation = FileTime { low: 0, high: 0 };
        let mut exit = FileTime { low: 0, high: 0 };
        let mut kernel = FileTime { low: 0, high: 0 };
        let mut user = FileTime { low: 0, high: 0 };
        let ok = GetProcessTimes(handle, &mut creation, &mut exit, &mut kernel, &mut user);
        let _ = CloseHandle(handle);
        (ok != 0).then_some((u64::from(creation.high) << 32) | u64::from(creation.low))
    }
}

#[cfg(target_os = "linux")]
fn process_start_id(pid: u32) -> Option<u64> {
    let text = fs::read_to_string(Path::new("/proc").join(pid.to_string()).join("stat")).ok()?;
    text.rsplit_once(')')?
        .1
        .split_whitespace()
        .nth(19)?
        .parse()
        .ok()
}

#[cfg(all(unix, not(target_os = "linux")))]
fn process_start_id(_pid: u32) -> Option<u64> {
    Some(now_ms() as u64)
}

fn now_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;

    fn temp_lock(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "math-atoms-lock-{label}-{}-{}",
            std::process::id(),
            now_ms()
        ))
    }

    #[test]
    fn live_owner_is_exclusive_and_kernel_releases_it() {
        let path = temp_lock("exclusive");
        let lease =
            acquire_file_lease(&path, Duration::from_secs(1), Duration::from_secs(30)).unwrap();
        let contender_path = path.clone();
        let contender = thread::spawn(move || {
            acquire_file_lease(
                contender_path,
                Duration::from_millis(100),
                Duration::from_secs(30),
            )
        });
        assert_eq!(
            contender.join().unwrap().unwrap_err().kind(),
            io::ErrorKind::WouldBlock
        );
        drop(lease);
        assert!(!path.exists());
        drop(acquire_file_lease(&path, Duration::from_secs(1), Duration::from_secs(30)).unwrap());
    }

    #[test]
    fn mismatched_owner_token_is_never_removed() {
        let path = temp_lock("replacement");
        fs::write(&path, "replacement").unwrap();
        remove_owner_file(&path, "original");
        assert_eq!(fs::read_to_string(&path).unwrap(), "replacement");
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn owner_token_binds_process_start_identity() {
        let token = owner_token().unwrap();
        assert!(token.contains(&format!("pid={}", std::process::id())));
        assert!(token.contains(" start="));
    }
}
