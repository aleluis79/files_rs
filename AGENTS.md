# AGENTS.md

## Project overview

This repository contains files_rs, a Rust terminal file manager with a Norton Commander-style dual-pane TUI. The app is built around a single interactive state machine and a small set of modules under src/.

## Key files

- [README.md](README.md) — user-facing overview, keybindings, installation, and shell integration.
- [src/main.rs](src/main.rs) — terminal setup, event loop, and exit-directory handoff.
- [src/app.rs](src/app.rs) — core app state, panel state, dialogs, search, and remote connection flows.
- [src/ui.rs](src/ui.rs) — rendering and TUI layout.
- [src/config.rs](src/config.rs) — config-file loading/saving for saved connections and theme selection.
- [src/theme.rs](src/theme.rs) — theme parsing, built-in themes, and custom theme files.
- [src/ops.rs](src/ops.rs) — recursive copy/remove operations.
- [src/transfer.rs](src/transfer.rs) — background transfer worker logic.
- [src/remote.rs](src/remote.rs) — remote connection and SCP-related behavior.

## Build and validation

Use these commands for local verification:

```bash
cargo test
cargo build --release
```

The project is small and mostly interactive, so prefer building and running the binary after changes that affect behavior, rendering, or file operations.

## Conventions to follow

- Keep the code aligned with the existing module boundaries. App state belongs in [src/app.rs](src/app.rs), rendering in [src/ui.rs](src/ui.rs), and file-system operations in [src/ops.rs](src/ops.rs).
- Prefer `anyhow::Result` and add `with_context(...)` messages for failures that surface to the user.
- Preserve the existing Spanish UI language and user-facing messages unless the change explicitly requires otherwise.
- Avoid breaking the shell integration flow that writes the exit directory through `NCRS_CHDIR_FILE`.
- Keep theme and config behavior backward-compatible. Changes to config shape or theme parsing should remain robust and documented.
- When editing remote transfer or file-operation logic, preserve the existing safety semantics and avoid changing behavior without a clear reason.

## Practical guidance for agents

- For behavior changes, inspect the relevant module first and keep the change localized.
- For TUI/UI changes, verify the rendering path and keyboard handling remain consistent with the existing layout.
- For config or theme changes, preserve the default files and current config locations under `~/.config/files-rs/`.
- If you add features, keep the documentation in [README.md](README.md) in sync when user-visible behavior changes.
