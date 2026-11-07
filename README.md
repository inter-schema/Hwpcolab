# HOP-Claw (Hwpcolab)

<div align="center">

![HOP-Claw Logo](https://img.shields.io/badge/HOP--Claw-AI_한글_협업_스튜디오-2563eb?style=for-the-badge&logo=electron&logoColor=white)
<br/>

[![GPL-3.0 License](https://img.shields.io/badge/License-GPL_v3.0-blue.svg)](./LICENSE)
[![Tests Passing](https://img.shields.io/badge/Tests-58%20Files%20%7C%20430%20Passed%20(100%25)-success.svg)](https://github.com/inter-schema/Hwpcolab/tree/main/tests)
[![Tauri v2](https://img.shields.io/badge/Tauri-v2.0-orange.svg)](https://v2.tauri.app/)
[![React 19](https://img.shields.io/badge/React-19.0-61dafb.svg)](https://react.dev/)
[![WASM Core](https://img.shields.io/badge/Engine-@rhwp/core%20+%20claw--hwp-purple.svg)](https://github.com/hahnlee/rhwp)
[![Branch main](https://img.shields.io/badge/Branch-main-green.svg)](https://github.com/inter-schema/Hwpcolab/tree/main)

<p align="center">
  <b>로컬 설치형 AI HWP / HWPX 문서 협업 데스크톱 애플리케이션</b><br/>
  외부 클라우드 파일 업로드 없이 로컬 WASM/Node.js 사이드카 엔진으로 직접 렌더링하고 편집하는 프라이버시 우선 AI 한글 오피스
</p>

</div>

---

## 🎥 주요 기능 시연 (Walkthrough Video)

> 아래 영상에서 **HWPX 문서 로드, 중첩 표(표 안의 표) 생성, 인라인 AI 툴바 교정, 작성 선호사항 및 AI 설정 모달 워크플로우**를 확인하실 수 있습니다.

[🎬 HOP-Claw 기능 시연 영상 재생 (docs/videos/hop_claw_feature_walkthrough.webm)](docs/videos/hop_claw_feature_walkthrough.webm)

---

## 🖼️ 화면 갤러리 (Feature Visual Gallery)

### 1. 메인 작업 공간 및 AI 사이드바 (Main Workspace & AI Assistant)
HWPX 공문서 원본 서식을 100% 유지한 채 화면에 렌더링하며, 좌측 AI 어시스턴트 패널에서 단락별 Diff 검토, 즉각 반영, 다중 첨부자료(Excel, PDF, HWP) 파싱을 지원합니다.
![메인 작업 공간](docs/images/01_workspace_overview.png)

---

### 2. [전후 비교 시연] 표 레이아웃 실측 진단 및 자동 교정 (Before & After Layout Optimization)
표 내부 셀 너비가 어긋나 텍스트가 잘리거나 침범하는 레이아웃 이슈가 발생했을 때, AI 어시스턴트(BAZE `gpt-6-luna`)가 에이전틱 실측 루프를 통해 페이지 인쇄 가능 폭(150mm)에 맞추어 표와 행 높이를 자동으로 최적화 교정합니다.

| 수정 전 (Before): 셀 폭 붕괴 및 텍스트 넘침/잘림 | AI 실측 추론 루프 (이슈 4건 진단 및 해소) | 수정 후 (After): 인쇄 폭 150mm 실측 교정 완료 |
| :---: | :---: | :---: |
| ![수정 전: 셀 폭 붕괴 및 텍스트 넘침](docs/images/02a_layout_before.png) | ![AI 실측 추론 루프](docs/images/02b_ai_reasoning_trace.png) | ![수정 후: 실측 교정 완료](docs/images/02c_layout_after.png) |

---

### 3. HWPX 표 안의 표(중첩 표) 생성 및 실시간 렌더링 (Nested Table)
기존 한글 에디터 엔진의 한계를 극복하고, 사용자가 원하는 셀 내부 어디서든 계층형 2단 중첩 표(`insert_nested_table`)를 클릭 한 번으로 생성하고 화면 중앙에서 즉시 확인 및 편집할 수 있습니다.
![중첩 표 생성 화면](docs/images/03_nested_table_in_action.png)

---

### 4. 직관적인 인라인 플로팅 퀵 툴바 (Inline Quick AI Toolbar)
문서 본문 드래그나 표 셀 선택 시 커서 옆에 즉각 팝업되어 공문서 개조식 변환, 맞춤법 교정, 표 채우기, 그리고 **[⊞ 표 안에 표]** 삽입 기능을 마우스 원클릭으로 실행합니다.
![인라인 플로팅 퀵 툴바](docs/images/04_inline_quick_toolbar.png)

---

### 5. 사용자 지정(Custom) AI 공급자 설정 & 화면 스크롤 개선
화면 해상도에 구애받지 않는 반응형 모달 설계(`max-h-[88vh]` 및 고정 푸터)가 적용되어, 스크롤 후에도 **"설정 저장" 버튼이 항상 하단에 고정**되며 시각적 저장 완료 피드백을 제공합니다. (사내 LLM 엔드포인트 및 API 키 안전 마스킹 지원)
![AI 프로바이더 설정창](docs/images/05_provider_settings_modal.png)

---

### 6. 작성 선호사항 (Writing Memory) & 12종 PII 비식별화
기존 공문서나 보고서에서 작성 스타일과 문체 규칙을 추출하여 보관하며, 단방향 영구 비식별화(주민번호, 계좌번호 등 12종 PII 마스킹) 가이드가 모달 상단에 친절하게 안내됩니다.
![작성 선호사항 모달](docs/images/06_writing_memory_guide.png)

---

## 💡 기술 심층 분석: 왜 HWPX에서만 중첩 표가 지원되는가? (HWPX vs HWP)

HOP-Claw는 **HWPX 파일에서만 중첩 표(`insert_nested_table`) 생성을 허용**하고, 레거시 HWP 5.0에서는 안전 가드로 이를 원천 차단합니다. 이는 두 포맷의 근본적인 바이너리/데이터 구조 차이 때문입니다:

| 비교 항목 | 레거시 HWP 5.0 (바이너리) | HWPX (KS X 6101 / OWPML 표준) |
| :--- | :--- | :--- |
| **저장 형식** | Microsoft OLE/CFB 기반 복합 이진 스트림 (`BodyText/SectionN`) | ZIP 압축 기반 표준 XML DOM 패키지 (`Contents/section0.xml`) |
| **표 데이터 구조** | `HWPTAG_TABLE` 및 `HWPTAG_CTRL_HEADER`의 고정 바이트 길이 레코드 | `<hp:tbl>` 태그 및 `<hp:tr>`, `<hp:tc>` 계층 구조 태그 |
| **셀 내부 데이터** | 셀마다 고정 크기 레코드 헤더와 바이트 오프셋 포인터로 텍스트 관리 | `<hp:tc>` 내부에 임의의 문단(`<hp:p>`) 및 하위 `<hp:subList>` 포함 |
| **중첩 표 주입 시 결과** | ❌ **치명적 파일 손상 (File Corrupted)**<br/>셀 레코드 내에 하위 표 바이트 블록을 강제 삽입하면 OLE 스트림의 전체 레코드 오프셋 테이블과 컨트롤 ID 포인터가 어긋나 한컴오피스 실행 시 복구 불가능한 손상 오류 발생 | ✅ **완벽한 구조적 호환성 (Valid DOM)**<br/>XML 표준 규격에 따라 `<hp:tc>`의 `<hp:subList>` 안에 새로운 `<hp:tbl>` 노드를 삽입하므로 포인터 붕괴 없이 한컴오피스 및 모든 뷰어에서 정상 인식 |
| **HOP-Claw 방어 전략** | Policy Guard (G1/G6)가 레거시 HWP 대상 중첩 표 연산을 차단하여 원본 문서 파괴 방지 | `claw-hwp` 기반 XML 노드 패치 및 `buildTablesMetaForHwpx` 프리오더 순회 인덱싱 완벽 지원 |

> ### 🔄 기존 HWP 문서 변환 안내 (HWP ➔ HWPX)
> 기존에 작성된 레거시 바이너리 HWP 문서를 HWPX로 변환하여 중첩 표 및 최신 XML 서식 기능을 온전히 활용하실 수 있습니다:
> - **한컴 공식 무료 변환 도구**: [한글과컴퓨터 다운로드 센터](https://www.hancom.com/cs_center/csDownload.do)에서 공식 배포 중인 **[HWPX 변환기(Add-in)]**를 다운로드하여 대량의 HWP 파일들을 일괄 자동 변환할 수 있습니다.
> - **한컴오피스 한글 프로그램 변환**: 한글 2014 VP, 2018, 2020, 2022, 2024 등 한글 오피스가 설치된 PC에서는 기존 문서를 연 후 `F12` 키 또는 **[파일] ➔ [다른 이름으로 저장하기]** 메뉴를 누르고, 파일 형식을 **한글 표준 문서 (*.hwpx)**로 지정하여 저장하시면 즉시 HWPX 표준 문서로 전환됩니다.

---

## ⚡ 에이전틱 루프 듀얼 아키텍처 (Legacy vs v2 State Machine)

HOP-Claw는 상단 설정에서 사용자가 에이전트 루프 방식을 자유롭게 선택할 수 있습니다:
1. **기존 루프 (Legacy Loop - 기본/안정화)**:
   - OpenAI 호환 단일 루프 기반으로 정밀 레이아웃 추론 및 도구 호출 수행.
   - 오랜 기간 테스트되어 높은 신뢰도와 안정성 제공.
2. **개선 루프 (v2 State Machine - 베타/고도화)**:
   - 공급자별 독립 어댑터(`ClaudeAdapter`, `OpenAiCompatAdapter`) 분리.
   - 정밀 상태 전이 머신(Init → Inspect → Trial Plan → Verification → Proposal).
   - 400 `reasoning_effort` 자동 자가 복구, 일시적 네트워크 오류 지수 백오프, 메타데이터 SSRF 엔드포인트 보안 검증 내장.

---

## 🛡️ 개인정보 보호 및 작성 선호사항 (Privacy & Writing Memory)

- **12종 민감 개인정보(PII) 자동 단방향 마스킹**:
  - 주민등록번호, 외국인등록번호, 여권번호, 운전면허번호, 전화번호, 이메일, 계좌번호, 카드번호, 주소, IP 주소, 사업자번호, 개인 식별 명칭 등
  - LLM API 서버로는 오직 임의의 토큰(예: `[주민등록번호_1]`)만이 전송되며, 외부에서 원본을 복원하는 언마스킹 수단은 원천 차단됩니다.
- **작성 선호사항 (Writing Memory)**:
  - 사용자가 지정한 여러 참고 문서에서 반복되는 양식, 서식, 어투, 결재선 표기법을 학습하여 메모리로 저장.
  - 사이드바에서 원클릭으로 주입 여부를 토글하여 일관된 공문서 작성 지원.

---

## 🚀 빠른 시작 (Getting Started)

### 📥 1. 설치 파일 다운로드 (GitHub Releases 권장)

최신 빌드 버전의 실행 파일 및 인스톨러를 다운로드하여 별도의 개발 환경 구축 없이 즉시 사용할 수 있습니다:

🔗 **[GitHub Releases 최신 릴리스 다운로드 페이지](https://github.com/inter-schema/Hwpcolab/releases)**

| 배포 형태 | 파일명 | 설명 |
| :--- | :--- | :--- |
| **무설치 포터블 패키지 (권장)** | `HOP-Claw-v0.2.0-portable.zip` (~35 MiB) | 압축 해제 후 즉시 실행 (`hop-claw.exe` + `WebView2Loader.dll` + `resources/` 내장 포터블 패키지) |
| **NSIS 설치 마법사** | `HOP-Claw_0.2.0_x64-setup.exe` (~30.5 MiB) | Windows 표준 설치 마법사 (바탕화면/시작메뉴 바로가기 생성 및 DLL·사이드카 자동 구성) |
| **MSI 인스톨러** | `HOP-Claw_0.2.0_x64_en-US.msi` (~33.6 MiB) | 공공기관 및 사내 IT 자산 관리(GPO) 배포에 적합한 표준 MSI 패키지 |

> 💡 **무설치(포터블) 실행 안내**:
> HOP-Claw는 MinGW 환경 및 Node.js 기반 HWP/HWPX 사이드카 엔진(`claw-hwp`) 구조를 갖추고 있습니다. 따라서 무설치 버전은 단독 `.exe` 하나만으로는 구동되지 않으며, `WebView2Loader.dll` 및 `resources/` 폴더가 반드시 동일한 위치에 있어야 정상 동작합니다.
> 이에 따라 무설치 버전은 **ZIP 패키지(`HOP-Claw-v0.2.0-portable.zip`)로 제공**되며, 원하는 폴더에 압축을 푼 뒤 `hop-claw.exe`를 실행하시면 됩니다. 시스템에 의존성 파일을 자동으로 구성하고 바로가기를 생성하려면 **NSIS 설치 파일(`HOP-Claw_0.2.0_x64-setup.exe`)** 사용을 권장합니다.

---

### 💻 2. 개발자용 소스 빌드 및 직접 실행

소스 코드에서 직접 빌드하거나 커스터마이징을 진행하는 개발 환경 설정 방법입니다:

#### 필수 요구 사항
- **Node.js**: v20.0.0 이상
- **Rust & Cargo**: 1.75 이상 (Tauri v2 데스크톱 빌드 시)
- **OS**: Windows 10/11 (64-bit)

#### 저장소 복제 및 개발 서버 실행
```bash
# 1. 저장소 복제
git clone https://github.com/inter-schema/Hwpcolab.git
cd Hwpcolab

# 2. 패키지 의존성 설치
npm install

# 3. 프론트엔드 Vite 개발 서버 실행
npm run dev

# 4. Tauri 데스크톱 애플리케이션 개발 모드 실행
npm run tauri dev
```

---

## 🧪 테스트 및 품질 검증 (Test Suite)

HOP-Claw는 100% 테스트 주도 품질 보증을 유지합니다:

```bash
# 단위 테스트 전체 실행 (58개 파일, 430개 테스트 전원 통과)
npm test

# 타입 검사 및 프론트엔드 프로덕션 빌드 번들링 검증
npm run build

# E2E 공급자 및 루프 모드 검증
node tests/e2e_providers_loops.mjs

# E2E 작성 선호사항 & PII 비식별화 검증
node tests/e2e_writing_memory.mjs
```

---

## 📦 프로덕션 데스크톱 패키징 (Packaging Guide)

Windows 환경에서 MinGW-w64 툴체인을 사용하여 상용 수준의 단독 실행 바이너리 및 인스톨러를 빌드합니다:

```powershell
# 1. MinGW 툴체인 및 환경 변수 로드
. C:\Users\jun\tools\env.ps1

# 2. Tauri 프로덕션 릴리스 빌드
npx tauri build
```

빌드 완료 시 다음 위치에 배포 아티팩트가 생성됩니다:
- **단독 실행 바이너리**: `src-tauri/target/release/hop-claw.exe` (~50.5 MiB)
- **MSI 설치 파일**: `src-tauri/target/release/bundle/msi/HOP-Claw_0.2.0_x64_en-US.msi` (~33.6 MiB)
- **NSIS 셋업 설치 파일**: `src-tauri/target/release/bundle/nsis/HOP-Claw_0.2.0_x64-setup.exe` (~30.5 MiB)

---

## 📄 라이선스 (License & Attribution)

본 프로젝트는 **[GNU General Public License v3.0 (GPL-3.0)](./LICENSE)** 하에 배포됩니다.

### 주요 오픈소스 기반
- **[`@rhwp/core`](https://github.com/hahnlee/rhwp)** (Edward Kim, MIT License) — WASM 기반 HWP/HWPX 문서 바이너리 파서 및 레이아웃 렌더러
- **[`claw-hwp`](https://github.com/DoHyun468/claw-hwp)** (DoHyun468, MIT License) — Node.js 사이드카 기반 HWP 바이너리/HWPX XML 변이 엔진
- **[`golbin/hop`](https://github.com/golbin/hop)** (golbin, MIT License) — 한글 문서 뷰어 및 웹 편집 인터페이스 프로토타입
- **[`SheetJS cfb`](https://github.com/SheetJS/sheetjs)** (Apache-2.0) — OLE/CFB 복합 파일 파싱 유틸리티
