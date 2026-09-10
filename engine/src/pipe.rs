//! The Windows control channel, docs/ROADMAP.md 7.4 and 12: a named pipe
//! with a discretionary access-control list, and the client's own SID as
//! its identity.
//!
//! This is the Windows half of `control::serve` and `control::Client`.
//! Everything above the transport is shared with unix; what differs is
//! that a pipe carries an access-control list instead of a mode and a
//! group, and that the peer is named by a SID rather than a uid.
//!
//! The list grants the local `SYSTEM` account and the local
//! administrators group full access and the rue group read and write,
//! which is the pipe's answer to the socket's `0660` and group `rue`.
//! Everyone else is refused by the operating system before a byte is read.
//!
//! What wine can and cannot do with any of this is stated in
//! docs/TESTING.md: the suite runs here, and a call wine does not
//! implement is reported as the refusal it is, never worked around.

#![cfg(windows)]

use std::ffi::OsStr;
use std::io;
use std::os::windows::ffi::OsStrExt;
use std::os::windows::io::FromRawHandle;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use windows_sys::Win32::Foundation::{
    CloseHandle, GetLastError, LocalFree, ERROR_PIPE_CONNECTED, ERROR_PIPE_LISTENING, GENERIC_READ,
    GENERIC_WRITE, HANDLE, INVALID_HANDLE_VALUE,
};
use windows_sys::Win32::Security::Authorization::{
    ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
};
use windows_sys::Win32::Security::{
    GetTokenInformation, LookupAccountNameW, LookupAccountSidW, RevertToSelf, TokenUser, PSID,
    SECURITY_ATTRIBUTES, SID_NAME_USE, TOKEN_QUERY, TOKEN_USER,
};
use windows_sys::Win32::Storage::FileSystem::{CreateFileW, OPEN_EXISTING, PIPE_ACCESS_DUPLEX};
use windows_sys::Win32::System::Pipes::{
    ConnectNamedPipe, CreateNamedPipeW, ImpersonateNamedPipeClient, SetNamedPipeHandleState,
    PIPE_NOWAIT, PIPE_READMODE_BYTE, PIPE_TYPE_BYTE, PIPE_UNLIMITED_INSTANCES, PIPE_WAIT,
};
use windows_sys::Win32::System::Threading::{GetCurrentThread, OpenThreadToken};

use crate::control::{handle, Daemon, Peer, SharedWriter};

/// The pipe a path names. A path that is already a pipe name is used as
/// it stands; anything else contributes its file name, so the same
/// `--socket` argument serves both platforms.
pub fn pipe_name(path: &Path) -> String {
    let s = path.to_string_lossy().replace('/', "\\");
    if s.starts_with("\\\\.\\pipe\\") {
        return s;
    }
    let last = path
        .file_name()
        .map(|f| f.to_string_lossy().into_owned())
        .unwrap_or_else(|| "rue".to_string());
    format!("\\\\.\\pipe\\{last}")
}

fn wide(s: &str) -> Vec<u16> {
    OsStr::new(s).encode_wide().chain(Some(0)).collect()
}

fn from_wide(buf: &[u16]) -> String {
    let end = buf.iter().position(|c| *c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..end])
}

fn last_error() -> io::Error {
    io::Error::from_raw_os_error(unsafe { GetLastError() } as i32)
}

/// The SID of an account name, as SDDL spells it.
fn sid_string(account: &str) -> Option<String> {
    let name = wide(account);
    let mut sid = vec![0u8; 256];
    let mut sid_len = sid.len() as u32;
    let mut domain = vec![0u16; 256];
    let mut domain_len = domain.len() as u32;
    let mut kind: SID_NAME_USE = 0;
    // SAFETY: every buffer is sized by the length passed beside it.
    let ok = unsafe {
        LookupAccountNameW(
            std::ptr::null(),
            name.as_ptr(),
            sid.as_mut_ptr() as PSID,
            &mut sid_len,
            domain.as_mut_ptr(),
            &mut domain_len,
            &mut kind,
        )
    };
    if ok == 0 {
        return None;
    }
    let mut out: *mut u16 = std::ptr::null_mut();
    // SAFETY: the SID was just filled; the string is freed below.
    let ok = unsafe { ConvertSidToStringSidW(sid.as_ptr() as PSID, &mut out) };
    if ok == 0 || out.is_null() {
        return None;
    }
    // SAFETY: out is a NUL-terminated wide string LocalAlloc'd by the call.
    let text = unsafe {
        let mut n = 0isize;
        while *out.offset(n) != 0 {
            n += 1;
        }
        let s = String::from_utf16_lossy(std::slice::from_raw_parts(out, n as usize));
        LocalFree(out as *mut core::ffi::c_void);
        s
    };
    Some(text)
}

