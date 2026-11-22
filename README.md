# Plekke

Plekke is a Windows desktop app for finding duplicates and clutter. It is a personal hard fork of [qarmin/czkawka](https://github.com/qarmin/czkawka): a fast, multithreaded Rust engine
for finding duplicates, big files, empty folders/files, similar images/videos/music, broken files and more.

This fork keeps the engine and the CLI, drops the GTK/Slint frontends, and adds Plekke, a Tauri desktop app styled
with mwp-ui. The app covers duplicates, big files, empty folders and empty files; selected results go to the
Recycle Bin, never straight to deletion.

## Layout

| Crate | What |
|---|---|
| `czkawka_core` | Scanning engine (library). See [czkawka_core/AGENTS.md](czkawka_core/AGENTS.md). |
| `czkawka_cli` | Command-line frontend. See [instructions/Instruction_CLI.md](instructions/Instruction_CLI.md). |
| `czkawka_app` | Plekke, the Tauri v2 desktop app; the page in `czkawka_app/ui/` uses a vendored copy of mwp-ui. |

## Build (Windows)

Needs the stable Rust MSVC toolchain (`winget install Rustlang.Rustup`), Visual Studio Build Tools with the
C++ workload, and the WebView2 runtime (part of Windows 11). Similar-video and video-optimizer tools also need
`ffmpeg`/`ffprobe` on `PATH`.

```
cargo run -p czkawka_app                       # Plekke
cargo build --release -p czkawka_cli
target/release/czkawka_cli dup -d C:\some\folder
```

An installer comes from `cargo tauri build` inside `czkawka_app/` (after `cargo install tauri-cli --version "^2"`).

## License

Code is MIT, copyright (c) 2020-2026 Rafał Mikrut (see `czkawka_core/LICENSE_MIT`,
`czkawka_cli/LICENSE_MIT`). Test files in `czkawka_core/test_resources` are CC BY 4.0
(`czkawka_core/LICENSE_CC_BY_4_TEST_FILES`).
