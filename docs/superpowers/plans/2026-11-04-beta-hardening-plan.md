# Beta Hardening Implementation Plan

> **For agentic workers:** Execute task-by-task in the current isolated GitHub branch. Keep all integration changes out of main. Use TDD and record results in BETA_REVIEW_LOG.md.

**Goal:** Harden Hwpcolab's font, HWPX, save, DOCX and Vite font-serving paths while adding repeatable CI.

**Architecture:** Keep Tauri 2 + Vite + RHWP. Add focused native/TypeScript helpers and regression fixtures. Use GitHub Actions for Linux frontend validation and Windows Rust validation.

**Tech Stack:** Rust/Tauri 2, fontdb, zip, TypeScript, Vite 6.4.5, Vitest, happy-dom, fflate.

**Spec:** `docs/superpowers/specs/2026-11-04-beta-hardening-design.md`

## Global Constraints

- Baseline: `e46f7230074cab63362f41d46cc1d1153085f745`.
- Branch: `codex/beta-hardening-2026-11-04`; integration: `Beta` only.
- Keep main untouched.
- Behavior fixes require a RED -> GREEN regression test.
- No blanket dependency changes or undocumented upstream ports.
- Preserve command names `list_local_fonts` and `read_local_font`.

## Review Focus

- Browser runtime must never invoke Tauri-only commands.
- Font path checks must reject traversal, sibling prefixes, symlink escapes and non-font files.
- HWPX must check first/stored `mimetype`, exact media type and essential OWPML parts.
- DOCX must support alternate namespace prefixes, XML entities, preserved whitespace, tabs, breaks and tables.
- Failed save operations must leave existing target bytes unchanged.

---

### Task 1: Test harness and native runtime regression

**Files:** `package.json`, `package-lock.json`, `.github/workflows/beta-validation.yml`, `src/hop-host/core/desktop-local-fonts-runtime.test.ts`.

- [ ] Add Vitest and happy-dom test dependencies; align Vite to 6.4.5.
- [ ] Add `npm test` and CI jobs for frontend and Windows Rust tests. Initially use setup installs so the generated lockfiles can be captured as artifacts.
- [ ] Add tests for `__TAURI_INTERNALS__`, `tauri:` protocol, ordinary browser, and absent window.
- [ ] Run PR CI and verify the two Tauri-positive cases fail because the current implementation returns false.
- [ ] Record exact RED run and artifact IDs.

### Task 2: Restore native font catalog

**Files:** `desktop-local-fonts.ts`, `src-tauri/src/font_catalog.rs`, `src-tauri/src/commands/fonts.rs`, `commands/mod.rs`, `lib.rs`, Cargo manifest/lock.

- [ ] Implement runtime detection minimally and verify focused RED -> GREEN.
- [ ] Add the fontdb catalog and OS-specific extra user directories.
- [ ] Register `list_local_fonts` and `read_local_font`.
- [ ] Add canonical-path, extension, sibling-prefix and symlink escape tests.
- [ ] Run all frontend tests and Windows `cargo test`.

### Task 3: HWPX validation and safe saves

**Files:** `document.rs`, `vite.config.ts`, new shared/test helpers, Cargo manifest/lock.

- [ ] Inventory repository HWPX fixtures and actual package members.
- [ ] Add failing generic-ZIP/malformed-package and valid-package tests.
- [ ] Add failing save tests for missing staging input, successful overwrite and preservation of old target on failure.
- [ ] Implement OWPML structural validation and same-directory durable replacement for both native and development bridge.
- [ ] Run fixture and Windows save tests.

### Task 4: DOCX XML parser

**Files:** `src/core/multiFileParser.ts`, new DOCX helper/test files, `package.json`/lock if parser dependency is required.

- [ ] Add fixture ZIPs with alternate prefixes, entities, xml:space, tabs, line breaks and tables.
- [ ] Verify current parser fails at least one intended assertion.
- [ ] Implement namespace-aware extraction and stable Markdown representation.
- [ ] Run DOCX tests and full frontend suite.

### Task 5: Vite font path security

**Files:** `vite.config.ts`, new path helper and tests.

- [ ] Add tests for valid retrieval, encoded traversal, sibling-prefix, symlink escape, malformed encoding and unsupported extension.
- [ ] Implement canonical path containment and not-found handling.
- [ ] Run the focused middleware tests and production build.

### Task 6: Locks, upstream review and finish

- [ ] Retrieve CI-generated npm and Cargo locks, commit them, then switch CI to `npm ci` and `cargo test --locked`.
- [ ] Compare current HOP/RHWP upstream fixes and port only relevant changes with regression tests.
- [ ] Review dependency versions and security advisories; avoid unrelated upgrades.
- [ ] Run complete test/build/Windows validation and inspect the whole PR diff.
- [ ] Update BETA_REVIEW_LOG.md with exact run URLs, commit IDs, results and residual risks.
- [ ] Merge the clean PR into Beta only.
