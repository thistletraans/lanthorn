# Lanthorn: project and handoff notes

This file is a practical handoff for future work in this checkout, including
bringing local work forward when the official Lanthorn repository changes. It
records durable project facts and the current state of this change; re-check
the live tree and `CLAUDE.md` before acting because both can change.

## Project at a glance

Lanthorn is a cross-platform terminal interactive-fiction interpreter with a
live automapper. Its TUI supports Z-machine (including graphical v6), Glulx,
and Scott Adams stories. It is a Rust Cargo workspace; the main TUI package is
`lanthorn` in `crates/app`.

Current checkout facts when these notes were written:

- Git remote `origin` points to the user's fork,
  `https://github.com/thistletraans/lanthorn.git`. No `upstream` remote is
  configured in this worktree.
- The working branch is `thistletraans-notepad-zen-fixes`. Its current `HEAD`
  is `2b7eec138269efbe8722d46a9630ebebcd58548d`, and the local
  `origin/main` reference points to that same commit. This is the base that
  was available locally during the work; fetch the canonical repository
  separately before claiming it is still current there.
- The feature work is uncommitted. `git status` shows both staged and
  unstaged changes, including pre-existing changes made while preparing this
  feature; inspect the full staged and unstaged diffs before staging or
  committing anything. Do not discard either set.
- The user explicitly said never to create a pull request. Do not create one.
  No commit or push has been requested or made in this session.

## Read these first

1. `CLAUDE.md` is the repository's authoritative working agreement. Follow
   its code, test, fixture, formatting, disk-hygiene, and commit guidance.
2. `docs/README.md` maps the player guide, generated references, internals,
   and historical design notes. `design/`, `plans/`, and `superpowers/` are
   working documents and may be historical rather than current.
3. `docs/internals/architecture.md` explains the crate boundaries and render
   pipeline. `docs/internals/interface.md` covers the TUI. `docs/internals/
   persistence.md` and `customization.md` explain save/config behavior.
4. `docs/internals/releasing.md` is for cutting an official release; it is not
   a generic recipe for syncing local changes with a newer upstream commit.

## Code map

- `crates/app/src/main.rs`: TUI composition, drawing, and startup wiring.
- `crates/app/src/input.rs`: event-to-action routing and action handling.
- `crates/app/src/state.rs`: application and transcript state.
- `crates/app/src/render/`: transcript and screen renderers.
- `crates/app/src/journal.rs`: Journal tab model and shared tab-bar drawing.
- `crates/app/src/journal_notes.rs`: per-story Notes editor, wrapping,
  selection, clipboard operations, persistence hooks, and edit history.
- `crates/app/src/slash.rs`: slash-command registry and dispatch.
- `crates/app/src/config.rs`, `config_template.rs`: global configuration,
  defaults, and generated config reference.
- `crates/app/src/styles.rs`: per-story config sidecar and style persistence.
- `crates/app/src/startup.rs`: TUI boot path; the reusable host boot logic is
  under `crates/app/src/host/`.
- `crates/mapper/`: engine-independent map graph and layout.
- `crates/zvm/`, `crates/gvm/`, `crates/scott/`: story interpreters.
- `crates/blorb/`, `crates/audio/`: media/resource handling.
- `crates/*-cli/`: terminal command-line players; keep their shared plumbing
  in `cli-host` and engine-specific rendering in each CLI.

The interpreter engines report story state; the mapper builds a spatial graph
without depending on an engine. The app hosts all three engines and draws them
through shared screen/UI models. See the architecture document before moving
responsibilities across these boundaries.

## Repository workflow and guardrails

- Use the Rust toolchain pinned in `rust-toolchain.toml` (currently 1.98).
- App unit tests use Cargo features such as `t-input`, `t-render`,
  `t-persist`, and `t-all`. A test build without the right feature silently
  omits those tests.
- Check `CLAUDE.md` for the minimum tests tied to changed files. It cautions
  against unnecessary full workspace gates and broad formatting churn.
