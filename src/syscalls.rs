use std::fs::File;
use std::os::unix::fs::FileExt;

use nix::fcntl::OFlag;
use nix::libc::user_regs_struct;
use nix::unistd::Pid;

use crate::event::Access;
use crate::resolve;

#[cfg(target_arch = "aarch64")]
mod nr {
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
}

#[cfg(target_arch = "x86_64")]
mod nr {
    pub const SYS_OPEN: u64 = 2;
    pub const SYS_STAT: u64 = 4;
    pub const SYS_LSTAT: u64 = 6;
    pub const SYS_ACCESS: u64 = 21;
    pub const SYS_EXECVE: u64 = 59;
    pub const SYS_RENAME: u64 = 82;
    pub const SYS_MKDIR: u64 = 83;
    pub const SYS_UNLINK: u64 = 87;
    pub const SYS_READLINK: u64 = 89;
    pub const SYS_OPENAT: u64 = 257;
    pub const SYS_MKDIRAT: u64 = 258;
    pub const SYS_NEWFSTATAT: u64 = 262;
    pub const SYS_UNLINKAT: u64 = 263;
    pub const SYS_RENAMEAT: u64 = 264;
    pub const SYS_READLINKAT: u64 = 267;
    pub const SYS_FACCESSAT: u64 = 269;
    pub const SYS_RENAMEAT2: u64 = 316;
    pub const SYS_EXECVEAT: u64 = 322;
    pub const SYS_STATX: u64 = 332;
    pub const SYS_FACCESSAT2: u64 = 439;
}

pub use nr::*;
const AT_FDCWD: u64 = (-100i64) as u64;

pub type Touch = (Access, String);

#[cfg(target_arch = "aarch64")]
pub fn nr_of(regs: &user_regs_struct) -> u64 {
    regs.regs[8]
}

#[cfg(target_arch = "aarch64")]
pub fn args_of(regs: &user_regs_struct) -> [u64; 6] {
    [regs.regs[0], regs.regs[1], regs.regs[2], regs.regs[3], regs.regs[4], regs.regs[5]]
}

#[cfg(target_arch = "aarch64")]
pub fn is_entry(regs: &user_regs_struct) -> bool {
    regs.regs[7] == 0
}

#[cfg(target_arch = "aarch64")]
pub fn ret_of(regs: &user_regs_struct) -> i64 {
    regs.regs[0] as i64
}

#[cfg(target_arch = "x86_64")]
pub fn nr_of(regs: &user_regs_struct) -> u64 {
    regs.orig_rax
}

#[cfg(target_arch = "x86_64")]
pub fn args_of(regs: &user_regs_struct) -> [u64; 6] {
    [regs.rdi, regs.rsi, regs.rdx, regs.r10, regs.r8, regs.r9]
}

#[cfg(target_arch = "x86_64")]
pub fn is_entry(regs: &user_regs_struct) -> bool {
    regs.rax as i64 == -38
}

#[cfg(target_arch = "x86_64")]
pub fn ret_of(regs: &user_regs_struct) -> i64 {
    regs.rax as i64
}

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

pub fn decode(pid: Pid, mem: &File, nr: u64, arg: &[u64]) -> Vec<Touch> {
    let mut out = Vec::new();
    let first = |kind: Access| at_path(pid, mem, arg[0], arg[1]).map(|p| (kind, p));
    #[cfg(target_arch = "x86_64")]
    let cwd_path = |kind: Access, ptr: u64| at_path(pid, mem, AT_FDCWD, ptr).map(|p| (kind, p));
    match nr {
        SYS_OPENAT => out.extend(first(kind_of(arg[2]))),
        SYS_NEWFSTATAT | SYS_STATX | SYS_FACCESSAT | SYS_FACCESSAT2 | SYS_READLINKAT => {
            out.extend(first(Access::Stat))
        }
        SYS_UNLINKAT => out.extend(first(Access::Remove)),
        SYS_MKDIRAT => out.extend(first(Access::Write)),
        SYS_RENAMEAT | SYS_RENAMEAT2 => {
            out.extend(first(Access::Remove));
            out.extend(at_path(pid, mem, arg[2], arg[3]).map(|p| (Access::Write, p)));
        }
        SYS_EXECVE => out.extend(at_path(pid, mem, AT_FDCWD, arg[0]).map(|p| (Access::Read, p))),
        SYS_EXECVEAT => out.extend(first(Access::Read)),
        #[cfg(target_arch = "x86_64")]
        SYS_OPEN => out.extend(cwd_path(kind_of(arg[1]), arg[0])),
        #[cfg(target_arch = "x86_64")]
        SYS_STAT | SYS_LSTAT | SYS_ACCESS | SYS_READLINK => {
            out.extend(cwd_path(Access::Stat, arg[0]))
        }
        #[cfg(target_arch = "x86_64")]
        SYS_UNLINK => out.extend(cwd_path(Access::Remove, arg[0])),
        #[cfg(target_arch = "x86_64")]
        SYS_MKDIR => out.extend(cwd_path(Access::Write, arg[0])),
        #[cfg(target_arch = "x86_64")]
        SYS_RENAME => {
            out.extend(cwd_path(Access::Remove, arg[0]));
            out.extend(cwd_path(Access::Write, arg[1]));
        }
        _ => {}
    }
    out
}
