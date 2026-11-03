use std::io;

use nix::libc::{self, sock_filter, sock_fprog};

use crate::syscalls::{
    SYS_EXECVE, SYS_EXECVEAT, SYS_FACCESSAT, SYS_FACCESSAT2, SYS_MKDIRAT, SYS_NEWFSTATAT,
    SYS_OPENAT, SYS_READLINKAT, SYS_RENAMEAT, SYS_RENAMEAT2, SYS_STATX, SYS_UNLINKAT,
};

const AUDIT_ARCH_AARCH64: u32 = 0xC000_00B7;
const LD_ABS: u16 = 0x20;
const JEQ_K: u16 = 0x15;
const RET_K: u16 = 0x06;
const RET_ALLOW: u32 = 0x7fff_0000;
const RET_TRACE: u32 = 0x7ff0_0000;

const WATCHED: [u64; 12] = [
    SYS_OPENAT,
    SYS_NEWFSTATAT,
    SYS_STATX,
    SYS_FACCESSAT,
    SYS_FACCESSAT2,
    SYS_READLINKAT,
    SYS_UNLINKAT,
    SYS_MKDIRAT,
    SYS_RENAMEAT,
    SYS_RENAMEAT2,
    SYS_EXECVE,
    SYS_EXECVEAT,
];

fn op(code: u16, jt: u8, jf: u8, k: u32) -> sock_filter {
    sock_filter { code, jt, jf, k }
}

pub fn program() -> Vec<sock_filter> {
    let count = WATCHED.len();
    let mut prog = vec![
        op(LD_ABS, 0, 0, 4),
        op(JEQ_K, 0, (count + 1) as u8, AUDIT_ARCH_AARCH64),
        op(LD_ABS, 0, 0, 0),
    ];
    for (pos, &nr) in WATCHED.iter().enumerate() {
        prog.push(op(JEQ_K, (count - pos) as u8, 0, nr as u32));
    }
    prog.push(op(RET_K, 0, 0, RET_ALLOW));
    prog.push(op(RET_K, 0, 0, RET_TRACE));
    prog
}

pub fn install(prog: &mut [sock_filter]) -> io::Result<()> {
    let fprog = sock_fprog { len: prog.len() as u16, filter: prog.as_mut_ptr() };
    let locked = unsafe { libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) };
    if locked != 0 {
        return Err(io::Error::last_os_error());
    }
    let res = unsafe {
        libc::syscall(libc::SYS_seccomp, libc::SECCOMP_SET_MODE_FILTER, 0, &fprog as *const sock_fprog)
    };
    if res != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn verdict(prog: &[sock_filter], arch: u32, nr: u32) -> u32 {
        let mut acc = 0u32;
        let mut pc = 0usize;
        loop {
            let ins = &prog[pc];
            pc += 1;
            match ins.code {
                LD_ABS => acc = if ins.k == 4 { arch } else { nr },
                JEQ_K => pc += if acc == ins.k { ins.jt as usize } else { ins.jf as usize },
                RET_K => return ins.k,
                other => panic!("unexpected opcode {other}"),
            }
        }
    }

    #[test]
    fn program_length_follows_watch_list() {
        assert_eq!(program().len(), WATCHED.len() + 5);
    }

    #[test]
    fn every_watched_syscall_is_traced() {
        let prog = program();
        for &nr in WATCHED.iter() {
            assert_eq!(verdict(&prog, AUDIT_ARCH_AARCH64, nr as u32), RET_TRACE, "nr {nr}");
        }
    }

    #[test]
    fn other_syscalls_run_free() {
        let prog = program();
        for nr in [0u32, 63, 64, 93, 222, 226, 435] {
            assert_eq!(verdict(&prog, AUDIT_ARCH_AARCH64, nr), RET_ALLOW, "nr {nr}");
        }
    }

    #[test]
    fn foreign_arch_is_not_trapped() {
        let prog = program();
        assert_eq!(verdict(&prog, 0x4000_0028, SYS_OPENAT as u32), RET_ALLOW);
    }
}