/// The access-control list the pipe carries: SYSTEM and the local
/// administrators in full, the rue group reading and writing, and no one
/// else at all. A group the system does not know is a refusal, never a
/// silently wider pipe.
pub fn sddl(group: &str) -> Result<String, String> {
    let mut acl = String::from("D:(A;;GA;;;SY)(A;;GA;;;BA)");
    match sid_string(group) {
        Some(sid) => {
            acl.push_str(&format!("(A;;GRGW;;;{sid})"));
            Ok(acl)
        }
        None => Err(format!(
            "the control pipe's group {group} is not an account this system knows"
        )),
    }
}

struct Descriptor(*mut core::ffi::c_void);

impl Drop for Descriptor {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // SAFETY: LocalAlloc'd by the conversion below.
            unsafe { LocalFree(self.0) };
        }
    }
}

fn security(group: &str) -> Result<(Descriptor, SECURITY_ATTRIBUTES), String> {
    let text = wide(&sddl(group)?);
    let mut sd: *mut core::ffi::c_void = std::ptr::null_mut();
    // SAFETY: the string is NUL-terminated; sd is freed by Descriptor.
    let ok = unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            text.as_ptr(),
            SDDL_REVISION_1,
            &mut sd,
            std::ptr::null_mut(),
        )
    };
    if ok == 0 {
        return Err(format!(
            "the control pipe's access-control list was refused: {}",
            last_error()
        ));
    }
    let attrs = SECURITY_ATTRIBUTES {
        nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: sd,
        bInheritHandle: 0,
    };
    Ok((Descriptor(sd), attrs))
}

/// The account at the other end of a connected pipe, by its own SID.
/// `None` when the platform will not say, which is a refusal to serve the
/// connection, never an anonymous one.
///
/// It is called only with a connected instance this module owns, which is
/// why it is private: every caller is in this file.
fn client_account(pipe: HANDLE) -> Option<String> {
    // SAFETY: the pipe is connected; RevertToSelf undoes this below.
    if unsafe { ImpersonateNamedPipeClient(pipe) } == 0 {
        return None;
    }
    let mut token: HANDLE = std::ptr::null_mut();
    // SAFETY: the thread is impersonating; the token is closed below.
    let ok = unsafe { OpenThreadToken(GetCurrentThread(), TOKEN_QUERY, 1, &mut token) };
    let name = if ok == 0 {
        None
    } else {
        let mut len = 0u32;
        // SAFETY: a null buffer with a zero length asks for the size.
        unsafe { GetTokenInformation(token, TokenUser, std::ptr::null_mut(), 0, &mut len) };
        let mut buf = vec![0u8; len.max(1) as usize];
        // SAFETY: the buffer is the size the call just asked for.
        let ok = unsafe {
            GetTokenInformation(
                token,
                TokenUser,
                buf.as_mut_ptr() as *mut core::ffi::c_void,
                len,
                &mut len,
            )
        };
        if ok == 0 {
            None
        } else {
            // SAFETY: the buffer holds a TOKEN_USER whose Sid points into it.
            let user = unsafe { &*(buf.as_ptr() as *const TOKEN_USER) };
            let mut name = vec![0u16; 256];
            let mut name_len = name.len() as u32;
            let mut domain = vec![0u16; 256];
            let mut domain_len = domain.len() as u32;
            let mut kind: SID_NAME_USE = 0;
            // SAFETY: both buffers are sized by the lengths beside them.
            let ok = unsafe {
                LookupAccountSidW(
                    std::ptr::null(),
                    user.User.Sid,
                    name.as_mut_ptr(),
                    &mut name_len,
                    domain.as_mut_ptr(),
                    &mut domain_len,
                    &mut kind,
                )
            };
            if ok == 0 {
                None
            } else {
                Some(from_wide(&name))
            }
        }
    };
    if !token.is_null() {
        // SAFETY: opened just above.
        unsafe { CloseHandle(token) };
    }
    // SAFETY: undoes the impersonation, whatever happened in between.
    unsafe { RevertToSelf() };
    name
}

