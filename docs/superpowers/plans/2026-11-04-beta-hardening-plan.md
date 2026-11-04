# Beta Hardening Implementation Plan

> Living plan for the Beta-only hardening workstream. Implementation and CI run directly through GitHub; COS is not part of this continuation.

**Goal:** Improve the whole Hwpcolab Beta workstream's font handling, document-format validation, safe saves, imports, local development bridge, and repeatable verification.

**Baseline:** `e46f7230074cab63362f41d46cc1d1153085f745`  
**Implementation branch:** `codex/beta-hardening-2026-11-04`  
**Integration target:** `origin/Beta` only. Never modify `main`.  
**PR:** #1

## Guardrails

- Keep the existing Tauri 2 + Vite + RHWP architecture.
- Preserve frontend command names and payload shapes.
- Behavior changes require a regression test that fails for the intended reason before implementation.
- Do not blindly upgrade HOP/RHWP or replace vendored code.
- Commit both JavaScript and Rust lockfiles; verify with locked installs.
- Do not claim a platform-specific runtime check from unit-test evidence alone.

## Completed work

### Task 1: Test harness and runtime detection
- [x] Add Vitest and happy-dom.
- [x] Add deterministic frontend test command and CI.
- [x] Cover Tauri internal marker, tauri: protocol, browser, and absent-window cases.
- [x] Restore desktop runtime detection without invoking Tauri commands in a normal browser.

### Task 2: Native local-font catalog
- [x] Port the relevant HOP v0.4.5 fontdb catalog behavior into Hwpcolab's Tauri structure.
- [x] Add OS-specific extra user-font directories.
- [x] Register list_local_fonts and read_local_font.
- [x] Canonicalize requested paths and enforce approved-root/extension checks.
- [x] Add runtime and native path regression tests.

### Task 3: HWPX package validation and safe saves
- [x] Validate ZIP structure, first/stored mimetype, exact OWPML media type and required document parts.
- [x] Reject generic ZIPs, malformed packages and encrypted OWPML parts.
- [x] Recognize the binary ZIP local-file-header signature in native document routing.
- [x] Allow valid data descriptors on ordinary ZIP entries while keeping mimetype strict.
- [x] Add generated and repository HWPX fixtures.
- [x] Replace direct destination overwrites with same-directory temporary-file replacement, sync and cleanup in Rust and Vite paths.
- [x] Test successful overwrite and preservation of existing targets on failure.

### Task 4: DOCX import
- [x] Replace regex-only XML extraction with namespace-aware DOM parsing.
- [x] Preserve paragraph boundaries, text/entities, xml:space, tabs, breaks and table structure.
- [x] Add generated regression fixtures and repository DOCX/XLSX sample coverage.
- [x] Bound attachment and ZIP expansion sizes.

### Task 5: Vite font serving and path security
- [x] Add canonical path containment, extension checks, malformed-encoding handling and symlink escape tests.
- [x] Ensure rejected font paths terminate with 404 rather than falling through to general Vite static middleware.
- [x] Verify valid WOFF2 retrieval and production build.

### Task 6: LLM request lifecycle and development-bridge parity
- [x] Add deterministic test for native stale-cancellation cleanup.
- [x] Scope native cleanup to a unique AtomicU64 registration token.
- [x] Reject duplicate active request IDs.
- [x] Add identity-safe ActiveHttpRequests for the Vite bridge.
- [x] Align Vite bridge URL validation, timeout clamp, request/response byte limits, redirect policy and response-body cancellation with native behavior.
- [x] Keep timeout active until response body reading completes.
- [x] Add regression tests for duplicate IDs, ID reuse, URL schemes/userinfo, UTF-8 byte limits, timeout bounds, streaming response limits and BOM handling.

### Task 7: Lockfiles, dependency audit and CI
- [x] Commit package-lock.json and src-tauri/Cargo.lock.
- [x] Add a scoped lockfile-sync job for the same-repository Beta workstream.
- [x] Use npm ci and cargo test --locked for the same-repository PR and Beta push paths.
- [x] Add npm audit --audit-level=high; the latest completed frontend run passed.
- [x] Update actions/checkout to v7.0.1 and actions/setup-node to v7.1.0 to avoid the Node 20 runtime deprecation warning.
- [x] Run frontend tests/build on Ubuntu and Rust tests on Windows.

## Current verification evidence

- Frontend: 209 tests pass; production build passes.
- Windows Rust: 25 tests pass.
- Locked frontend install: npm ci passes.
- Dependency audit: npm audit --audit-level=high passes.
- The most recent completed integrated checkpoint is workflow run 41743372019: lockfiles were already up to date, npm ci/audit/tests/build passed, and Windows cargo test --locked was still running at log time.
- Workflow run 41743558019 validates the updated Node 24 GitHub Actions releases.
- The latest BETA_REVIEW_LOG.md update and this plan update trigger subsequent runs; their current-head results must be checked before integration.

## Remaining work

### Task 8: Upstream comparison and final integration
- [x] Compare HOP and RHWP release versions against the reviewed upstream state.
- [x] Port only the relevant HOP native font catalog behavior.
- [x] Confirm RHWP v0.8.7 is already current; no blind engine replacement.
- [ ] Obtain the final current-head Windows cargo test --locked result and latest Actions run.
- [ ] Re-read the complete PR diff, including command registration, font allow-list, save parity, HWPX parity, DOCX import and LLM lifecycle.
- [ ] Record remaining platform-specific limitations. Packaged Tauri WebView smoke testing remains distinct from Rust unit tests.
- [ ] Confirm Beta has not moved behind the work branch.
- [ ] Integrate the reviewed result into Beta only; do not merge to main.

## References

- Design: `docs/superpowers/specs/2026-11-04-beta-hardening-design.md`
- Evidence and running history: `BETA_REVIEW_LOG.md`
