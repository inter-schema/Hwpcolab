# COS Reconnection Test Plan

> Prepared: 2026-11-04 (Asia/Seoul)  
> Repository: `inter-schema/Hwpcolab`  
> Target: `codex/beta-hardening-2026-11-04` → PR #1 → `Beta` only  
> Guardrail: do not merge to or modify `main`. Do not expose API keys in files, terminal output, screenshots, commits, or logs.

## 0. Purpose and execution rules

This is the handoff plan for the next session in Chat On Steroids (COS). Once the user supplies the API details, first establish what the API permits, then execute the tasks below against the exact checked-out PR head. Do not assume an API schema, endpoint, model, or permission before inspecting the supplied contract.

- Work only in the repository and branch stated above unless the user explicitly changes the target.
- Before any write: record repository, branch, HEAD SHA, working-tree state and PR base/head SHAs.
- Keep a clean baseline and make a separate commit for each coherent fix.
- Never store credentials in source files, test fixtures, shell history, screenshots, console logs, Git, or this plan. Prefer process/session environment variables or the approved secret store; redact values in all output.
- Do not run destructive commands, broad cleanup, force-push, merge, or change branches without checking the exact target first.
- For every defect: reproduce → isolate root cause → add regression test → verify the test fails for the intended reason → implement the smallest safe fix → run targeted tests → run the full required gates.
- Report evidence, not assumptions. A passing unit suite is not proof that the packaged Windows WebView works.
- When a task is blocked by missing API permissions, missing device/runtime, or missing test data, record the blocker and continue with independent tasks.

## 1. Connection and API contract checks

Run these before touching application code.

| ID | Check | Method | Pass condition |
|---|---|---|---|
| CON-01 | Identity and session | Read the COS connection/session identity and authenticated principal, without printing secrets | Correct user/workspace is active |
| CON-02 | API contract | Inspect the user-supplied API base URL, docs/schema, auth method, version and rate limits | Exact endpoints and auth semantics are documented |
| CON-03 | Read-only probe | Call the least-privileged health/version or metadata endpoint | Expected status/schema; no mutation |
| CON-04 | Scope and permissions | Inspect available repository, filesystem, command, browser and screenshot scopes | Actual capabilities are known; no inferred permissions |
| CON-05 | Secret handling | Supply credential through approved secure input/environment path; test redaction | Credential never appears in logs, files or screenshots |
| CON-06 | Workspace mapping | Resolve the repo path and compare its remote, branch and HEAD to PR #1 | Workspace maps to the intended checkout |
| CON-07 | Recovery | Verify that failed API calls and interrupted commands can be identified/retried safely | No duplicate writes or ambiguous partial completion |
| CON-08 | Context handoff | Read this plan and `BETA_REVIEW_LOG.md`; create/update a dated execution log | Work resumes from evidence, not memory |

**Connection acceptance gate:** do not start write operations until CON-01 through CON-06 pass. If the provided API is an application/model API rather than a COS control API, test it in an isolated configuration and keep its key out of the repository.

## 2. Current baseline to re-check at session start

Last known state when this plan was prepared:

