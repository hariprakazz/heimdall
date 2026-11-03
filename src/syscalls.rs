use std::fs::File;
use std::os::unix::fs::FileExt;

use nix::fcntl::OFlag;
use nix::unistd::Pid;

use crate::event::Access;
use crate::resolve;

pub const SYS_MKDIRAT: u64 = 34;
pub const SYS_UNLINKAT: u64 = 35;
pub const SYS_RENAMEAT: u64 = 38;
pub const SYS_FACCESSAT: u64 = 48;
pub const SYS_OPENAT: u64 = 56;
pub const SYS_READLINKAT: u64 = 78;
pub const SYS_NEWFSTATAT: u64 = 79;
pub const SYS_EXECVE: u64 = 221;
pub const SYS_RENAMEAT2: u64 = 276;
pub const SYS_EXECVEAT: u64 = 281;
pub const SYS_STATX: u64 = 291;
pub const SYS_FACCESSAT2: u64 = 439;
const AT_FDCWD: u64 = (-100i64) as u64;

pub type Touch = (Access, String);

fn read_cstr(mem: &File, addr: u64) -> Option<String> {
    let mut out = Vec::new();
    let mut chunk = [0u8; 256];
    let mut off = 0u64;
    loop {
        let got = mem.read_at(&mut chunk, addr + off).ok()?;
        if got == 0 {
            return None;
        }
        if let Some(end) = chunk[..got].iter().position(|&b| b == 0) {
            out.extend_from_slice(&chunk[..end]);
            return Some(String::from_utf8_lossy(&out).into_owned());
        }
        out.extend_from_slice(&chunk[..got]);
        off += got as u64;
        if out.len() > 4096 {
            return None;
        }
    }
}

fn kind_of(flags: u64) -> Access {
    let fl = OFlag::from_bits_truncate(flags as i32);
    let writes = OFlag::O_WRONLY | OFlag::O_RDWR | OFlag::O_CREAT | OFlag::O_TRUNC;
    if fl.intersects(writes) {
        Access::Write
    } else {
        Access::Read
    }
}

fn at_path(pid: Pid, mem: &File, dirfd: u64, ptr: u64) -> Option<String> {
    let raw = read_cstr(mem, ptr)?;
    if raw.is_empty() {
        return None;
    }
    Some(resolve::absolute(pid, dirfd as i32, &raw))
}

pub fn decode(pid: Pid, mem: &File, reg: &[u64; 31]) -> Vec<Touch> {
    let mut out = Vec::new();
    let first = |kind: Access| at_path(pid, mem, reg[0], reg[1]).map(|p| (kind, p));
    match reg[8] {
        SYS_OPENAT => out.extend(first(kind_of(reg[2]))),
        SYS_NEWFSTATAT | SYS_STATX | SYS_FACCESSAT | SYS_FACCESSAT2 | SYS_READLINKAT => {
            out.extend(first(Access::Stat))
        }
        SYS_UNLINKAT => out.extend(first(Access::Remove)),
        SYS_MKDIRAT => out.extend(first(Access::Write)),
        SYS_RENAMEAT | SYS_RENAMEAT2 => {
            out.extend(first(Access::Remove));
            out.extend(at_path(pid, mem, reg[2], reg[3]).map(|p| (Access::Write, p)));
        }
        SYS_EXECVE => out.extend(at_path(pid, mem, AT_FDCWD, reg[0]).map(|p| (Access::Read, p))),
        SYS_EXECVEAT => out.extend(first(Access::Read)),
        _ => {}
    }
    out
}
