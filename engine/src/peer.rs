//! Peer credentials on the control socket (7.4): the connecting process's
//! effective uid as the kernel reports it (`SO_PEERCRED` on Linux,
//! `LOCAL_PEERCRED` on FreeBSD, `getpeereid` on macOS), and the account
//! name for a uid. Nothing here trusts what the client says about itself.

#![cfg(unix)]

use std::io;
use std::os::unix::io::AsRawFd;
use std::os::unix::net::UnixStream;

/// The peer's effective uid and gid.
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
pub fn my_uid() -> u32 {
    // SAFETY: geteuid has no preconditions.
    unsafe { libc::geteuid() }
}
