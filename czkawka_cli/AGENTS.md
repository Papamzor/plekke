# czkawka_cli – Architecture Guide

## Overview

`czkawka_cli` is a thin CLI wrapper around `czkawka_core`. It handles argument
parsing, thread orchestration, progress rendering, and result output. All
scanning logic lives in `czkawka_core`; this crate only wires it to the
terminal.

---

## Source Layout

```
czkawka_cli/src/
├── main.rs       # Entry point, thread spawning, tool dispatchers
├── commands.rs   # All clap CLI argument definitions
├── parsers.rs    # parse_* clap value-parser functions used by commands.rs
└── progress.rs   # indicatif progress bar rendering
```

---

## Execution Model

```
main()
 ├─ Parse args (clap)
 ├─ Create crossbeam channel  (Sender<ProgressData>, Receiver<ProgressData>)
 ├─ Create Arc<AtomicBool> stop_flag
 ├─ spawn calculation_thread → dispatches to one of 14 tool functions
 ├─ Register Ctrl+C handler → sets stop_flag = true
 ├─ connect_progress(&progress_receiver)   [blocks, renders progress bar]
 └─ join calculation_thread → print output → exit(11) if items found
```

The **main thread** renders progress. The **calculation thread** runs the scan.
`progress_receiver.recv()` exits naturally when the sender drops (scan ends).

---

## CLI Subcommands

Defined via `clap` derive in `commands.rs`. Top-level enum `Commands` has 14
variants:

| Subcommand        | Alias            | Purpose                                 |
|-------------------|------------------|-----------------------------------------|
| `Duplicates`      | `dup`            | Find duplicate files (hash/name/size)   |
| `EmptyFolders`    | `empty-folders`  | Find empty directories                  |
| `BiggestFiles`    | `big`            | Find biggest or smallest files          |
| `EmptyFiles`      | `empty-files`    | Find zero-byte files                    |
| `Temporary`       | `temp`           | Find temporary files                    |
| `SimilarImages`   | `image`          | Find similar images (perceptual hash)   |
| `SameMusic`       | `music`          | Find duplicate music (tags/fingerprint) |
| `InvalidSymlinks` | `symlinks`       | Find broken symlinks                    |
| `BrokenFiles`     | `broken`         | Find broken PDF/audio/image/archive     |
| `SimilarVideos`   | `video`          | Find similar videos (frame hashing)     |
| `BadExtensions`   | `ext`            | Find files with wrong extensions        |
| `BadNames`        | `bad-names`      | Find problematic file names             |
| `VideoOptimizer`  | `video-optimizer`| Transcode or crop videos                |
| `ExifRemover`     | `exif-remover`   | Remove EXIF metadata from images        |

### `CommonCliItems` (shared by every subcommand)

- `-d` directories (required)
- `-e` excluded directories
- `-E` excluded items (glob patterns, e.g. `*/.*`)
- `-x` allowed extensions (supports macros: `IMAGE`, `VIDEO`, `MUSIC`, `TEXT`)
- `-P` excluded extensions
- `-R` disable recursive search
- `-X` (unix only) exclude other filesystems
- `-f` save text results to file
- `-C` save compact JSON results to file
- `--pretty-file-to-save` (`-p`) save pretty JSON results to file
- `-N` / `-M` suppress result/message output
- `-W` suppress non-zero exit code when items found
- `-T` thread count (0 = all CPUs)
- `-H` disable cache

### Delete methods

**`DMethod`** (for similarity-grouping tools): `-D NONE|AEN|AEO|ON|OO|AEB|AES|OB|OS|HARD`
**`SDMethod`** (for simple tools): `-D` delete flag, `-Q` dry-run, `-y` move-to-trash

---

## Tool Dispatcher Pattern

Every dispatch function builds its tool and hands it to the shared `run` helper:

```rust
fn tool_name(args: &ToolArgs, stop_flag: &Arc<AtomicBool>, progress_sender: &Sender<ProgressData>) -> CliOutput {
    let mut tool = ToolType::new(ToolParameters::new(args.some_field, ...));
    args.file_size.apply(&mut tool);                 // FileSizeArgs tools only
    set_delete(&mut tool, args.delete_method.into()); // DMethod, or SDMethod via `.into()`
    run(tool, &args.common_cli_items, &reference_dirs_or_empty, stop_flag, progress_sender, |tool| {
        // optional fix step (rename/remove EXIF/transcode), `|_| {}` when none
    })
}
```

`run` applies the `CommonCliItems` settings (threads, paths, extensions, cache),
scans, runs the fix closure and returns `save_and_write_results_to_writer`.

---

## Progress Rendering (`progress.rs`)

`connect_progress(&progress_receiver)` loops on `progress_receiver.recv()` and
renders via indicatif:

- **Spinner** while collecting files or loading/saving cache.
- **Linear bar** `[=>  ]` once the total entry or byte count is known.

The stage label (e.g. "Calculating hashes", "Reading tags") is derived from the
`CurrentStage` enum on each `ProgressData` message.

---

## Result Output

`save_and_write_results_to_writer` writes three optional file formats (text,
compact JSON, pretty JSON) and buffers stdout output, which is printed after the
thread join. The `CliOutput.found_any_files` flag drives the exit code:

- `exit(11)` when items are found (and `-W` is not set) - useful in scripts.
- `exit(0)` otherwise.

---

## Cancellation

`Arc<AtomicBool>` (SeqCst ordering) is shared between the Ctrl+C signal handler
and the calculation thread. The tools in `czkawka_core` poll it during scanning
and stop gracefully when it is `true`.

---

## Key Dependencies

| Crate              | Purpose                          |
|--------------------|----------------------------------|
| `clap` 4.5         | CLI parsing (derive API)         |
| `indicatif` 0.18   | Progress bars                    |
| `crossbeam-channel`| Unbounded channel for progress   |
| `ctrlc` 3.4        | SIGINT / Ctrl+C handling         |
| `humansize` 2.1    | Human-readable byte sizes        |
| `czkawka_core`     | All scanning logic               |


## Comments

Short and minimal - see root `AGENTS.md`. Only add a comment where the code's behavior can't be
inferred from reading it (a non-obvious constraint, a workaround, a cross-module coupling);
never restate what the code already says.