- Generated reference pages under `docs/reference/` come from code registries.
  Regenerate them with the documented `LANTHORN_REGEN_DOCS=1` workflow after
  changing a registry; do not hand-maintain generated rows.
- Per-story `config.toml` writes are sparse read-modify-writes: preserve
  unknown keys and treat an absent key as inheritance. Global config defaults
  also have a template/reference; keep these in sync.
- Do not stage blindly (`git add .`), commit, push, publish, or cut a release
  unless explicitly requested. Inspect `git status` first and stage named
  files only. Never create a pull request for this work.
- `CLAUDE.md` notes that buildinfo embeds Git state and commits invalidate
  build fingerprints. Verify before committing when practical, to avoid
  paying for a full relink twice.

Useful commands (run from the repository root):

```powershell
cargo check -p lanthorn --lib --tests --features t-all
cargo test -p lanthorn --lib --features t-all <test-name-filter>
cargo test -p lanthorn --lib --bins --features t-all
cargo build --workspace
```

### Local Windows build and portable folder

This checkout's `.cargo/config.toml` sets Cargo's local output directory to
`target.noindex`. Build the release TUI executable with:

```powershell
cargo build --release --locked -p lanthorn --bin lanthorn
```

The result is `target.noindex/release/lanthorn.exe`. The repository's release
workflow packages each platform binary with `README.md` and `LICENSE`; it also
ships `lanthorn-mapgen.exe` and the three engine CLI players. For a local
Windows-only bundle, build all of Lanthorn's binaries with
`cargo build --release --locked -p lanthorn --bins`, then place the desired
`.exe` files beside `README.md` and `LICENSE` and zip that folder. Build on the
target platform for a matching executable; a Windows `.exe` is not a macOS or
Linux build. The official release workflow is the reference for cross-platform
archives, checksums, and publication. A local ZIP is just a portable bundle,
not an official project release.

For a development run, specify the TUI binary explicitly because the package
also provides `lanthorn-mapgen`:

```powershell
cargo run -p lanthorn --bin lanthorn -- "C:\path\to\story-file.z5"
```

### Preserve and update this checkout

