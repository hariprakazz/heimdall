# heimdall

[![ci](https://github.com/hariprakazz/heimdall/actions/workflows/ci.yml/badge.svg)](https://github.com/hariprakazz/heimdall/actions/workflows/ci.yml)

Heimdall runs a build under ptrace, records every file each process touches, and tells you where the real build disagrees with the dependency graph Ninja was given.

I wrote it because a build that works on my machine and breaks on yours usually has an input nobody declared. The build file says what a step should use. It says nothing about what the step really opens.

## What it does

Run a build once and save the trace:

    heimdall run --out trace.jsonl ninja -C build -j4

Then ask questions about the trace:

    heimdall actions trace.jsonl --dir build
    heimdall diff trace1.jsonl trace2.jsonl
    heimdall report trace.jsonl --root /path/to/project

`actions` matches each `sh -c` command to a Ninja edge, follows the process tree so a read made by gcc or ld counts for the edge that spawned it, and prints:

- undeclared reads per edge, with toolchain paths filtered out
- missing dependency edges, where an edge reads a file another edge writes without declaring it
- declared inputs that were never read, or only probed with stat
- a suggested `| path` addition to the build line for each finding

`diff` compares two traces of the same build, keyed by command string, and lists the commands whose reads, writes or removes changed between runs. That is how you catch a flaky step.

`report` is the older mode: every read outside a directory you name.

Setting the `HEIMDALL_FAST` environment variable to any value before `run` installs a seccomp filter so only the file syscalls stop for ptrace.

## Results

**libuv** (CMake and Ninja, 79 build steps, `-j4`)

- All 79 commands matched to an edge. 368446 of 369455 recorded accesses were attributed. The rest are Ninja's own, before it spawns a shell.
- Zero missing dependency edges, zero project level undeclared reads and zero unused declared inputs. An earlier version flagged the `libuv.so.1` symlink step as unused, because creating a symlink never opens its target. Heimdall now skips a declared input when the output of the edge is a symlink pointing at it.
- Two runs diffed with 0 commands differing. A slow trace against a fast mode trace also differed in 0 commands.
- A gcc compile traced under `strace -f` and under heimdall touched the same 228 paths.

**libpng** (CMake and Ninja, 52 steps)

Editing `pngusr.dfa` does not regenerate `pnglibconf.h` with the CMake and Ninja build. I reproduced it on libpng18 at commit a70313d (CMake 3.31.6, Ninja 1.12.1, Debian arm64). Uncommenting `@# define PNG_USER_WIDTH_MAX 65535` in `pngusr.dfa` and running `ninja -C b` prints "no work to do", and `b/pnglibconf.h` keeps 1000000. After `ninja -t clean` and a full rebuild it shows 65535. I reported it as [libpng #978](https://github.com/pnggroup/libpng/issues/978). I also opened a fix in [PR #979](https://github.com/pnggroup/libpng/pull/979), still open. Reading the CMake rules, the `pnglibconf.c` step passes `pngusr.dfa` to `options.awk` but does not list it in `DEPENDS`.

Heimdall found this because the steps that generate `pnglibconf.h` read files that the build graph does not list as inputs. After removing system headers, about 34 lines remain, and they come down to three groups:

- the generated `genout.cmake`, `gensrc.cmake` and `genchk.cmake` scripts
- `dfn.awk`, `options.awk` and `pngusr.dfa`
- `png.h`, `pngconf.h`, `pngpriv.h` and the per architecture `check.h` files

I confirmed a stale build for `pngusr.dfa` and for a file passed with `-DDFA_XTRA` (same steps, and a clean rebuild picked up the change). I also checked `dfn.awk` by hand. After touching it, Ninja only re-ran CMake, and `pnglibconf.out` and `intprefix.out` stayed older than the edited file. I would call that low severity, and it is mentioned in the same issue. I did not test the other files or the Autotools build.

Full write-up with the reproduction and the cause: [docs/libpng-stale-build.md](docs/libpng-stale-build.md)

**fmt** (CMake and Ninja, 79 build steps, `-j4`, commit 47fcd35, default configuration with its test executables)

A clean result. All 79 commands matched an edge, with zero missing dependency edges, zero unused declared inputs and zero project level undeclared reads. The 1411 undeclared (edge, path) pairs are all toolchain paths such as `/lib/aarch64-linux-gnu`, `/usr/bin` and the gcc libexec directory. This covers the files read in one full build and says nothing about incremental builds. The same commit was also traced on a GitHub ubuntu-24.04 x86_64 runner with the same result: 79 of 79 commands matched, zero missing edges, zero unused inputs and no project level reads. That run first listed four reads of `/usr/local/bin/cmake`, because the runner installs cmake there, so I added that directory to the toolchain paths.

**flatbuffers** (CMake and Ninja, 102 build steps, `-j4`, commit 5eb190f, default configuration with tests on)

A clean result. All 102 commands matched an edge, with zero missing dependency edges, zero unused declared inputs and zero project level undeclared reads. Eleven of the commands ran `flatc` to generate code from the project's `.fbs` files, so this does cover generated sources. The 1644 undeclared (edge, path) pairs are all toolchain paths. The check ran on aarch64.

**re2c** (CMake and Ninja, 805 build steps, `-j4`, commit d40e257)

Configured with `RE2C_FOR_BUILD` pointing at a prebuilt `re2c` and `RE2C_REBUILD_LEXERS=ON`, so the lexers are regenerated during the build. Parser regeneration with Bison was not exercised. All 805 commands matched an edge with zero missing dependency edges. The four lexer generator steps read the `re2c` binary that was passed in through that option and the copies under `bootstrap/`, and none of these reads are declared. I think the binary read is intended, since it is an external tool, and I did not report anything upstream. The one unused declared input is `VerifyGlobs.cmake_force`, a file CMake never creates. The check ran on aarch64.

**Overhead** on libuv with `-j4`, release build, average of 3 runs, measured on aarch64 only: 0.68s plain, 5.36s under ptrace (7.9x), 4.20s with the seccomp filter (6.2x). The fast mode is about 22% faster than plain ptrace, which helps but not by much, and I only measured it on libuv, not on x86_64.

## Limits

- Linux aarch64 and x86_64. Both run in CI. The real project checks were done on aarch64, except that fmt was also traced on an x86_64 runner. The syscall decoder is written per ABI. On aarch64 a 32 bit process gets through the seccomp filter untraced, and x32 on x86_64 is not handled.
- Ninja only. There is no Make support.
- Paths are compared as text. `/lib` and `/usr/lib` are the same directory on many systems, so two spellings of one file are treated as different files.
- Transitive dependencies are followed through declared inputs only. A path that depends on a dependency Ninja does not know about is still reported.
- Stat only probes are not judged as undeclared reads.
- CMake projects that re-check globbed source directories on every build show those directories as undeclared reads and one unused input, `VerifyGlobs.cmake_force`, which CMake never creates. I saw this on re2c and not on fmt.
- Toolchain paths are a fixed list of prefixes. A compiler or build tool installed somewhere else, such as `/usr/local/bin`, can show up as a project level read until its directory is added to the list.
- `diff` compares sets of reads, writes and removes. It cannot see a lost failed probe or a changed count, and it ignores a temp file that a step both writes and removes.
- Suggested additions are hints, not patches. Heimdall never reads `build.ninja`, so it cannot print a diff that applies. With a generated Ninja file such as CMake's, the real fix belongs in the generator.
- The symlink check reads the output from disk, so it only works while the build tree still exists when `actions` runs.
- Five real projects were checked (libuv, libpng, fmt, flatbuffers, re2c), all on aarch64 (fmt also on x86_64), and only libpng had a finding that I reported upstream.

## Building

I build and run inside a Linux arm64 container so the tracing works on a Mac:

    docker run -it --rm --platform linux/arm64 --cap-add=SYS_PTRACE \
      --security-opt seccomp=unconfined -v "$PWD":/work -w /work \
      rust:latest bash
    apt-get update && apt-get install -y strace cmake ninja-build
    cargo build --release
    cargo test

ptrace needs `SYS_PTRACE`, and the tests run real Ninja builds, so Ninja has to be installed.

## Layout

    src/main.rs        command line and the run subcommand
    src/trace.rs       ptrace loop
    src/filter.rs      seccomp program for the fast mode
    src/syscalls.rs    syscall decoder (aarch64, x86_64)
    src/resolve.rs     path cleanup against cwd and directory fds
    src/event.rs       trace events and their JSON lines format
    src/ninja.rs       build steps from Ninja's own tools
    src/declared.rs    declared inputs from the deps log and ninja query
    src/attribute.rs   the actions report
    src/diff.rs        two run comparison
    src/report.rs      reads outside a root directory
    tests/             planted fixtures, each with a catching case and a quiet control

## License

MIT, see the LICENSE file.
