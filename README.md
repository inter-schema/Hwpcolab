# HOP-Claw (Hwpcolab)
> **로컬 설치형 AI HWP/HWPX 문서 협업 데스크톱 애플리케이션**

HOP-Claw는 외부 서버에 한글 문서를 업로드하지 않고, 로컬 환경에서 안전하게 인라인 AI 교정, Smart Diff 검토, 표 채우기 등을 수행할 수 있는 오픈소스 데스크톱 프로그램입니다.

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
5. **다양한 AI 프로바이더 지원**:
   - OpenAI, Anthropic Claude, Google Gemini, BAZE API Gateway, 로컬 Ollama (`127.0.0.1:11434`) 지원.

---

## 🚀 빠른 시작 (Getting Started)

### 필수 요구 사항
- Node.js 20+
- Rust (Cargo) 및 Tauri v2 환경

### 설치 및 개발 실행
```bash
# 저장소 복제
git clone https://github.com/inter-schema/Hwpcolab.git
cd Hwpcolab

# 의존성 설치
npm install

# 프론트엔드 개발 서버 실행
npm run dev

# Tauri 데스크톱 앱 실행
npm run tauri dev
```

### 프로덕션 빌드
```bash
# 데스크톱 바이너리 빌드 (.exe / installer)
npm run tauri build
```

---

## 📄 라이선스 (License)

본 프로젝트는 **[GNU General Public License v3.0 (GPL-3.0)](./LICENSE)** 하에 배포됩니다.
- 본 소프트웨어는 Edward Kim의 [`@rhwp/core`](https://github.com/hahnlee/rhwp) (MIT), DoHyun468의 [`claw-hwp`](https://github.com/DoHyun468/claw-hwp) (MIT), `golbin/hop` (MIT), SheetJS `cfb` (Apache-2.0) 오픈소스 프로젝트를 기반으로 합니다.
