//! Atomic file writes: a temp file in the target's directory, flushed to
//! disk, then renamed over the target, and the directory flushed so the
//! rename survives a power cut. Every file Lampstand replaces whole goes
//! through here: the repo's chunks and snapshots, and (through
//! [`write_atomic`] and [`write_private`]) the app's device-local files.

use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

/// Who may open a file written here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Access {
    /// The platform's default for a new file in that directory.
    Default,
    /// Only this user: mode 0600 on Unix, and on Windows a protected DACL
    /// that grants the current user alone (nothing inherited from the
    /// folder).
    Owner,
}

/// A temp file beside `dir`'s other files, removed on drop unless it was
/// renamed into place with [`TempFile::persist`].
pub(crate) struct TempFile {
    path: PathBuf,
    file: Option<File>,
}

impl TempFile {
    pub(crate) fn new_in(dir: &Path) -> io::Result<Self> {
        Self::create(dir, Access::Default)
    }

    fn create(dir: &Path, access: Access) -> io::Result<Self> {
        let path = dir.join(format!(".tmp-{}", random_hex()));
        let mut options = File::options();
        options.write(true).create_new(true);
        if access == Access::Owner {
            owner_only::options(&mut options);
        }
        let file = options.open(&path)?;
        // From here on, dropping `tmp` removes the file.
        let tmp = Self {
            path,
            file: Some(file),
        };
        if access == Access::Owner {
            // Before any bytes are written, so the secret is never readable
            // by anyone else, even for a moment.
            owner_only::restrict(tmp.file.as_ref().expect("just opened"))?;
        }
        Ok(tmp)
    }

    pub(crate) fn file(&mut self) -> &mut File {
        self.file.as_mut().expect("temp file open until persisted")
    }

    /// Flushes the contents to disk and renames the file to `target`.
    pub(crate) fn persist(self, target: &Path) -> io::Result<()> {
        self.persist_with(target, File::sync_all, false).map(drop)
    }

    /// Flushes the file with `sync`, renames it to `target` and flushes the
    /// directory with `sync`. With `keep_open` the file comes back still
    /// open (renamed under it, which only Unix allows); otherwise it is
    /// closed before the rename.
    fn persist_with(
        mut self,
        target: &Path,
        sync: fn(&File) -> io::Result<()>,
        keep_open: bool,
    ) -> io::Result<Option<File>> {
        sync(self.file())?;
        let kept = if keep_open {
            fs::rename(&self.path, target)?;
            self.file.take()
        } else {
            // Close first: Windows will not rename over a file that is open.
            drop(self.file.take());
            if let Err(e) = fs::rename(&self.path, target) {
                let _ = fs::remove_file(&self.path);
                return Err(e);
            }
            None
        };
        sync_dir(target.parent().unwrap_or(Path::new(".")), sync)?;
        Ok(kept)
    }
}

impl Drop for TempFile {
    fn drop(&mut self) {
        if self.file.take().is_some() {
            let _ = fs::remove_file(&self.path);
        }
    }
}

/// Atomic writes made durable together, for a batch of documents.
///
/// On Apple platforms `File::sync_all` is `F_FULLFSYNC`, which also flushes
/// the drive's own cache and costs about 6 ms each, so a `put` (a chunk,
/// the team index and a `kind` file, each with its directory) took ~40 ms.
/// Here each file and directory gets a plain `fsync(2)`, which hands the
/// data to the drive, and [`Durable::finish`] issues one `F_FULLFSYNC` for
/// the batch, which flushes the drive cache for all of it. Elsewhere
/// `sync_all` already is `fsync(2)`, so each write is durable on its own.
#[derive(Default)]
pub(crate) struct Durable {
    #[cfg(target_vendor = "apple")]
    last: Option<File>,
}

impl Durable {
    pub(crate) fn write_atomic(&mut self, target: &Path, bytes: &[u8]) -> io::Result<()> {
        #[cfg(target_vendor = "apple")]
        {
            self.last = write(target, bytes, Access::Default, fsync, true)?;
            Ok(())
        }
        #[cfg(not(target_vendor = "apple"))]
        write_atomic(target, bytes)
    }

    /// Makes everything written so far durable.
    pub(crate) fn finish(self) -> io::Result<()> {
        #[cfg(target_vendor = "apple")]
        if let Some(file) = self.last {
            file.sync_all()?;
        }
        Ok(())
    }
}