/// Serve the named pipe until `stop` is set, one thread per connection.
pub fn serve(
    path: &Path,
    group: Option<String>,
    daemon: Arc<Daemon>,
    stop: Arc<AtomicBool>,
) -> io::Result<()> {
    let name = wide(&pipe_name(path));
    let group = group.unwrap_or_else(|| "rue".to_string());
    let (_sd, attrs) = security(&group).map_err(io::Error::other)?;
    let owner = crate::peer::my_account().unwrap_or_default();
    while !stop.load(Ordering::SeqCst) {
        // SAFETY: the name and the attributes outlive the call.
        let h = unsafe {
            CreateNamedPipeW(
                name.as_ptr(),
                PIPE_ACCESS_DUPLEX,
                PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_NOWAIT,
                PIPE_UNLIMITED_INSTANCES,
                64 * 1024,
                64 * 1024,
                0,
                &attrs,
            )
        };
        if h == INVALID_HANDLE_VALUE {
            return Err(last_error());
        }
        // Wait for a client, checking `stop` as the unix loop does.
        loop {
            if stop.load(Ordering::SeqCst) {
                // SAFETY: the handle is ours and still open.
                unsafe { CloseHandle(h) };
                return Ok(());
            }
            // SAFETY: h is a pipe instance with no client yet.
            let ok = unsafe { ConnectNamedPipe(h, std::ptr::null_mut()) };
            let err = unsafe { GetLastError() };
            if ok != 0 || err == ERROR_PIPE_CONNECTED {
                break;
            }
            if err == ERROR_PIPE_LISTENING {
                std::thread::sleep(Duration::from_millis(50));
                continue;
            }
            // SAFETY: the handle is ours and still open.
            unsafe { CloseHandle(h) };
            return Err(io::Error::from_raw_os_error(err as i32));
        }
        // Reads block from here on: the frames are newline-delimited.
        let mode = PIPE_READMODE_BYTE | PIPE_WAIT;
        // SAFETY: h is a connected pipe instance.
        unsafe { SetNamedPipeHandleState(h, &mode, std::ptr::null_mut(), std::ptr::null_mut()) };
        let account = client_account(h);
        let d = daemon.clone();
        let owner = owner.clone();
        // SAFETY: the handle is ours alone from here; the File owns it and
        // closes it, which is what releases the pipe instance. A raw
        // handle is not `Send`; a File is, so the conversion happens here
        // and the File is what crosses into the thread.
        let file = unsafe { std::fs::File::from_raw_handle(h as *mut _) };
        let reader_file = match file.try_clone() {
            Ok(f) => f,
            Err(_) => continue,
        };
        std::thread::spawn(move || {
            let peer = Peer {
                owner: account.as_deref() == Some(owner.as_str()),
                user: account,
                uid: None,
            };
            let writer: SharedWriter = Arc::new(Mutex::new(Box::new(file)));
            handle(std::io::BufReader::new(reader_file), writer, peer, d);
        });
    }
    Ok(())
}

/// Connect to the daemon's pipe.
pub fn connect(
    path: &Path,
) -> io::Result<(
    Box<dyn std::io::Read + Send>,
    Box<dyn std::io::Write + Send>,
)> {
    let name = wide(&pipe_name(path));
    // SAFETY: the name outlives the call; the handle becomes a File.
    let h = unsafe {
        CreateFileW(
            name.as_ptr(),
            GENERIC_READ | GENERIC_WRITE,
            0,
            std::ptr::null(),
            OPEN_EXISTING,
            0,
            std::ptr::null_mut(),
        )
    };
    if h == INVALID_HANDLE_VALUE {
        return Err(last_error());
    }
    // SAFETY: the handle is ours; the File owns it from here.
    let file = unsafe { std::fs::File::from_raw_handle(h as *mut _) };
    let read = file.try_clone()?;
    Ok((Box::new(read), Box::new(file)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_path_becomes_a_pipe_name_and_a_pipe_name_stays_one() {
        assert_eq!(pipe_name(Path::new(r"\\.\pipe\rue")), r"\\.\pipe\rue");
        assert_eq!(pipe_name(Path::new("rued.sock")), r"\\.\pipe\rued.sock");
        assert_eq!(
            pipe_name(Path::new(r"C:\ProgramData\rue\rued.sock")),
            r"\\.\pipe\rued.sock"
        );
        assert_eq!(
            pipe_name(Path::new("/var/run/rue/rued.sock")),
            r"\\.\pipe\rued.sock"
        );
    }

    #[test]
    fn the_access_control_list_names_system_the_administrators_and_the_group() {
        // A real Windows system names the local administrators group and
        // the list carries its well-known SID. Wine may not resolve a
        // well-known name at all; either way the list is never widened,
        // which is what the second half asserts.
        match sddl("Administrators") {
            Ok(acl) => {
                assert!(acl.starts_with("D:(A;;GA;;;SY)(A;;GA;;;BA)"), "{acl}");
                assert!(acl.contains("(A;;GRGW;;;S-1-5-32-544)"), "{acl}");
            }
            Err(e) => {
                assert!(e.contains("not an account this system knows"), "{e}");
                eprintln!(
                    "note: this platform does not resolve the local administrators group; \
                     the list is proven on a real machine in Phase 3W"
                );
            }
        }
        // A group the system does not know is a refusal, never a wider
        // pipe: the control channel is not opened to everyone by default.
        let err = sddl("no-such-group-rue-test").unwrap_err();
        assert!(err.contains("not an account this system knows"), "{err}");
    }

    #[test]
    fn this_process_has_an_account_name() {
        // The daemon compares a client's account with its own to decide
        // `:socket_owner`; without one, that identity can never match.
        assert!(crate::peer::my_account().is_some_and(|n| !n.is_empty()));
    }
}
