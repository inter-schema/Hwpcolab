# BAZE API Gateway 설정 및 연동 사양 문서

이 문서는 HOP-Claw 프로젝트에서 마인드로직(Mindlogic)의 **BAZE API Gateway** 연동 사양을 관리하는 문서입니다.

---

## 1. BAZE API Gateway 정보

- **서비스명**: BAZE API Gateway (마인드로직 FactChat / BAZE Enterprise)
- **게이트웨이 Base URL**: `https://api.baze.co/v1/gateway`
- **Chat Completions 엔드포인트**: `https://api.baze.co/v1/gateway/chat/completions`
- **모델 목록 조회 엔드포인트**: `https://api.baze.co/v1/gateway/models/`
- **인증 방식**: `Authorization: Bearer <API_KEY>` 또는 `x-api-key: <API_KEY>` (둘 다 지원)

---

## 2. API 키 및 설정 모델

> **주의**: API 키는 보안을 위해 `.env` 파일 또는 애플리케이션 내 설정 UI를 통해 개별 입력해야 합니다.

```ini
BAZE_API_KEY=
BAZE_MODEL=gpt-6-luna
BAZE_GATEWAY_URL=https://api.baze.co/v1/gateway/chat/completions
```

- **API Key**: (별도 발급 필요)
- **지정 모델**: `gpt-6-luna`
- **검증 상태**: `https://api.baze.co/v1/gateway/models/` 정상 인증 확인 완료 (2026-10-31)

---

## 3. 게이트웨이 제공 주요 모델 목록 (검증 확인)

BAZE API Gateway에서 단일 키로 즉시 호출 가능한 주요 모델:

| 모델 ID | 제공사 | 유형 | 설명 |
| :--- | :--- | :--- | :--- |
| **`gpt-6-luna`** | OpenAI | LLM | **기본 적용 모델 (경량·초고속 추론)** |
| `gpt-6-sol` | OpenAI | LLM | 고성능 종합 모델 |
| `gpt-6.1-terra` | OpenAI | LLM | 차세대 최적화 모델 |
| `gpt-6.1-sol` | OpenAI | LLM | 차세대 플래그십 모델 |
| `gpt-5.6-terra` | OpenAI | LLM | 고성능 코딩 및 문서 작업 모델 |
| `gpt-5.6-sol` | OpenAI | LLM | 범용 플래그십 모델 |
| `claude-sonnet-4` | Anthropic | LLM | Claude 계열 모델 (`/v1/gateway/claude`) |
| `gemini-2.5-pro` | Google | LLM | Google Gemini 계열 (`/v1/gateway/gemini`) |

---

## 4. 프로젝트(HOP-Claw) 연동 구조

1. **상태 관리 (`src/store/useDocStore.ts`)**:
   - `provider`: `"baze"` (기본 활성화)
   - `apiKey`: `""` (환경 변수 또는 UI 입력값)
   - `model`: `gpt-6-luna`
   - `customEndpoint`: `https://api.baze.co/v1/gateway/chat/completions`

2. **AI 오케스트레이터 (`src/services/aiOrchestrator.ts`)**:
   - `provider === "baze"` 요청 시 `callOpenAiCompatibleApi`를 통해 BAZE 게이트웨이 엔드포인트로 HWP 편집 연산(`propose_hwp_edits`) 전송.

3. **CORS 우회 프록시 (`src-tauri/src/commands/llm_proxy.rs`)**:
   - Tauri 러스트 백엔드의 `call_llm_http` 커맨드를 통해 클라이언트 브라우저 CORS 제약 없이 `api.baze.co`로 직접 안전하게 전송.

4. **UI 설정 화면 (`src/components/sidebar/ModelPrivacySettings.tsx`)**:
   - AI 설정 모달에서 `BAZE` 탭을 선택하여 API 키 및 모델(`gpt-6-luna` 등) 변경 및 저장 가능.
