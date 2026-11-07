# A ptrace tool found a stale build in libpng's CMake setup

## What Heimdall does

Heimdall runs a build under ptrace and records every file each process touches. It attributes those accesses to Ninja edges and reports where the real build differs from the declared graph: undeclared reads, missing edges, and declared inputs that are never read.

I pointed it at libpng to see whether it would find anything on a real project.

## The finding

Editing `pngusr.dfa` does not regenerate `pnglibconf.h` with the CMake and Ninja build.

Tested on the libpng18 branch, commit a70313d, with CMake 3.31.6 and Ninja 1.12.1 on Debian arm64.

~~~
cmake -S . -B b -G Ninja -DPNG_TESTS=OFF
ninja -C b
grep 'define PNG_USER_WIDTH_MAX' b/pnglibconf.h
~~~

This shows `#define PNG_USER_WIDTH_MAX 1000000`. Then I uncommented the line `@# define PNG_USER_WIDTH_MAX 65535` in `pngusr.dfa` and rebuilt.

~~~
ninja -C b
grep 'define PNG_USER_WIDTH_MAX' b/pnglibconf.h
~~~

Ninja prints "no work to do" and the header still says 1000000. After `ninja -C b -t clean` and a full rebuild it says 65535. Restoring the file and doing the same again gives 1000000.

So an incremental build silently keeps the old configuration. I reported it as [libpng #978](https://github.com/pnggroup/libpng/issues/978).

## Why it happens

Ninja decides what to rebuild from the inputs each edge declares. If none of the declared inputs changed, the edge is skipped.

In libpng's CMake files the `pnglibconf.c` step runs `options.awk` over `pngusr.dfa` and over `DFA_XTRA` (`scripts/cmake/gensrc.cmake.in`, around line 65). The `generate_source` call for that output in `CMakeLists.txt` lists only `options.awk`, `pnglibconf.dfa` and `pngconf.h` in `DEPENDS`. The `generate_source` helper in `scripts/cmake/PNGGenConfig.cmake` passes only that list to `add_custom_command` and adds nothing implicit.

So the command reads `pngusr.dfa`, but the graph never learns that. Heimdall flagged it because its trace showed the file being read by the generation steps while the Ninja graph did not list it.

The same trace listed about 34 other project level reads in those steps: the generated `genout.cmake`, `gensrc.cmake` and `genchk.cmake` scripts, `dfn.awk`, `options.awk`, `png.h`, `pngconf.h`, `pngpriv.h` and the per architecture `check.h` files.

## What is proven and what is not

I confirmed a stale build for `pngusr.dfa` and for a file passed with `-DDFA_XTRA`.

- `dfn.awk`: after touching it, Ninja only re-ran CMake, and `pnglibconf.out` and `intprefix.out` stayed older than the edited file. I would call that low severity.
- The other undeclared reads are listed by the trace, but I did not test whether editing them leaves a stale output.
- `-DDFA_XTRA`: I configured a second build directory with `-DDFA_XTRA=/tmp/xtra.dfa`, then changed that file to contain `@# define PNG_USER_WIDTH_MAX 65535`. Ninja said no work to do while the file was newer than `pnglibconf.h`, which kept 1000000. A clean rebuild gave 65535.
- I did not test the Autotools build.
- Heimdall only runs on aarch64 Linux and only reads Ninja builds. I have checked it on three real projects: libuv and fmt were clean, libpng had the findings above.

If the libpng maintainers reply, I will update this page with what they say.

## Overhead

Measured on libuv with `-j4`, release build, average of 3 runs.

| Mode | Time | Slowdown |
| --- | --- | --- |
| plain build | 0.68s | 1x |
| ptrace | 5.36s | 7.9x |
| ptrace with seccomp filter (`HEIMDALL_FAST`) | 4.20s | 6.2x |

The seccomp mode is about 22% faster than plain ptrace. That helps, but it is still a large cost, and I only measured it on libuv.

## Other limits

- Paths are compared lexically, so two spellings of the same file can look different.
- Stat-only probes are not judged as reads.
- The two run diff compares sets of files, not order or counts.
- Suggested `| path` additions are hints. Heimdall never reads `build.ninja`.
- The symlink output check needs the output file to exist.