- PR: [#1](https://github.com/inter-schema/Hwpcolab/pull/1), open, base `Beta`.
- Branch: `codex/beta-hardening-2026-11-04`.
- Last verified head: `631aa79ab9b0ff11fd62f56c6fecff488907ec08`.
- Base: `e46f7230074cab63362f41d46cc1d1153085f745`.
- Last current-head Actions run: `41748782849` (run #141), completed with failure.
- Frontend: npm install/lockfile sync, audit, 212 tests and production build passed on that run.
- Windows Rust: rustfmt and Clippy passed; 26/27 tests passed.
- Known blocker: `hwpx_package::tests::rejects_duplicate_package_part_names`. The intended duplicate-entry archive fixture did not reliably exercise a real duplicate ZIP member. The validator's duplicate rejection is not considered proven until a genuine duplicate archive is fed to it.
- No packaged Windows Tauri WebView smoke test has been recorded.
- `main` and `Beta` have not been changed by the isolated PR workstream.

**Mandatory refresh:** query PR #1 and its latest workflow run before relying on any of these values. If the head moved, update this section in the execution log and run tests against the actual current head.

## 3. Prioritized concrete engineering tasks

### P0 — Unblock and prove HWPX duplicate-entry rejection

**Task HWPX-01: inspect the current validator and fixture builder**
- Read `src-tauri/src/hwpx_package.rs`, `src/hop-host/core/hwpx-package.ts`, and their tests.
- Trace how the Rust test archive is assembled, how the central directory is enumerated, and how duplicate names are normalized.
- Determine why the existing fixture contains only one effective `Contents/section0.xml` entry.

**Task HWPX-02: construct a genuine duplicate ZIP fixture**
- Prefer a minimal low-level fixture builder that writes local file headers, central-directory records and EOCD deterministically, or another method proven to retain duplicate names.
- Include first/stored `mimetype` and all required OWPML package parts.
- Before validator invocation, independently enumerate central-directory records and assert that two entries with the same normalized name are present.
- Test exact duplicate names and case-only duplicates if the package policy is case-insensitive.
- Do not weaken production validation to make a defective fixture pass.

**Acceptance**
- The fixture self-check proves that two duplicate members exist.
- The new test fails against the vulnerable behavior and passes after the fix.
- Rust `cargo test --locked` passes on Windows CI.
- TypeScript validator has a matching duplicate-entry regression test if its ZIP construction path can represent duplicates.
- Record the actual CI run URL and test counts.

### P1 — HWPX package and document-open matrix

| ID | Scenario | Expected behavior |
|---|---|---|
| HWPX-03 | Real repository HWPX fixtures | Accept valid packages |
| HWPX-04 | Generic ZIP renamed to .hwpx | Reject |
| HWPX-05 | Wrong/missing/deflated/non-first mimetype | Reject |
| HWPX-06 | Missing required OWPML parts | Reject |
| HWPX-07 | Truncated ZIP, bad central directory, invalid offsets | Reject cleanly; no panic |
| HWPX-08 | Encrypted entry | Reject |
| HWPX-09 | Duplicate member name, including case-fold collision | Reject |
| HWPX-10 | Legal data descriptor on ordinary member | Accept when otherwise valid |
| HWPX-11 | Open-document routing with bytes 50 4B 03 04 | Correctly route to package validation |
| HWPX-12 | Invalid package through native and Vite bridge | Same accept/reject decision |

Also test user-visible failure messaging and ensure a failed open does not replace the currently open document or leak a partial session.

### P1 — Font catalog and actual font rendering

| ID | Scenario | Expected behavior |
|---|---|---|
| FONT-01 | Tauri v2 runtime marker and custom protocol detection | Desktop runtime detected |
| FONT-02 | Enumerate installed Windows fonts | Stable metadata; no crash on unusual font files |
| FONT-03 | Read an allowed font file | Bytes returned only for an approved canonical path |
| FONT-04 | Arbitrary outside path / sibling-prefix path | Rejected |
| FONT-05 | Symlink or reparse-point escape | Rejected after canonical resolution |
| FONT-06 | Unsupported extension / malformed path | Rejected |
| FONT-07 | TTC/OTC with multiple faces | Each face has correct PostScript name and face index |
| FONT-08 | Renderer loads selected collection face | Displayed face matches selected metadata |
| FONT-09 | Missing/unreadable font | Graceful error and UI remains usable |

**Manual COS task:** launch the built desktop app, open a document that uses a known installed font, change to a font from a collection file if available, save, close and reopen. Capture screenshots of the font picker and rendered document, with private paths/usernames redacted.

### P1 — Atomic save integrity and recovery

| ID | Scenario | Expected behavior |
|---|---|---|
| SAVE-01 | Replace existing document | New bytes are complete |
| SAVE-02 | Missing staging file | Existing target remains unchanged |
| SAVE-03 | Replacement/rename failure | Existing target survives; temp file cleaned |
| SAVE-04 | Unicode path and non-ASCII document content | Exact bytes preserved |
| SAVE-05 | Large document | No partial/truncated target |
| SAVE-06 | Two concurrent save attempts | No cross-contamination or silent data loss |
| SAVE-07 | Simulated process interruption before replacement | Previous target remains valid |
| SAVE-08 | Native Rust and Vite dev bridge | Equivalent outcome and error behavior |

Check both the native staging path and Vite development bridge. Do not simulate destructive power loss on user documents; use disposable fixtures only.

### P1 — DOCX extraction and attachment limits

- DOCX: alternate namespace prefixes, paragraphs, runs, tables, tabs, explicit breaks, entities, `xml:space="preserve"`, soft/non-breaking hyphens, malformed XML.
- Ensure indentation/newlines between XML nodes do not become spurious user text.
- Verify only `word/document.xml` is extracted when intended; oversized unrelated embedded media must not be inflated.
- Test per-entry and total ZIP expansion thresholds at limit−1, exact limit and limit+1.
- Test raw attachment size at 128 MiB boundary using sparse/disposable fixtures or mocked metadata; do not allocate needless large buffers.
- Verify the browser picker/drop, parser entry point and native read boundary each enforce the same raw-size policy.

### P1 — XLSX import compatibility and archive safety

- Open repository XLSX samples and representative workbooks with multiple sheets, formulas, dates, Unicode sheet names and empty cells.
- Verify expected Markdown and structured sheet output from SheetJS 0.20.3.
- Test oversized entry, aggregate ZIP expansion, corrupt central directory and unsupported input.
- Confirm ordinary legacy XLS/OLE files are handled separately: ZIP expansion preflight does not apply to OLE, while the raw file limit still applies.

### P1 — Vite font asset route containment

| ID | Scenario | Expected behavior |
|---|---|---|
| PATH-01 | Valid in-root WOFF2 | 200 and correct bytes |
| PATH-02 | Parent traversal and encoded traversal | 404 |
| PATH-03 | Sibling directory sharing root prefix | 404 |
| PATH-04 | Malformed percent encoding | Controlled 404, no crash |
| PATH-05 | Symlink to outside root | 404 |
| PATH-06 | Missing file / directory / wrong extension | 404 |
| PATH-07 | Rejected request middleware chain | Does not call `next()` or fall through to static serving |

Use an isolated temporary directory and disposable symlinks. Never create test symlinks against system font folders.

### P1 — LLM proxy native/Vite parity

| ID | Scenario | Expected behavior |
|---|---|---|
| LLM-01 | Request without request ID | Reaches local mock server |
| LLM-02 | Duplicate active request ID | Second request rejected; first remains controllable |
| LLM-03 | Reuse ID after cancellation | New registration remains intact after old cleanup |
| LLM-04 | Cancel while receiving response body | Read aborts and registry is cleaned safely |
| LLM-05 | HTTP/HTTPS endpoint | Accepted |
| LLM-06 | Other scheme or URL userinfo | Rejected |
| LLM-07 | Cross-origin redirect | Not automatically followed |
| LLM-08 | Request body > 20 MiB | Rejected before send |
| LLM-09 | Response body > 32 MiB | Bounded; stream cancelled |
| LLM-10 | Oversized Content-Length | Rejected before body read |
| LLM-11 | Timeout lower/upper/outside bounds | Clamped to 1 second–10 minutes |
| LLM-12 | UTF-8 BOM response | BOM stripped |
| LLM-13 | Native Rust and Vite bridge | Same safety semantics |

Use a local mock HTTP server only. Do not send test prompts to paid provider APIs unless the user explicitly supplies a test endpoint and budget/usage constraints.

### P2 — Tauri CSP and packaged Windows WebView smoke test

- Confirm production CSP is active and does not contain unrestricted `unsafe-eval`.
- Confirm Tauri IPC/custom protocol, local workers, fonts, images and WASM paths still function.
- Build/package the Windows app using the documented project command.
- Launch the packaged app, not only Vite dev mode.
- Test startup, document open, font enumeration, editor rendering, save, close/reopen, LLM proxy configuration UI, cancellation and error display.
- Capture screenshots and console/runtime errors. Redact account names, local absolute paths and tokens.
- Record Windows version, app build ID, WebView runtime version if available, and exact artifact hash.

A unit-test green result is not a substitute for this packaged-app check.

## 4. Full validation gates

Run in this order, recording exact command, exit code and output artifact:

1. Frontend dependencies: `npm ci`
2. Security audit: `npm audit --audit-level=high`
3. Focused TypeScript tests for the changed subsystem.
4. Full frontend tests: `npm test`
5. Production build: `npm run build`
6. Rust formatting check using the workflow's exact changed-file scope.
7. Clippy: `cargo clippy --locked --manifest-path src-tauri/Cargo.toml --lib -- -D warnings`
8. Windows Rust tests: `cargo test --locked --manifest-path src-tauri/Cargo.toml -- --test-threads=1 --nocapture`
9. Packaged Windows Tauri WebView smoke test.
10. Refresh PR checks and inspect the complete final diff before any integration.

Do not silently change the CI command or narrow test scope just to obtain a green check. Explain any changed scope and preserve a separate record of tests not run.

## 5. Proposed COS execution backlog

Execute in this order; parallelize only independent read-only review after the baseline is captured.

1. **COS-BOOT-01 — Verify connection and supplied API contract.** Deliver a capability/permission table and safe credential path.
2. **COS-BOOT-02 — Reconcile workspace with PR #1.** Deliver repo, branch, HEAD, clean/dirty state, PR head and latest Actions run.
3. **COS-TEST-01 — Diagnose HWPX duplicate fixture.** Deliver root cause and a minimal archive reproducer.
4. **COS-FIX-01 — Repair duplicate archive fixture and regression.** Deliver test-level RED/GREEN evidence and the smallest code change needed.
5. **COS-TEST-02 — Run the HWPX package matrix.** Deliver native/TypeScript parity table.
6. **COS-TEST-03 — Run font catalog and collection-face smoke tests.** Deliver font metadata and screenshot evidence.
7. **COS-TEST-04 — Run atomic-save failure/recovery matrix.** Deliver before/after hashes for disposable files.
8. **COS-TEST-05 — Run DOCX/XLSX parser and archive-boundary matrix.** Deliver expected-vs-actual extraction results.
9. **COS-TEST-06 — Run Vite path traversal and middleware fall-through matrix.** Deliver HTTP status and middleware evidence.
10. **COS-TEST-07 — Run LLM proxy parity matrix against local mock server.** Deliver request/cancellation traces with secrets redacted.
11. **COS-TEST-08 — Build and smoke-test packaged Windows app.** Deliver build hash, environment facts, screenshots and defects.
12. **COS-REVIEW-01 — Full diff and release gate review.** Deliver green/red checklist, residual risks and a Beta-only integration recommendation. Do not merge unless every mandatory gate is green and user authorization still covers integration.

## 6. Execution log template

Append one entry per COS session. Do not overwrite previous evidence.

### YYYY-MM-DD — COS session N

- COS session / workspace:
- API contract version and permitted scopes (no secrets):
- Repository / branch / HEAD before:
- PR #1 head and Beta base:
- Worktree state:
- Tasks attempted:
- Tests run and exact commands:
- Results / CI run URLs:
- Files changed and commit SHAs:
- Screenshots / artifacts:
- Blockers / not-run tests:
- Repository / branch / HEAD after:
- Next task:

## 7. Definition of done

- [ ] API connection validated without leaking credentials.
- [ ] Current branch/PR identity verified; Beta-only boundary preserved.
- [ ] Duplicate HWPX test uses a demonstrably genuine duplicate ZIP entry.
- [ ] All relevant frontend tests and production build pass.
- [ ] npm audit high-severity gate passes.
- [ ] Rust formatting, Clippy and locked Windows tests pass.
- [ ] Native and Vite implementations have matching security behavior where both exist.
- [ ] Packaged Windows WebView smoke test is completed or explicitly marked as a release blocker.
- [ ] Complete PR diff reviewed; no unrelated/generated secrets or accidental user data.
- [ ] Final evidence and residual risks recorded.
- [ ] Integration target is `Beta` only; `main` remains untouched.
