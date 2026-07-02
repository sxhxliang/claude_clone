# Release Packaging

This repository is packaged as a standalone GPUI application.

## Update Source

The app uses `gpui-updater` with `GitHubSource` and checks:

- repository: `sxhxliang/claude_clone`
- release tag: semantic version tag such as `v0.5.1`
- checksums asset: `SHA256SUMS`
- platform asset name: must contain the current CPU architecture

`GitHubSource` selects assets by extension first (`.dmg`, `.exe`, `.tar.gz`) and
this app also requires the asset name to contain `std::env::consts::ARCH`.

## Release Assets

The GitHub Actions release workflow publishes:

- `claude_clone-v<version>-macos-<arch>.dmg`
- `claude_clone-v<version>-windows-<arch>.exe`
- `claude_clone-v<version>-linux-<arch>.tar.gz`
- `SHA256SUMS`

The workflow validates that the pushed tag matches `Cargo.toml` version. For
example, `version = "0.5.1"` must be released with tag `v0.5.1`.

## Icons

The source icon is `assets/icons/claude_clone.svg`. Generated raster assets live
under `assets/icons/` and are regenerated with:

```bash
python -m pip install pillow
python scripts/generate_icons.py
```

Windows embeds `assets/icons/claude_clone.ico` at compile time through
`build.rs`. macOS packaging generates `claude_clone.icns` from the checked-in PNG
sizes and places it in the `.app` bundle.
