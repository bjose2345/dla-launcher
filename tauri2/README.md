# Tauri application

This directory contains the active DLA Launcher product implementation.

```text
crates/       Rust domain, application, and native adapters
src-tauri/    Tauri composition root, commands, configuration, and assets
ui/           Thin frontend gateway and router around shared/ui
scripts/      Platform verification and Android release tooling
tests/        Synthetic native test applications
```

From this directory:

```bash
cargo tauri dev
cargo tauri build --ci
```

See [the root build guide](../docs/BUILDING.md) for prerequisites, verification,
Windows packaging, Android targets, signing, and archive-tool configuration.
