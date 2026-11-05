# Hwpcolab Beta Hardening — Temporary Working Memory

> Long-running engineering handoff. Read this file before resuming. Update it after every material test, review, commit, or CI result. This is working context, not a substitute for source, tests, or CI evidence.

Updated: 2026-11-05 21:00 Asia/Seoul

## Repository boundaries

- Repository: `inter-schema/Hwpcolab`
- Integration base: `Beta` (current origin/Beta: `fb05cd96591f585234a6b443bc19f8378c40b726`)
- Current working branch: `codex/beta-hardening-2026-11-05`
- Previous hardening branch: `codex/beta-hardening-2026-11-04` (head: `f4039b20ca42269913881d4e05e076a0e5b1cc4e`)
- Origin remote: `https://github.com/inter-schema/Hwpcolab.git`
- Push permission: Confirmed (authenticated as `inter-schema`)

## Verification status (2026-11-05)

1. **Unit Tests (`vitest run`)**:
   - **34 files, 242 tests passed, 0 failures**.
   - Verified suites: `font-ids.test.ts`, `pending-char-shape-ime.test.ts`, `toolbar.test.ts`, `custom-select.test.ts`, `file.test.ts`.

2. **Frontend Build (`tsc && vite build`)**:
   - Zero TypeScript compile errors. Clean production bundle output in ~15s.

3. **Live Playwright E2E Verification (`verify_all_3_fixes.mjs`)**:
   - **17/17 passed (100%)** covering all 3 user-reported bug fixes:
     - Bug 1 (`rhwp#8140`): Font language auto-resolution in '대표' mode across all 7 language tables; dynamic activeFontLang toolbar display.
     - Bug 2: File open routing via header input, `Ctrl+O` shortcut, and HOP menu `파일 > 열기` with event bridging to React host.
     - Bug 3: Toolbar dropdown popups unclipped by `.sb-font { overflow: hidden }` via fixed floating portals, textarea focus preservation, and multi-stroke Korean IME formatting persistence.

4. **UI & GPL Suite (`verify_ui_and_gpl.mjs`)**:
   - **23/23 passed (100%)** covering split-pane slider, reasoning level selector, license dialog.

5. **Desktop Packaging (Tauri v2 + MinGW)**:
   - Toolchain environment loaded from `C:\Users\jun\tools\env.ps1`.
   - Standalone exe: `src-tauri/target/release/hop-claw.exe` (52.9 MB, valid PE x86_64).
   - WiX MSI installer: `src-tauri/target/release/bundle/msi/HOP-Claw_0.1.0_x64_en-US.msi` (35.2 MB).
   - NSIS setup installer: `src-tauri/target/release/bundle/nsis/HOP-Claw_0.1.0_x64-setup.exe` (31.9 MB).

## Next operator / agent guidelines

- When running Tauri commands locally, ensure MinGW bin is on PATH (`. C:\Users\jun\tools\env.ps1`) to provide `dlltool.exe` and `as.exe`.
- Any new test scripts or screenshots must be kept in scratch directories or gitignored folders to preserve git cleanliness.
- Target branch for integration remains `Beta`.
