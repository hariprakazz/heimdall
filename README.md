# heimdall
A runtime hermeticity checker for builds. Heimdall runs your build command, watches which files it actually touches, and compares that against what the project declares. Anything read from outside the declared boundary is reported as a hermeticity violation.

Status: design stage. No implementation yet. Everything below describes what v0 is meant to do, not what works today.

Why

Build files like CMakeLists.txt, Makefile and dub.json describe what a build should use. They do not stop a compiler from quietly reading /usr/include/foo.h or a file two directories up. Those undeclared reads are why a build passes on one machine and breaks on another, and why incremental builds go stale.

Heimdall ignores what the config says and records what the build does.

How it works
Spawn the build command under ptrace, following forks and execs so child tools (gcc, ld, as) stay traced.
Intercept file syscalls (openat, execve, and friends) and read the path argument from the tracee's memory.
Resolve each path to a canonical absolute form, using the directory fd or the tracee's cwd for relative paths.
Build a graph in petgraph: processes and files as nodes, reads and writes as edges.
Compare every read against the declared workspace boundary and report violations.
Planned usage
heimdall ninja
heimdall make -j8
heimdall --root ./src --report violations.json ninja

Planned output: a list of violations, each with the offending path, the process that touched it, and the command line that spawned that process.

Scope for v0
Linux x86_64 only
ptrace backend via the nix crate
Report mode only, no blocking
Declared boundary taken from a CLI flag, not parsed from build files
Later
eBPF backend through aya for lower overhead
Reading declared inputs from Ninja and DUB build graphs
Blocking mode that kills the build on the first violation
Layout
heimdall/
├── Cargo.toml
└── src/
    ├── main.rs     process orchestration and CLI
    ├── trace.rs    ptrace loop and syscall decoding
    └── graph.rs    graph construction and boundary checks
Build
cargo build --release
License

TBD