/// A plain `fsync(2)`, not std's `F_FULLFSYNC` (see [`Durable`]).
#[cfg(target_vendor = "apple")]
fn fsync(file: &File) -> io::Result<()> {
    use std::os::fd::AsRawFd;
    // SAFETY: the descriptor belongs to `file`, which is open for the call.
    if unsafe { libc::fsync(file.as_raw_fd()) } == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

/// The one atomic write: everything else here is a way of calling it.
fn write(
    target: &Path,
    bytes: &[u8],
    access: Access,
    sync: fn(&File) -> io::Result<()>,
    keep_open: bool,
) -> io::Result<Option<File>> {
    let dir = target.parent().unwrap_or(Path::new("."));
    let mut tmp = TempFile::create(dir, access)?;
    tmp.file().write_all(bytes)?;
    tmp.persist_with(target, sync, keep_open)
}

/// Replaces `target` with `bytes` atomically and durably: a reader sees the
/// old contents or the new, and after a power cut the new contents are
/// there once this returns. The directory must exist.
pub fn write_atomic(target: &Path, bytes: &[u8]) -> io::Result<()> {
    write(target, bytes, Access::Default, File::sync_all, false).map(drop)
}

/// [`write_atomic`] for a secret: the file is readable and writable by this
/// user alone (mode 0600 on Unix; on Windows a DACL granting only the
/// current user), from the moment it exists.
pub fn write_private(target: &Path, bytes: &[u8]) -> io::Result<()> {
    write(target, bytes, Access::Owner, File::sync_all, false).map(drop)
}

/// Narrows an existing file to this user alone, as [`write_private`] would
/// have written it: for secrets written before it did.
pub fn restrict_to_owner(path: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        let mode = fs::metadata(path)?.permissions().mode();
        if mode & 0o077 != 0 {
            fs::set_permissions(path, fs::Permissions::from_mode(mode & 0o700))?;
        }
        Ok(())
    }
    #[cfg(not(unix))]
    {
        let mut options = File::options();
        options.read(true);
        owner_only::options(&mut options);
        owner_only::restrict(&options.open(path)?)
    }
}

/// Makes a rename durable on Unix. Windows has no directory handles to
/// flush, and NTFS journals the rename itself.
fn sync_dir(dir: &Path, sync: fn(&File) -> io::Result<()>) -> io::Result<()> {
    #[cfg(unix)]
    {
        sync(&File::open(dir)?)
    }
    #[cfg(not(unix))]
    {
        let _ = (dir, sync);
        Ok(())
    }
}

/// Owner-only files, per platform.
#[cfg(unix)]
mod owner_only {
    use std::fs::{File, OpenOptions};
    use std::io;
    use std::os::unix::fs::OpenOptionsExt as _;

    /// Creates the file with mode 0600 (the umask can only narrow it).
    pub(super) fn options(options: &mut OpenOptions) {
        options.mode(0o600);
    }

    pub(super) fn restrict(_file: &File) -> io::Result<()> {
        Ok(())
    }
}

#[cfg(windows)]
mod owner_only {
    use std::fs::{File, OpenOptions};
    use std::io;
    use std::mem::size_of;
    use std::os::windows::fs::OpenOptionsExt as _;
    use std::os::windows::io::AsRawHandle as _;
    use std::ptr;

    use windows_sys::Win32::Foundation::{CloseHandle, ERROR_SUCCESS, HANDLE};
    use windows_sys::Win32::Security::Authorization::{SetSecurityInfo, SE_FILE_OBJECT};
    use windows_sys::Win32::Security::{
        AddAccessAllowedAce, GetLengthSid, GetTokenInformation, InitializeAcl, TokenUser,
        ACCESS_ALLOWED_ACE, ACL, ACL_REVISION, DACL_SECURITY_INFORMATION,
        PROTECTED_DACL_SECURITY_INFORMATION, TOKEN_QUERY, TOKEN_USER,
    };
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

    const GENERIC_READ: u32 = 0x8000_0000;
    const GENERIC_WRITE: u32 = 0x4000_0000;
    const WRITE_DAC: u32 = 0x0004_0000;
    const FILE_ALL_ACCESS: u32 = 0x001F_01FF;

    /// Opens the handle with the right to replace its DACL as well.
    pub(super) fn options(options: &mut OpenOptions) {
        options.access_mode(GENERIC_READ | GENERIC_WRITE | WRITE_DAC);
    }

