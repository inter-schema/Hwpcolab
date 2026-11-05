# HOP-Claw (Hwpcolab)
> **로컬 설치형 AI HWP/HWPX 문서 협업 데스크톱 애플리케이션**

HOP-Claw는 외부 서버에 한글 문서를 업로드하지 않고, 로컬 환경에서 안전하게 인라인 AI 교정, Smart Diff 검토, 표 채우기 등을 수행할 수 있는 오픈소스 데스크톱 프로그램입니다.

---

## 📊 현재 프로젝트 상태 (Project Status: 2026-11-05)

- **활성 브랜치**: `codex/beta-hardening-2026-11-05`
- **통합 대상**: `Beta`
- **단위 테스트**: **34개 파일, 242개 테스트 전원 통과 (100% Pass, 0 Fail)** (`npm test`)
- **E2E 및 회귀 검증**:
  - `verify_all_3_fixes.mjs`: **17/17 통과** (대표 글꼴 자동화, 파일 열기 라우팅, 드롭다운 팝업/IME 조합 유지)
  - `verify_ui_and_gpl.mjs`: **23/23 통과** (UI 레이아웃, 크기 조절 분할바, GPL v3 모달)
- **데스크톱 패키징**:
  - `hop-claw.exe` (x86_64 단독 바이너리, ~50.5 MiB)
  - `HOP-Claw_0.1.0_x64_en-US.msi` (WiX MSI 인스톨러, ~33.6 MiB)
  - `HOP-Claw_0.1.0_x64-setup.exe` (NSIS 셋업 인스톨러, ~30.5 MiB)

---

## 🌟 주요 기능 (Key Features)

1. **오프라인 우선 & 데이터 프라이버시 보호 (Privacy-First)**:
   - 문서를 외부 클라우드에 업로드하지 않고 로컬에서 WASM 및 Node.js 사이드카로 직접 렌더링/편집합니다.
   - 개인정보(주민등록번호, 계좌번호 등) 제로-컨텍스트 마스킹 지원.
2. **듀얼 인터랙션 모드 (Dual Mode UI)**:
   - **인라인 플로팅 툴바**: 문서 본문 및 표 셀 드래그 시 즉각 팝업 (공문서 어투 변환, 요약, 맞춤법 교정, 표 채우기).
   - **AI 채팅 사이드바**: 문서 전체 분석, 다중 참조 문서(Excel, PDF, DOCX, HWP) 파싱 및 종합 편집.
3. **Smart Diff & 부분 수락/반려 (Smart Diff Engine)**:
   - AI가 제안한 변경 사항을 단락/셀 단위로 시각적 비교(Visual Diff)하고 원하는 항목만 선별 반영.
4. **.hwp & .hwpx 완벽 지원 (Dual-Engine Mutation)**:
   - `@rhwp/core` WASM 기반 렌더링 및 `claw-hwp` 기반 바이트 레벨/OWPML XML 패치.
   - 7개 HWP 언어 슬롯(한글/영문/한자/일어/기타/기호/사용자) 자동 글꼴 ID 매핑 (`rhwp#8140`).
5. **다양한 AI 프로바이더 지원**:
   - OpenAI, Anthropic Claude, Google Gemini, BAZE API Gateway, 로컬 Ollama (`127.0.0.1:11434`) 지원.

---

## 🚀 빠른 시작 (Getting Started)

### 필수 요구 사항
- Node.js 20+
- Rust (Cargo) 및 Tauri v2 환경 (Windows MinGW/GNU 또는 MSVC)

### 설치 및 개발 실행
```bash
# 저장소 복제
git clone https://github.com/inter-schema/Hwpcolab.git
cd Hwpcolab

# 의존성 설치
npm install

# 프론트엔드 개발 서버 실행
npm run dev

# Tauri 데스크톱 앱 실행 (개발 모드)
npm run tauri dev
```

---

## 🧪 테스트 및 품질 검증 가이드 (Testing Guide)

```bash
# 1. 단위 테스트 실행 (Vitest)
npm test

# 2. 타입 검사 및 프론트엔드 빌드 검증
npm run build

# 3. Playwright E2E 전체 회귀 테스트 실행
node tests/verify_ui_and_gpl.mjs
```

---

## 📦 데스크톱 프로덕션 패키징 (Packaging Guide)

Windows 환경에서 MinGW-w64 툴체인을 사용하여 릴리스 실행 파일 및 MSI/NSIS 인스톨러를 생성하는 방법입니다:

```powershell
# 1. 환경 변수 설정 (Cargo 및 MinGW bin 경로가 PATH에 포함되어야 함)
. [local path redacted]

# 2. Tauri 릴리스 빌드 및 번들 생성
npx tauri build

# 생성되는 패키지 아티팩트 위치:
# - 단독 실행 바이너리: src-tauri/target/release/hop-claw.exe
# - MSI 인스톨러: src-tauri/target/release/bundle/msi/HOP-Claw_0.1.0_x64_en-US.msi
# - NSIS 셋업 설치 파일: src-tauri/target/release/bundle/nsis/HOP-Claw_0.1.0_x64-setup.exe
```

---

## 📄 라이선스 (License)

본 프로젝트는 **[GNU General Public License v3.0 (GPL-3.0)](./LICENSE)** 하에 배포됩니다.
- 본 소프트웨어는 Edward Kim의 [`@rhwp/core`](https://github.com/hahnlee/rhwp) (MIT), DoHyun468의 [`claw-hwp`](https://github.com/DoHyun468/claw-hwp) (MIT), `golbin/hop` (MIT), SheetJS `cfb` (Apache-2.0) 오픈소스 프로젝트를 기반으로 합니다.
