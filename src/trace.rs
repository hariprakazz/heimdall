use std::collections::HashMap;
use std::fs::File;
use std::os::unix::process::CommandExt;
use std::process::Command;

use anyhow::{bail, Context, Result};
use nix::libc::{PTRACE_EVENT_CLONE, PTRACE_EVENT_EXEC, PTRACE_EVENT_FORK, PTRACE_EVENT_VFORK};
use nix::sys::ptrace;
use nix::sys::signal::Signal;
use nix::sys::wait::{waitpid, WaitPidFlag, WaitStatus};
use nix::unistd::Pid;

use crate::event::Event;
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

fn on_syscall(pid: Pid, task: &mut Task, emit: &mut impl FnMut(Event)) -> Result<()> {
    let regs = ptrace::getregs(pid)?;
    if regs.regs[7] == 0 {
        task.pending = syscalls::decode(pid, &task.mem, &regs.regs);
    } else {
        let ret = regs.regs[0] as i64;
        let ok = ret >= 0;
        let errno = if ok { 0 } else { (-ret) as i32 };
        for (kind, path) in task.pending.drain(..) {
            emit(Event::Access { pid: pid.as_raw(), kind, path, ok, errno });
        }
    }
    Ok(())
}

pub fn run(argv: &[String], mut emit: impl FnMut(Event)) -> Result<i32> {
    let (prog, rest) = argv.split_first().context("no command given")?;
    let mut cmd = Command::new(prog);
    cmd.args(rest);
    unsafe {
        cmd.pre_exec(|| ptrace::traceme().map_err(std::io::Error::from));
    }
    let child = cmd.spawn().with_context(|| format!("cannot start {prog}"))?;
    let root = Pid::from_raw(child.id() as i32);

    match waitpid(root, None)? {
        WaitStatus::Stopped(_, Signal::SIGTRAP) => {}
        other => bail!("unexpected first stop: {other:?}"),
    }
    ptrace::setoptions(
        root,
        ptrace::Options::PTRACE_O_TRACESYSGOOD
            | ptrace::Options::PTRACE_O_EXITKILL
            | ptrace::Options::PTRACE_O_TRACEFORK
            | ptrace::Options::PTRACE_O_TRACEVFORK
            | ptrace::Options::PTRACE_O_TRACECLONE
            | ptrace::Options::PTRACE_O_TRACEEXEC,
    )?;

    let mut tasks: HashMap<Pid, Task> = HashMap::new();
    tasks.insert(root, Task::open(root)?);
    let mut root_code = 0;
    ptrace::syscall(root, None)?;

    while !tasks.is_empty() {
        match waitpid(None, Some(WaitPidFlag::__WALL))? {
            WaitStatus::PtraceSyscall(pid) => {
                let task = task_of(&mut tasks, pid)?;
                on_syscall(pid, task, &mut emit)?;
                ptrace::syscall(pid, None)?;
            }
            WaitStatus::PtraceEvent(pid, _, ev) => {
                if ev == PTRACE_EVENT_FORK || ev == PTRACE_EVENT_VFORK || ev == PTRACE_EVENT_CLONE {
                    let kid = ptrace::getevent(pid)? as i32;
                    emit(Event::Spawn { parent: pid.as_raw(), child: kid });
                } else if ev == PTRACE_EVENT_EXEC {
                    let task = task_of(&mut tasks, pid)?;
                    task.mem = File::open(format!("/proc/{pid}/mem"))?;
                    let path = std::fs::read_link(format!("/proc/{pid}/exe"))
                        .map(|p| p.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    emit(Event::Exec { pid: pid.as_raw(), path, argv: read_argv(pid) });
                }
                ptrace::syscall(pid, None)?;
            }
            WaitStatus::Stopped(pid, sig) => {
                task_of(&mut tasks, pid)?;
                let fwd = if sig == Signal::SIGSTOP || sig == Signal::SIGTRAP {
                    None
                } else {
                    Some(sig)
                };
                ptrace::syscall(pid, fwd)?;
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
