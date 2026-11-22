# czkawka (hard fork) - Codebase Guide

Hard fork of qarmin/czkawka. Upstream is irrelevant: never pull from it, never shape changes for it.
Goal: `czkawka_core` as the scanning engine behind a Tauri desktop app styled with mwp-ui
(`../mwp-ui/mwp-ui`). Old GTK/Slint frontends are deleted; delete freely when code only served them.

All code, comments, commit messages and docs are in **English**.

## Workspace

```
czkawka_core/   # Scanning engine (library). Architecture: czkawka_core/AGENTS.md
czkawka_cli/    # CLI frontend, also the quickest smoke test for core changes
```

Edition 2024, Rust >= 1.94.1 (stable MSVC toolchain on Windows).

## Quality gate

Before calling a task done:

```
cargo +nightly fmt        # .rustfmt.toml uses nightly-only import grouping
cargo clippy --all-targets -- -D warnings
cargo test -p czkawka_core
cargo run --release -p czkawka_cli -- dup -d <some folder>   # smoke run, deletes nothing by default
```

## Output conventions

User-facing sizes in metric (SI) units: kB, MB, GB. Dates as DD/MM/YYYY.

## Rust Style

Applies to every Rust crate in the workspace. Crate-specific notes live in that crate's own `AGENTS.md`.

**Formatting & lints**
- Format with `cargo +nightly fmt`, then `cargo fmt` for stable compatibility
  (`.rustfmt.toml`: `max_width = 180`, granular import grouping `group_imports = "StdExternalCrate"`).
- Code must compile without clippy warnings. Suppress a lint locally only with a comment
  explaining why.

**Errors & panics**
- Prefer `?`, `ok_or_else`, `map_err` over panic-based flows.
- `unwrap()` only in tests.
- `expect()` in production only when BOTH hold: there is no correct recovery path, and
  continuing would likely hide a serious bug - always with a precise, actionable message.
- Never silently swallow errors with `let _ = ...` unless the failure is genuinely irrelevant.
- Model domain errors with explicit types (`thiserror`/enums), not raw strings.
- Wrap I/O errors with context (e.g. file path) instead of letting std's bare "Permission
  denied" propagate - without the path it's hard to find the source.
- Don't ignore `Result`; handle it explicitly.

**Code shape**
- Keep functions focused; once one grows past ~30-50 lines, extract its logical steps into
  separate, well-named functions. Never carve a long function into sections with comment
  headers or bare `{ ... }` scope blocks - a `// validate input` comment over a block is the
  signal to pull it out into `validate_input(...)`, not to fence it off.
- Name things by intent, not by container/type (`user_count`, not `vec_users_len`).
- Make invalid states unrepresentable: enums over strings, `Option<T>` over sentinel values
  (`-1`, empty string).
- Prefer iterators and collection methods over manual index-based loops.
- Prefer `#[derive(...)]` over manual impls when behavior is standard.
- Avoid unnecessary copies and allocations; prefer references and `Cow` where it helps.
- Prefer external crates over reimplementing them.

**Comments**
Keep comments short and minimal - a single terse line, not a paragraph. Code should be
self-documenting through clear naming. Add a comment only when the _why_ is not easily inferred
from reading the code - algorithmic choices, non-obvious constraints, workarounds for external
library bugs, etc. Do not restate what the code already says. Never use the `—`/`–` dash
characters in code, comments, or commit messages - plain `-` only (ASCII box-drawing diagrams are
the only exception).

Exception: `czkawka_core` is a library - every frontend in this workspace depends on it, and so do
external consumers who only read its public API, not its internals. Its `pub fn`/`pub struct`
surface warrants more explanation than the rest of the workspace; see `czkawka_core/AGENTS.md`.

**Tests, fuzzers, benchmarks**
- Cover as much code as possible with tests; keep them readable with explicit `assert_eq!` and
  input data close to the assertions.
- Add fuzzers/examples/benchmarks where it helps show usage, find bugs, or measure the
  performance impact of a change.

**`unsafe`**
Avoid whenever possible. Every `unsafe` block documents its invariants and why the safety
requirements hold.

**Files & logging**
- Keep source files at or below 500 lines; split large files into modules.
- Always log via `log` (`handsome_logger`); reach for `tracing` only if actually needed.

**Auto-vectorization**
LLVM vectorizes a loop when it can prove: no loop-carried dependency, no aliasing, no bounds
checks, and a favorable cost model.
- Enable it: `.iter()`/`.iter_mut()` + `.zip()` instead of index loops (drops bounds checks);
  `.chunks_exact(N)`/`.chunks_exact_mut(N)` + `.remainder()` for aligned SIMD chunk processing;
  `(&[T], &mut [T])` over `(&mut [T], &mut [T])` so LLVM can assume no aliasing; keep the loop
  body uniform (no per-element `if`/`match`, no early returns); use a reduction (`.sum()`/
  `.fold()`) instead of a running accumulator that depends on its own previous value.
- Kills it: `slice[i]` index loops, value-based `continue` inside an element loop, mutable
  state shared across iterations.
- Verify: inspect asm on godbolt.org for `vmulps`/`vpaddb`/`vpsubusb` vs scalar `mulss`/`movss`;
  measure the delta with `RUSTFLAGS="-C opt-level=3 -C no-vectorize-loops -C no-vectorize-slp"`;
  build with `RUSTFLAGS="-C target-cpu=native"` to unlock AVX2/AVX-512 on the host CPU.
- When LLVM won't vectorize, reach for explicit SIMD: `std::arch` (stable, arch-specific),
  `std::simd`/`portable_simd` (nightly, portable), or the stable `wide`/`pulp` crates.

**Performance micro-hints**
- Pre-allocate with `Vec::with_capacity(n)` / `HashMap::with_capacity(n)` when the count is
  roughly known - avoids the realloc ladder.
- Reuse buffers across calls (`buf.clear()`) instead of reallocating.
- For almost-always-short collections, consider `SmallVec`/`ArrayVec` (stack-allocated until
  they spill).
- Use the smallest enum discriminant that fits (`#[repr(u8)]`/`#[repr(u16)]`) for tighter
  packing and better cache density.
- Wrap Criterion bench inputs/outputs in `std::hint::black_box(...)` to stop dead-code
  elimination from invalidating the measurement.
