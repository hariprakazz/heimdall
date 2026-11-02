use std::fs::File;
use std::os::unix::fs::FileExt;
use std::os::unix::process::CommandExt;
use std::process::Command;

use anyhow::{bail, Context, Result};
use nix::fcntl::OFlag;
use nix::sys::ptrace;
use nix::sys::signal::Signal;
use nix::sys::wait::{waitpid, WaitStatus};
use nix::unistd::Pid;

use crate::event::{Access, Event};

const SYS_OPENAT: u64 = 56;

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

pub fn run(argv: &[String], mut emit: impl FnMut(Event)) -> Result<i32> {
    let (prog, rest) = argv.split_first().context("no command given")?;
    let mut cmd = Command::new(prog);
    cmd.args(rest);
    unsafe {
        cmd.pre_exec(|| ptrace::traceme().map_err(std::io::Error::from));
    }
    let child = cmd.spawn().with_context(|| format!("cannot start {prog}"))?;
    let pid = Pid::from_raw(child.id() as i32);

    match waitpid(pid, None)? {
        WaitStatus::Stopped(_, Signal::SIGTRAP) => {}
        other => bail!("unexpected first stop: {other:?}"),
    }
    ptrace::setoptions(
        pid,
        ptrace::Options::PTRACE_O_TRACESYSGOOD | ptrace::Options::PTRACE_O_EXITKILL,
    )?;
    let mem = File::open(format!("/proc/{}/mem", pid))?;

    let mut in_call = false;
    let mut pending: Option<(Access, String)> = None;
    ptrace::syscall(pid, None)?;

    loop {
        match waitpid(pid, None)? {
            WaitStatus::PtraceSyscall(_) => {
                let regs = ptrace::getregs(pid)?;
                if !in_call {
                    pending = None;
                    if regs.regs[8] == SYS_OPENAT {
                        if let Some(path) = read_cstr(&mem, regs.regs[1]) {
                            pending = Some((kind_of(regs.regs[2]), path));
                        }
                    }
                } else if let Some((kind, path)) = pending.take() {
                    let ok = (regs.regs[0] as i64) >= 0;
                    emit(Event::Access { pid: pid.as_raw(), kind, path, ok });
                }
                in_call = !in_call;
                ptrace::syscall(pid, None)?;
            }
            WaitStatus::Exited(_, code) => {
                emit(Event::Exit { pid: pid.as_raw(), code });
                return Ok(code);
            }
            WaitStatus::Signaled(_, sig, _) => bail!("child killed by {sig}"),
            WaitStatus::Stopped(_, sig) => {
                ptrace::syscall(pid, Some(sig))?;
            }
            _ => {
                ptrace::syscall(pid, None)?;
            }
        }
    }
}
