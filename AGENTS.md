# Repository Guidelines

## Project Structure & Module Organization

This crate is the `claude_clone` example inside the GPUI component ecosystem. The entry point is `src/main.rs`, which initializes GPUI, app state, dock layout, panels, settings, and menus.

Core UI modules live in `src/`: `chat_view.rs`, `conversation_panel.rs`, `sidebar.rs`, `side_panel.rs`, `dialogs.rs`, `settings_window.rs`, `provider_settings.rs`, `theme.rs`, and `titlebar.rs`. Data and persistence live in `models.rs`, `store.rs`, and `panel_data.rs`. Provider, mock, and MCP integrations live in `genai_backend.rs`, `mock_backend.rs`, and `mcp_backend.rs`. Document parsing and optional OCR support are in `document_parser.rs`; updater, i18n, menus, search, and system-file helpers have dedicated modules.

Integration tests are in `tests/`. App icons are under `assets/icons/`, translations are under `locales/`, and release/package helpers are in `scripts/`.

## Build, Test, and Development Commands

Run these from this directory:

- `cargo run -p claude_clone` launches the desktop example.
- `cargo check -p claude_clone` type-checks the crate quickly.
- `cargo test -p claude_clone` runs unit and integration tests.
- `cargo clippy -p claude_clone -- --deny warnings` applies the expected lint gate.
- `cargo fmt --all` formats the workspace using Rust 2024 settings.
- `cargo run -p claude_clone --features document-ocr` runs with the optional OCR path enabled.

## Coding Style & Naming Conventions

Use Rust 2024 and the workspace `.rustfmt.toml`. Keep modules focused by feature or UI surface. Follow standard Rust naming: `snake_case` for functions and modules, `PascalCase` for structs/enums, and `SCREAMING_SNAKE_CASE` for constants. Prefer existing GPUI and `gpui-component` patterns before adding new abstractions.

## Testing Guidelines

Place isolated unit tests next to code under `#[cfg(test)] mod tests`. Use `tests/` for behavior that crosses modules; name integration test files by feature, for example `tests/side_panel_data.rs`. For UI changes, manually run `cargo run -p claude_clone` and exercise the affected panels, settings, conversation flows, and provider/MCP paths when relevant.

## Commit & Pull Request Guidelines

Recent history uses Conventional Commits, often with scopes, such as `feat(chat): add token usage statistics` or `chore(release): bump claude_clone`. Keep commits focused on one change.

PRs should describe the problem and solution, link issues when applicable, list exact test commands, and include before/after screenshots for UI work. Call out breaking changes, setup requirements, and any AI-generated code that was reviewed.

## Security & Configuration Tips

Do not commit API keys, provider secrets, local MCP server configuration, or machine-specific paths. Treat MCP child-process configuration as sensitive and document required setup separately.
