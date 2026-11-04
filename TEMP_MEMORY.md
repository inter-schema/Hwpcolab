# Hwpcolab Beta Hardening — Temporary Working Memory

> Durable context for the ongoing QA loop. Read before each resumed session and update after meaningful work. This file is temporary working memory, not the source of truth for code or CI.

Updated: 2026-11-04 18:21 Asia/Seoul

## Repository boundaries

- Repository: inter-schema/Hwpcolab
- PR #1: https://github.com/inter-schema/Hwpcolab/pull/1
- Integration base: Beta only. Do not modify main. Do not merge without a separate explicit instruction.
- PR head before this memory commit: 2407072f96b2a9415be638a96bce055a7d7b0792
- Beta: e46f7230074cab63362f41d46cc1d1153085f745
- main: 0c23baaf63a49e75fd176a8b18d182acfa91546e
- Local checkout: C:\Users\jun\Documents\chatonster\_work\Hwpcolab-Beta-resume
- Local branch: codex/beta-hardening-resume-2026-11-04
- Local HEAD: 699af2903b70ce20c3d855c8306e2a97d92036ed
- Local and remote PR branches diverged from common ancestor 7be157a: local is 6 commits ahead and 8 behind remote. Do not push local HEAD directly or force-push. Reconcile local working-tree changes onto current PR head using expected-head guarded GitHub commits or a deliberate cherry-pick/rebase after inspecting conflicts.
- Local worktree is dirty: 9 modified files plus this memory file; approximate source diff before memory is +883/-41 lines.
- Use normal fast-forward updates with an exact expected-head guard. Never force-push.

## API provider priority and secrets

- Primary secret file: C:\Users\jun\Documents\chatonster\z.aifree.env
- Backup secret file: C:\Users\jun\Documents\chatonster\backup_baze_api.env
- User instruction: test primary Z.ai first; use BAZE only after a real primary failure.
- Last primary test: glm-4.7-flash returned HTTP 429, code 1305 (temporary overload).
- Only after primary failure, BAZE gpt-6-luna model listing and chat completion returned HTTP 200 with a non-empty answer.
- Never print, prompt with, commit, or copy API key values. Record provider/model/status/error code only.

## Local HWPX patch not yet reconciled to remote PR

Existing published work includes raw ZIP central-directory inspection, OWPML checks, strict first/stored mimetype validation, MIME payload CRC, ZIP64 rejection, encrypted/malformed/duplicate/empty-name rejection, and local-header parity checks.

Latest local implementation additions:
1. Reject overlapping complete local ZIP record ranges, including header, name/extra, compressed payload, and descriptor.
2. Accept both signed and unsigned ordinary-entry data descriptors; keep mimetype strict.
3. Validate descriptor CRC, compressed size, and uncompressed size against central-directory values.
4. Add a nested-record fixture: an otherwise valid Contents/metadata.xml local header is embedded inside stored Contents/section0.xml data, then the metadata central entry points to that embedded header. Reject because the record ranges overlap.
5. Add regressions for unsigned descriptors and independent CRC/compressed-size/uncompressed-size corruption.
6. Use full Unicode name folding: JS unicode-case-folding 1.1.2; Rust unicase 2.10.0.
7. Fix Rust UniCase key construction to use name.clone() to avoid Clippy E0382.

## Current local changed files

- BETA_REVIEW_LOG.md
- docs/superpowers/plans/2026-11-04-beta-hardening-plan.md
- package.json and package-lock.json
- src-tauri/Cargo.toml and src-tauri/Cargo.lock
- src-tauri/src/hwpx_package.rs
- src/hop-host/core/hwpx-package.test.ts
- src/hop-host/core/hwpx-package.ts
- TEMP_MEMORY.md

## Verification evidence and known gaps

- Latest remote PR head 2407072f96b2a9415be638a96bce055a7d7b0792 has Actions run 41803650311, completed success. All three jobs succeeded: Sync dependency lockfiles, Windows Rust tests, Frontend tests and build. This verifies that remote head, not the later local-only additions.
- Current local focused HWPX suite: 24/24 passed.
- Before the latest overlap/unsigned-descriptor additions, the full Vitest suite had 223 passing tests; rerun full suite for current count.
- Earlier TypeScript compilation, Vite build, and npm audit passed before the latest overlap/descriptor additions; rerun on current patch.
- Cargo/rustc/rustfmt are unavailable in the local COS Windows workspace. Fresh Windows CI is authoritative for new native tests.
- Packaged Windows Tauri WebView smoke test remains outstanding: real HWPX open/edit/save/reopen, installed-font enumeration/rendering, and LLM UI/cancellation.
- First read-only Rust reviewer found full Unicode-fold and ordinary local-header mismatch; those were addressed. Fresh review is required against the newest published head.

## Required next actions

1. Read TEMP_MEMORY.md, BETA_REVIEW_LOG.md, and docs/superpowers/plans/2026-11-04-beta-hardening-plan.md.
2. Refresh PR #1 and Beta/main refs before writing.
3. Confirm the overlap fixture creates intersecting local-record ranges and is rejected for that reason.
4. Run focused HWPX 24 tests, full Vitest, TypeScript compile, Vite build, npm audit --audit-level=high, and git diff --check.
5. Inspect Rust fixtures/code; do not mark native tests passed until fresh Windows CI does so.
6. Reconcile local resume history (6 ahead/8 behind from common ancestor 7be157a) onto current remote PR head. Preserve bot-generated lockfile and formatting commits.
7. Publish only to codex/beta-hardening-2026-11-04 with an expected-head guard. Never write directly to Beta or main.
8. Inspect current-head Windows rustfmt, Clippy and cargo test --locked steps.
9. Reuse a suitable sleeping reviewer or spawn fresh read-only Rust and TS reviewers against the exact new PR SHA.
10. Update this file and BETA_REVIEW_LOG.md after each meaningful cycle.

## Reporting rules

- Distinguish local test evidence, CI evidence, static review, and unrun tests.
- Never claim a push, commit, test, CI run, or reviewer result without tool evidence.
- Do not merge PR #1 without a separate explicit user instruction.