Work in this existing worktree and branch; the user has already tested this
local build successfully. Before any commit, inspect both staged and unstaged
diffs and stage only explicitly intended paths. The user has said not to make
a pull request. A future upstream refresh needs a separately configured
canonical remote (the current `origin` is the user's fork), a fetch, review of
the new commits, and a deliberate merge or rebase; do not infer that the local
`origin/main` ref is current on GitHub.

`cargo-nextest` is not installed in the current Windows environment. If it is
installed later, use the exact `nextest` commands and suite filters in
`CLAUDE.md`; otherwise Cargo's built-in `cargo test` is available. Full test
runs can be much slower with Cargo's runner.

## Current feature work in this checkout

The local change set brings Notes and Zen Mode work onto the current local
upstream base and fixes the usability issues reported during testing.

### Zen Mode

- `/toggle-zen-mode` toggles the persisted `command_separator` setting.
- At the live transcript bottom, earlier transcript rows are dimmed and a
  themed separator is drawn immediately above the latest command. Scrollback
  and pager views are not transformed.
- `zen_mode_separator` is a theme element, defaulting to the shared `line`
  role; theme configuration is described in `docs/reference/style.md` and
  `style.example.toml`.
- Notification toasts drawn over a dimmed transcript do not inherit DIM
  unless their own style explicitly requests it. The old troubleshooting
  “ZEN MODE” footer reminder was removed.
- Config persistence errors for the toggle are shown to the player.

### Journal Notes

- Notes is a Journal tab and stores multiline player-authored text in the
  story's `config.toml` sidecar. It does not write after each keystroke:
  Ctrl+S, leaving Notes, or quitting saves, avoiding the latency of repeatedly
  rewriting the sidecar during typing or paste. Save errors are surfaced in
  the status notification.
- Select the Notes tab, then Shift+Enter or click inside it to edit. Esc saves
  and exits editing. Clicking outside also exits; Alt+Left/Right changes
  Journal tabs and saves Notes as it leaves the editor.
- Shift+Arrow selects text; Ctrl+C copies, Ctrl+X cuts, and terminal
  bracketed paste inserts text. Wrapping reuses
  `render::transcript::wrap_line_ranges`, preserving words when they fit on a
  subsequent row.
- Ctrl+Z and Ctrl+Y undo/redo up to 30 edit actions. Adjacent word-character
  typing and adjacent backspaces group together; a paste or selection
  replacement is one action. Moving the caret separates typing groups.
  History is in-memory for the current editing session and is cleared when
  editing begins again; it is not persisted to disk.

Relevant implementation is in `journal_notes.rs`, `journal.rs`, `state.rs`,
`styles.rs`, `startup.rs`, `input.rs`, `main.rs`, `render/transcript.rs`,
`config.rs`, `config_template.rs`, and `slash.rs`. Player-facing details are
in `docs/guide/playing.md`; generated command/config/style references should
be validated via the `docs_reference` tests when their registries/templates
change.

## Verification status for this feature

Latest checks after Notes undo grouping and the 30-action history change:

- `cargo test -p lanthorn --lib --features t-input journal_notes`: 7 passed.
- Notes shortcut routing and Alt+Left save-on-leave routing tests passed
  individually; the latter verifies the sidecar contains the Notes text after
  the tab changes.
- `cargo check -p lanthorn --bin lanthorn` passed during this work, and the
  full binary check also passed after the Notes editor was added. Cargo emits
  existing unused/dead-code warnings, especially in `term_colors.rs` and
  terminal-watchdog code in `main.rs`.
- `docs_reference` passed all 5 checks after regenerating the command
  reference.
- `git diff --check` and `rustfmt --check` for the Notes editor passed.
- The user manually launched the local Anchorhead build and reported that it
  worked. This is a user smoke test, not an exhaustive test of every story or
  terminal.
- Two `cargo test --workspace --all-features` runs were attempted with
  `LANTHORN_FIXTURES_REQUIRED=1`. Both stop at the same failing test in
  `lanthorn-gvm-cli`: `glulxercise_all_groups_pass`, whose bundled Glulx math
  test reports `pow(2, -1075)` as the smallest positive subnormal instead of
  zero. This test and its math implementation are outside the Notes/Zen
  changes; the workspace gate is not green.
- Strict `cargo clippy -p lanthorn --all-targets --all-features -- -D
  warnings` fails on pre-existing unused/dead-code warnings in
  `term_colors.rs` and an unnecessary cast in transcript code. The cast was
  restored to its original form because changing it was unrelated; rerunning
  with those existing warning classes allowed completed successfully. This
  is not a clean strict Clippy pass.
- `cargo-nextest` is not installed in this Windows environment; use Cargo's
  built-in `cargo test` for focused validation unless nextest is installed.

All feature changes remain uncommitted on `thistletraans-notepad-zen-fixes`.
The user has asked not to create a PR. Do not claim the workspace test gate or
strict Clippy is clean.

## When the official repository changes

The current `origin` remote points at the user's fork, not the canonical
repository. Before integrating a later official update:

1. Check `git status`, save this uncommitted work on a dedicated branch or
   another recoverable snapshot, and leave `.vs/` and other unrelated
   untracked files untouched.
2. Fetch `origin`; inspect the new commits, release notes/changelog, tags,
   and changed files before choosing a merge or rebase. Do not blindly pull
   a release over local work.
3. Resolve conflicts by following the changed subsystem's current design and
   tests, not by restoring old line numbers or stale notes. Re-check
   `CLAUDE.md`, `Cargo.toml`, feature gates, and generated reference docs
   because repository workflow can change too.
4. Run focused tests for touched code, then the appropriate merged-tree check.
   If tests fail on absent story fixtures, distinguish those skips/failures
   from real regressions and restore the fixtures before claiming a full pass.
5. For an actual release operation, use `docs/internals/releasing.md`; do not
   infer release steps from a normal upstream sync.

Keep this file current when the branch, test status, remote arrangement, or
feature state materially changes. The code and `CLAUDE.md` remain authoritative.
