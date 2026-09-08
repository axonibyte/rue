//! Peer credentials on the control channel (7.4): the connecting
//! process's effective uid as the kernel reports it (`SO_PEERCRED` on
//! Linux, `LOCAL_PEERCRED` on FreeBSD, `getpeereid` on macOS), and the
//! account name for a uid; on Windows the client's SID at the other end of
//! the named pipe, and the account name for a SID. Nothing here trusts
//! what the client says about itself.

#[cfg(unix)]
use std::io;
#[cfg(unix)]
use std::os::unix::io::AsRawFd;
#[cfg(unix)]
use std::os::unix::net::UnixStream;

/// The peer's effective uid and gid.
#[cfg(unix)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PeerCred {
    pub uid: u32,
    pub gid: u32,
}

#[cfg(target_os = "linux")]
pub fn peer_cred(s: &UnixStream) -> io::Result<PeerCred> {
    let mut cred = libc::ucred {
        pid: 0,
        uid: 0,
        gid: 0,
    };
    let mut len = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
    // SAFETY: SO_PEERCRED fills a ucred of the given length on the socket's
    // own descriptor.
    let rc = unsafe {
        libc::getsockopt(
            s.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            &mut cred as *mut libc::ucred as *mut libc::c_void,
            &mut len,
        )
    };
    if rc != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(PeerCred {
        uid: cred.uid,
        gid: cred.gid,
    })
}

#[cfg(any(target_os = "freebsd", target_os = "dragonfly"))]
pub fn peer_cred(s: &UnixStream) -> io::Result<PeerCred> {
    let mut cred: libc::xucred = unsafe { std::mem::zeroed() };
    let mut len = std::mem::size_of::<libc::xucred>() as libc::socklen_t;
    // SAFETY: LOCAL_PEERCRED fills an xucred of the given length on the
    // socket's own descriptor.
    let rc = unsafe {
        libc::getsockopt(
            s.as_raw_fd(),
            0,
            libc::LOCAL_PEERCRED,
            &mut cred as *mut libc::xucred as *mut libc::c_void,
            &mut len,
        )
    };
    if rc != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(PeerCred {
        uid: cred.cr_uid,
        gid: cred.cr_groups[0],
    })
}

#[cfg(any(
    target_os = "macos",
    target_os = "ios",
    target_os = "openbsd",
    target_os = "netbsd"
))]
pub fn peer_cred(s: &UnixStream) -> io::Result<PeerCred> {
    let mut uid: libc::uid_t = 0;
    let mut gid: libc::gid_t = 0;
    // SAFETY: getpeereid writes the peer's ids through the two pointers.
    let rc = unsafe { libc::getpeereid(s.as_raw_fd(), &mut uid, &mut gid) };
    if rc != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(PeerCred { uid, gid })
}

/// The account name for a uid, from the password database; `None` when
/// the uid has no entry.
#[cfg(unix)]
pub fn user_name(uid: u32) -> Option<String> {
    let mut buf = vec![0u8; 16 * 1024];
    let mut pw: libc::passwd = unsafe { std::mem::zeroed() };
    let mut result: *mut libc::passwd = std::ptr::null_mut();
    // SAFETY: getpwuid_r writes into the buffers of the sizes given and
    // sets result to null when there is no entry.
    let rc = unsafe {
        libc::getpwuid_r(
            uid,
            &mut pw,
            buf.as_mut_ptr() as *mut libc::c_char,
            buf.len(),
            &mut result,
        )
    };
    if rc != 0 || result.is_null() {
        return None;
    }
    // SAFETY: pw_name points into buf, NUL-terminated by getpwuid_r.
    let name = unsafe { std::ffi::CStr::from_ptr(pw.pw_name) };
    Some(name.to_string_lossy().into_owned())
}

/// This process's effective uid.
#[cfg(unix)]
pub fn my_uid() -> u32 {
    // SAFETY: geteuid has no preconditions.
    unsafe { libc::geteuid() }
}

/// The gid of a group named by name or by number; `None` when the system
/// knows no such group. The control socket belongs to it (7.4).
#[cfg(unix)]
pub fn gid_for(spec: &str) -> Option<u32> {
    if let Ok(n) = spec.parse::<u32>() {
        return Some(n);
    }
    let c = std::ffi::CString::new(spec).ok()?;
    // SAFETY: getgrnam reads a NUL-terminated name and returns a static
    // entry or null.
    let g = unsafe { libc::getgrnam(c.as_ptr()) };
    if g.is_null() {
        return None;
    }
    // SAFETY: g is a valid entry.
    Some(unsafe { (*g).gr_gid })
}

/// This process's own account name, whichever platform names it.
#[cfg(unix)]
pub fn my_account() -> Option<String> {
    user_name(my_uid())
}

/// The account this process runs as, from the operating system.
#[cfg(windows)]
pub fn my_account() -> Option<String> {
    use std::os::windows::ffi::OsStringExt;
    let mut buf = vec![0u16; 256];
    let mut len = buf.len() as u32;
    // SAFETY: the buffer is the size the length says.
    let ok = unsafe {
        windows_sys::Win32::System::WindowsProgramming::GetUserNameW(buf.as_mut_ptr(), &mut len)
    };
    if ok == 0 {
        return None;
    }
    let n = len.saturating_sub(1) as usize;
    Some(
        std::ffi::OsString::from_wide(&buf[..n.min(buf.len())])
            .to_string_lossy()
            .into_owned(),
    )
}
