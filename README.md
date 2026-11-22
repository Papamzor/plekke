# czkawka (hard fork)

Personal hard fork of [qarmin/czkawka](https://github.com/qarmin/czkawka): a fast, multithreaded Rust engine
for finding duplicates, big files, empty folders/files, similar images/videos/music, broken files and more.

This fork keeps the engine and the CLI, drops the GTK/Slint frontends, and is building a Tauri desktop app
styled with mwp-ui.

## Layout

| Crate | What |
|---|---|
| `czkawka_core` | Scanning engine (library). See [czkawka_core/AGENTS.md](czkawka_core/AGENTS.md). |
| `czkawka_cli` | Command-line frontend. See [instructions/Instruction_CLI.md](instructions/Instruction_CLI.md). |

## Build (Windows)

Needs the stable Rust MSVC toolchain (`winget install Rustlang.Rustup`) and Visual Studio Build Tools with the
C++ workload. Similar-video and video-optimizer tools also need `ffmpeg`/`ffprobe` on `PATH`.

```
cargo build --release -p czkawka_cli
target/release/czkawka_cli dup -d C:\some\folder
```

## License

Code is MIT, copyright (c) 2020-2026 Rafał Mikrut (see `czkawka_core/LICENSE_MIT`,
`czkawka_cli/LICENSE_MIT`). Test files in `czkawka_core/test_resources` are CC BY 4.0
(`czkawka_core/LICENSE_CC_BY_4_TEST_FILES`).
