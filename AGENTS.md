# Repository Guidelines

## Project Structure & Module Organization

This directory is the `claude_clone` example crate inside the larger `gpui-component` workspace. The crate entry point is `src/main.rs`, which wires GPUI startup, dock layout, panels, settings, and app state together.

Key modules live under `src/`:
- `chat_view.rs`, `conversation_panel.rs`, `sidebar.rs`, and `side_panel.rs` implement the main UI surfaces.
- `models.rs` and `store.rs` define persisted conversations, settings, and storage locations.
- `genai_backend.rs`, `mock_backend.rs`, and `mcp_backend.rs` handle model/provider and MCP integration.
- `document_parser.rs` contains document parsing and optional OCR support behind the `document-ocr` feature.
- `theme.rs`, `titlebar.rs`, `dialogs.rs`, `settings_window.rs`, and `provider_settings.rs` contain supporting UI.

There is no local `tests/` or asset directory at present; shared assets come from `gpui-component-assets`.

## Build, Test, and Development Commands

Run commands from this directory or the workspace root:

- `cargo run -p claude_clone` launches the Claude Clone example.
- `cargo check -p claude_clone` type-checks this crate quickly.
- `cargo test -p claude_clone` runs tests for this crate when tests are added.
- `cargo clippy -p claude_clone -- --deny warnings` matches the workspace CI lint policy.
- `cargo fmt --all` formats the workspace with Rust 2024 style settings.
- `cargo run -p claude_clone --features document-ocr` enables the optional OCR code path.

## Coding Style & Naming Conventions

Use Rust 2024 and the workspace `.rustfmt.toml`. Keep modules focused and named by feature or UI surface. Follow existing Rust naming: `snake_case` for functions/modules, `PascalCase` for structs/enums, and `SCREAMING_SNAKE_CASE` for constants. Prefer existing GPUI and `gpui-component` patterns before introducing new abstractions.

## Testing Guidelines

Add unit tests near the code under `#[cfg(test)] mod tests` for isolated logic, or create `tests/` integration tests if behavior spans modules. For UI changes, manually run `cargo run -p claude_clone` and exercise the affected panels, settings, and conversation flows. Include the exact command and scenario in the PR.

## Commit & Pull Request Guidelines

Recent history uses Conventional Commits with scopes, for example `feat(sidebar): add conversation tree view`. Keep commits focused.

PRs should address one problem, link the issue when applicable, describe changes in English, include before/after screenshots for UI work, call out breaking changes, and list how the change was tested. If AI generated code was used, identify it and ensure it was reviewed and tested.

## Security & Configuration Tips

Do not commit API keys, provider secrets, local MCP server configuration, or machine-specific paths. Treat MCP child-process configuration as sensitive and document any required setup separately.
