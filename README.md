# Ecp Clipboard

Windows clipboard history with a Rust background service, an on-demand GPUI window, and a standalone CLI.

> This refactor is under validation. Current UI startup measurements have not met the 300 ms p95 target. The repository does not automatically publish binaries from tags.

## Programs

| Program | Purpose |
| --- | --- |
| `ecp-clipboard.exe` | Resident tray, clipboard change notifications, hotkeys, history writer |
| `ecp-ui.exe` | GPUI history window; exits when closed |
| `ecp.exe` | Script-friendly CLI |

Build all three with `cargo build --workspace --release --locked`. The background package has no GPUI dependency.

## Usage

Start `ecp-clipboard.exe` and press **Ctrl+Alt+V** or use the tray. The compact window supports search, type filters, copy, delete, and clear. A URL row copies the URL; its separate **Open webpage** button opens the browser. An image row copies the stored original image. `Win+V` takeover is off by default and must be enabled in the window. Its status is shown in the footer; registration failure leaves Ctrl+Alt+V available when that hotkey registered successfully.

```powershell
ecp list 20
ecp search "中文 标点："
ecp paste --id 42
ecp paste 1
ecp clear
```

Search and list show stable `id=...` values. `paste --id` uses that ID. The original `paste N` still means the Nth most recent entry.

## Storage

Settings use the existing EcpClipboard config directory. History metadata is in `clipboard.sqlite3`; full-size lossless PNG images and compressed previews are in the adjacent `images` directory. Back up both together. Before a schema upgrade, the application saves a database snapshot and image files in `backups/pre-v3-...`. Old image entries retain only their existing thumbnail and are labeled accordingly. Migration moves those thumbnails to files in small background batches.

New installations default to a 200-entry history limit. On upgrade, if an old database contains more entries than the old display-only `max_history` value, the limit is raised to preserve them; lowering it later in the UI evicts older entries. The combined original and preview image budget defaults to 500 MB. Search matches punctuation-preserving substrings; whitespace-separated terms must all appear.

For isolated validation, set `ECP_DATA_DIR` and `ECP_CONFIG_DIR` for the launched process only. The repeatable UI benchmark is `cargo run -p ecp-clipboard --release --example bench_ui -- empty 10`; datasets are `empty`, `text200`, `image200`, and `mixed2000`.

## Release status

CI checks formatting, warning-free Clippy, tests, and Windows Release builds for all three programs. Tag builds do not publish artifacts. The locked GPUI commit and the full dependency license and notice obligations must be reviewed before distribution. See [validation notes](./VALIDATION.md).
