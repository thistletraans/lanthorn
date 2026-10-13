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

- Git remote `origin` points to `https://github.com/sharkusk/lanthorn.git`.
- The working branch is `codex/live-command-separator-journal-notes`, currently
  based at `5138c6b3` (`fix(ifdb): ranged requests ask for identity encoding;
  gzip replies no longer fake a size`). At inspection time this base is also
  `origin/main`; the feature changes are working-tree edits, not commits yet.
- Only `origin` is configured; there is no separate `upstream` remote.
- `origin` points directly at the canonical project, not the user's fork. Do
  not push feature work there; create a personal GitHub fork and push the
  feature branch to it if remote backup or sharing is wanted.
- Feature changes are uncommitted. Preserve them before switching branches,
  rebasing, or merging. `.vs/` is untracked local editor content; leave it
  alone and do not stage it. `docs/PROJECT-UNDERSTANDING.md` is also currently
  untracked and needs an explicit add if the user wants it committed.

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
  files only.
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

### Preserve and update this checkout

Keep this clone in its current folder and use the existing feature branch; a
second clone is unnecessary. First inspect `git status` and review the diff,
then stage only the files intended for the feature and commit them on
`codex/live-command-separator-journal-notes`. Do not use `git add .`: it would
also pick up unrelated local files such as `.vs/`. For a durable off-machine
backup, create a personal GitHub fork, add it as a second remote (for example
`fork`), and push the feature branch there. Keep `origin` pointed at the
creator's repository for fetching official updates. Once the feature is
committed, update later by fetching `origin`, reviewing the new commits, and
rebasing or merging the feature branch onto the chosen official base; then
resolve conflicts and verify the combined tree. Avoid doing this while the
feature edits are still only uncommitted working-tree changes.

`cargo-nextest` is not installed in the current Windows environment. If it is
installed later, use the exact `nextest` commands and suite filters in
`CLAUDE.md`; otherwise Cargo's built-in `cargo test` is available. Full test
runs can be much slower with Cargo's runner.

## Current feature work in this checkout

The requested implementation adds:

1. Optional Zen Mode, toggled by `/toggle-zen-mode`, dims earlier transcript
   rows and draws a rule before the latest visible command. The rule is the
   `zen_mode_separator` element, whose default parent is the `line` role and
   whose parent can be changed independently in `[elements]` in `style.toml`.
   The persisted `command_separator` setting defaults off. The rule is
   suppressed while paginating or scrolled away from the live bottom.
2. A framed Notes tab in the Journal, matching the other tab panels. Notes are
   multiline, per-story text stored in the story's `config.toml` sidecar; edits
   persist as the player types. Shift+Enter or a click in Notes starts editing;
   Esc or a click outside saves and returns game focus. There is no separate
   Notes command-palette entry.
3. Alt+Left/Right traverses Journal tabs. Shift+Arrow remains available to map,
   Documents, Hints, and Notes navigation instead of being repurposed for tabs.
4. Notes supports multiline editing, selection, copy/paste, and soft wrapping;
   its per-story content is saved in the story's config sidecar.
5. Command/config docs, player instructions, interface notes, and this handoff.

Main implementation files are `journal.rs`, new `journal_notes.rs`,
`state.rs`, `styles.rs`, `startup.rs`, `input.rs`, `main.rs`,
`render/transcript.rs`, `config.rs`, `config_template.rs`, and `slash.rs`.
The Journal model and slash-command registry have tests that pin their tab and
command counts, so add new rows/variants to those tests. Generated reference
docs should be checked against `docs_reference` after registry changes.

## Verification status for this feature

Verification status at the latest handoff update:

- The user confirmed Zen Mode's visible dimming and separator in a real
  Mini-Zork session. The dedicated `zen_mode_separator` theme selector was
  added afterward; it defaults to the shared `line` role and has passed a
  compile check, but its custom-parent behavior has not yet been visually
  checked.
- `cargo check -p lanthorn --bin lanthorn --features t-all` passed after the
  latest source edits. It emitted existing unused import/dead-code warnings
  in `term_colors.rs` and watchdog code in `main.rs`.
- These feature changes remain uncommitted on
  `codex/live-command-separator-journal-notes`; do not treat the current
  executable or a copied ZIP as a durable source backup.

Earlier verification in this environment:

- `cargo check -p lanthorn --lib --tests --features t-all` succeeded after
  fixing two compile issues (Notes renderer visibility and the updated tab
  assertion in `journal_hints_tab`). It emitted unrelated warnings in
  `term_colors.rs`, `startup.rs`, and `main.rs`.
- Earlier focused tests passed for Notes editor selection, per-story Notes
  persistence, command-separator config persistence, and slash registry
  completeness. The revised Notes click/Shift+Enter and Alt+Arrow tab routing
  have regression tests in the source but have not yet been run.
- `git diff --check` passed; `rustfmt --check --edition 2021` passed for the
  new Notes editor file.
- The project's `scripts/fixtures.manifest` was fetched for local tests:
  46 ignored fixture files were downloaded and verified by byte count and
  SHA-256 into `crates/app/tests/fixtures/stories/`. They are deliberately not
  tracked or redistributed. The documented Bash script was unavailable in
  this sandbox, so an equivalent one-off fetch read the manifest, verified
  every result, and left no archive cache behind.
- After fixture fetch, `cargo test -p lanthorn --lib --bins --features t-all`
  reported 4,027 library tests passed, 1 failed, and 16 ignored. The remaining
  failure is `documents_tab::tests::the_folder_header_is_an_osc8_link`: on
  this Windows environment the buffer contains the clipped `Folder:` path
  text but not the OSC8 marker that the test expects. It reproduces when run
  alone and is outside this feature's changed files. The binary test target
  passed separately: 210 passed, none failed.
- A focused Notes editor test initially wrote to a relative `config.toml`
  because its default `AppState` had an empty game directory. The test now
  uses a dedicated temporary story directory, checks the persisted text, and
  removes it. The updated focused test passed. The full library suite was run
  before this test-only cleanup edit; rerun it if another full local pass is
  needed.
- Earlier Cargo dependency fetching failed with Windows Schannel
  `SEC_E_NO_CREDENTIALS`; it later succeeded, so that was a transient
  environment/download problem rather than a source compile error.

## When the official repository changes

The current remote is already named `origin` and points at the project's
canonical GitHub repository. Before integrating an official update:

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