    /// Replaces the file's DACL with one entry, full access for the user
    /// this process runs as, and stops it inheriting from the folder.
    pub(super) fn restrict(file: &File) -> io::Result<()> {
        let user = current_user()?;
        // SAFETY: `user` holds a TOKEN_USER whose SID points into it, and
        // stays alive until the DACL has been set.
        unsafe {
            let token_user = &*(user.as_ptr() as *const TOKEN_USER);
            let sid = token_user.User.Sid;
            let len =
                size_of::<ACL>() + size_of::<ACCESS_ALLOWED_ACE>() + GetLengthSid(sid) as usize;
            // u32 words keep the ACL aligned.
            let mut acl = vec![0u32; len.div_ceil(4)];
            let acl = acl.as_mut_ptr() as *mut ACL;
            if InitializeAcl(acl, len as u32, ACL_REVISION) == 0
                || AddAccessAllowedAce(acl, ACL_REVISION, FILE_ALL_ACCESS, sid) == 0
            {
                return Err(io::Error::last_os_error());
            }
            let status = SetSecurityInfo(
                file.as_raw_handle() as HANDLE,
                SE_FILE_OBJECT,
                DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
                ptr::null_mut(),
                ptr::null_mut(),
                acl,
                ptr::null(),
            );
            if status != ERROR_SUCCESS {
                return Err(io::Error::from_raw_os_error(status as i32));
            }
        }
        Ok(())
    }

    /// The process token's TOKEN_USER, in a buffer aligned for it.
    fn current_user() -> io::Result<Vec<u64>> {
        // SAFETY: plain Win32 calls on a token handle closed before return;
        // the buffer is sized by the first call.
        unsafe {
            let mut token: HANDLE = ptr::null_mut();
            if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
                return Err(io::Error::last_os_error());
            }
            let mut len = 0u32;
            GetTokenInformation(token, TokenUser, ptr::null_mut(), 0, &mut len);
            let mut buf = vec![0u64; (len as usize).div_ceil(8)];
            let ok = GetTokenInformation(
                token,
                TokenUser,
                buf.as_mut_ptr().cast(),
                (buf.len() * 8) as u32,
                &mut len,
            );
            let err = io::Error::last_os_error();
            CloseHandle(token);
            if ok == 0 {
                return Err(err);
            }
            Ok(buf)
        }
    }
}

pub(crate) fn random_bytes<const N: usize>() -> [u8; N] {
    let mut buf = [0u8; N];
    getrandom::fill(&mut buf).expect("operating system random source");
    buf
}

fn random_hex() -> String {
    random_bytes::<8>()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_files(dir: &Path) -> Vec<String> {
        fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .filter(|n| n.starts_with(".tmp-"))
            .collect()
    }

    #[test]
    fn a_write_replaces_the_file_and_leaves_no_temp_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.json");
        write_atomic(&path, b"one").unwrap();
        write_atomic(&path, b"two").unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"two");
        assert!(temp_files(dir.path()).is_empty());
    }

    #[test]
    fn a_failed_write_leaves_the_old_file_and_no_temp_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.json");
        write_atomic(&path, b"old").unwrap();
        // A directory in the way of the rename.
        let blocked = dir.path().join("b");
        fs::create_dir(&blocked).unwrap();
        fs::write(blocked.join("x"), b"").unwrap();
        assert!(write_atomic(&blocked, b"new").is_err());
        assert_eq!(fs::read(&path).unwrap(), b"old");
        assert!(temp_files(dir.path()).is_empty());
    }

    #[test]
    fn a_batch_of_durable_writes_lands() {
        let dir = tempfile::tempdir().unwrap();
        let mut durable = Durable::default();
        durable.write_atomic(&dir.path().join("a"), b"1").unwrap();
        durable.write_atomic(&dir.path().join("b"), b"2").unwrap();
        durable.finish().unwrap();
        assert_eq!(fs::read(dir.path().join("a")).unwrap(), b"1");
        assert_eq!(fs::read(dir.path().join("b")).unwrap(), b"2");
        assert!(temp_files(dir.path()).is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn a_private_file_is_mode_0600_and_an_old_one_is_narrowed() {
        use std::os::unix::fs::PermissionsExt as _;
        let mode = |p: &Path| fs::metadata(p).unwrap().permissions().mode() & 0o777;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("secret");
        write_private(&path, b"key").unwrap();
        assert_eq!(mode(&path), 0o600);

        let old = dir.path().join("old");
        fs::write(&old, b"key").unwrap();
        fs::set_permissions(&old, fs::Permissions::from_mode(0o644)).unwrap();
        restrict_to_owner(&old).unwrap();
        assert_eq!(mode(&old), 0o600);
    }
}
