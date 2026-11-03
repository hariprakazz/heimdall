use std::collections::HashMap;
use std::ffi::CString;
use std::fs::File;

use anyhow::{bail, Context, Result};
use nix::libc::{
    PTRACE_EVENT_CLONE, PTRACE_EVENT_EXEC, PTRACE_EVENT_FORK, PTRACE_EVENT_SECCOMP,
    PTRACE_EVENT_VFORK,
};
use nix::sys::ptrace;
use nix::sys::signal::{raise, Signal};
use nix::sys::wait::{waitpid, WaitPidFlag, WaitStatus};
use nix::unistd::{execvp, fork, ForkResult, Pid};

use crate::event::Event;
use crate::filter;
use crate::syscalls::{self, Touch};

struct Task {
    mem: File,
    pending: Vec<Touch>,
}

impl Task {
    fn open(pid: Pid) -> Result<Task> {
        let mem = File::open(format!("/proc/{pid}/mem"))?;
        Ok(Task { mem, pending: Vec::new() })
    }
}

fn task_of(tasks: &mut HashMap<Pid, Task>, pid: Pid) -> Result<&mut Task> {
    if !tasks.contains_key(&pid) {
        tasks.insert(pid, Task::open(pid)?);
    }
    Ok(tasks.get_mut(&pid).unwrap())
}

fn read_argv(pid: Pid) -> Vec<String> {
    std::fs::read(format!("/proc/{pid}/cmdline"))
        .map(|raw| {
            raw.split(|&b| b == 0)
                .filter(|part| !part.is_empty())
                .map(|part| String::from_utf8_lossy(part).into_owned())
                .collect()
        })
        .unwrap_or_default()
}

fn resume(pid: Pid, task: &Task, fast: bool, sig: Option<Signal>) -> nix::Result<()> {
    if fast && task.pending.is_empty() {
        ptrace::cont(pid, sig)
    } else {
        ptrace::syscall(pid, sig)
    }
}

fn on_syscall(pid: Pid, task: &mut Task, fast: bool, emit: &mut impl FnMut(Event)) -> Result<()> {
    let regs = ptrace::getregs(pid)?;
    if regs.regs[7] == 0 {
        if !fast {
            task.pending = syscalls::decode(pid, &task.mem, &regs.regs);
        }
        return Ok(());
    }
    let ret = regs.regs[0] as i64;
    let ok = ret >= 0;
    let errno = if ok { 0 } else { (-ret) as i32 };
    for (kind, path) in task.pending.drain(..) {
        emit(Event::Access { pid: pid.as_raw(), kind, path, ok, errno });
    }
    Ok(())
}

fn start(argv: &[String], fast: bool) -> Result<Pid> {
    let name = CString::new(argv[0].as_str())?;
    let args = argv
        .iter()
        .map(|arg| CString::new(arg.as_str()))
        .collect::<std::result::Result<Vec<_>, _>>()?;
    let mut bpf = filter::program();
    match unsafe { fork()? } {
        ForkResult::Child => {
            let _ = ptrace::traceme();
            if fast {
                let _ = raise(Signal::SIGSTOP);
                if filter::install(&mut bpf).is_err() {
                    unsafe { nix::libc::_exit(126) }
                }
            }
            let _ = execvp(&name, &args);
            unsafe { nix::libc::_exit(127) }
        }
        ForkResult::Parent { child } => Ok(child),
    }
}

pub fn run(argv: &[String], mut emit: impl FnMut(Event)) -> Result<i32> {
    let fast = std::env::var_os("HEIMDALL_FAST").is_some();
    let prog = argv.first().context("no command given")?;
    let root = start(argv, fast)?;

    let first = if fast { Signal::SIGSTOP } else { Signal::SIGTRAP };
    match waitpid(root, None)? {
        WaitStatus::Stopped(_, sig) if sig == first => {}
        WaitStatus::Exited(_, 127) => bail!("cannot start {prog}"),
        other => bail!("unexpected first stop: {other:?}"),
    }
    ptrace::setoptions(
        root,
        ptrace::Options::PTRACE_O_TRACESYSGOOD
            | ptrace::Options::PTRACE_O_EXITKILL
            | ptrace::Options::PTRACE_O_TRACEFORK
            | ptrace::Options::PTRACE_O_TRACEVFORK
            | ptrace::Options::PTRACE_O_TRACECLONE
            | ptrace::Options::PTRACE_O_TRACEEXEC
            | ptrace::Options::PTRACE_O_TRACESECCOMP,
    )?;

    let mut tasks: HashMap<Pid, Task> = HashMap::new();
    tasks.insert(root, Task::open(root)?);
    let mut root_code = 0;
    resume(root, &tasks[&root], fast, None)?;

    while !tasks.is_empty() {
        match waitpid(None, Some(WaitPidFlag::__WALL))? {
            WaitStatus::PtraceSyscall(pid) => {
                let task = task_of(&mut tasks, pid)?;
                on_syscall(pid, task, fast, &mut emit)?;
                resume(pid, task, fast, None)?;
            }
            WaitStatus::PtraceEvent(pid, _, ev) => {
                let task = task_of(&mut tasks, pid)?;
                if ev == PTRACE_EVENT_FORK || ev == PTRACE_EVENT_VFORK || ev == PTRACE_EVENT_CLONE {
                    let kid = ptrace::getevent(pid)? as i32;
                    emit(Event::Spawn { parent: pid.as_raw(), child: kid });
                } else if ev == PTRACE_EVENT_EXEC {
                    task.mem = File::open(format!("/proc/{pid}/mem"))?;
                    let path = std::fs::read_link(format!("/proc/{pid}/exe"))
                        .map(|p| p.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    emit(Event::Exec { pid: pid.as_raw(), path, argv: read_argv(pid) });
                } else if ev == PTRACE_EVENT_SECCOMP {
                    let regs = ptrace::getregs(pid)?;
                    task.pending = syscalls::decode(pid, &task.mem, &regs.regs);
                }
                resume(pid, task, fast, None)?;
            }
            WaitStatus::Stopped(pid, sig) => {
                let task = task_of(&mut tasks, pid)?;
                let fwd = if sig == Signal::SIGSTOP || sig == Signal::SIGTRAP {
                    None
                } else {
                    Some(sig)
                };
                resume(pid, task, fast, fwd)?;
            }
            WaitStatus::Exited(pid, code) => {
                emit(Event::Exit { pid: pid.as_raw(), code });
                tasks.remove(&pid);
                if pid == root {
                    root_code = code;
                }
            }
            WaitStatus::Signaled(pid, sig, _) => {
                let code = 128 + sig as i32;
                emit(Event::Exit { pid: pid.as_raw(), code });
                tasks.remove(&pid);
                if pid == root {
                    root_code = code;
                }
            }
            _ => {}
        }
    }
    Ok(root_code)
}
