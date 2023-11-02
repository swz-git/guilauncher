# GUI launcher

A RLBotGUI launcher

## Compiling

### Windows

Make sure you have the [rust toolchain installed](https://rustup.rs/). Build using `cargo build --release`

### Cross-compiling on linux

Make sure you have the [rust toolchain](https://rustup.rs/) and [cargo-xwin](https://github.com/rust-cross/cargo-xwin) installed. Build using `cargo xwin build --target x86_64-pc-windows-msvc --release`

## Improvements over old batch installer

*  Self-updater
*  Rust (more reliable, faster)
*  Probably more anti-virus friendly (no bat2exe)