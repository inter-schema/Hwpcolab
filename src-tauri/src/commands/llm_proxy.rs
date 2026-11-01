use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

pub fn get_cancel_map() -> &'static Arc<Mutex<HashMap<String, tokio::sync::oneshot::Sender<()>>>> {
    static CANCEL_MAP: OnceLock<Arc<Mutex<HashMap<String, tokio::sync::oneshot::Sender<()>>>>> = OnceLock::new();
    CANCEL_MAP.get_or_init(|| Arc::new(Mutex::new(HashMap::new())))
}

#[derive(Deserialize, Debug)]
pub struct LlmProxyRequest {
    pub url: String,
    pub headers: HashMap<String, String>,
    pub body_json: serde_json::Value,
    pub timeout_ms: Option<u64>,
    pub request_id: Option<String>,
}

#[derive(Serialize, Debug)]
pub struct LlmProxyResponse {
    pub status: u16,
    pub body: serde_json::Value,
}

#[tauri::command]
pub async fn cancel_llm_request(request_id: String) -> Result<bool, String> {
    let map = get_cancel_map();
    let mut lock = map.lock().map_err(|e| e.to_string())?;
    if let Some(tx) = lock.remove(&request_id) {
        let _ = tx.send(());
        Ok(true)
    } else {
        Ok(false)
    }
}

/// WebView CORS를 완벽히 우회하는 범용 LLM HTTP POST 프록시 (Claude / OpenAI / Gemini / 로컬 Ollama 공용)
#[tauri::command]
pub async fn call_llm_http(req: LlmProxyRequest) -> Result<LlmProxyResponse, String> {
    let timeout_ms = req.timeout_ms.unwrap_or(180_000);
    let client = reqwest::Client::builder()
        .timeout(Duration::from_millis(timeout_ms))
        .build()
        .map_err(|e| format!("Reqwest 클라이언트 생성 실패: {}", e))?;

    let mut builder = client.post(&req.url);
    for (k, v) in req.headers {
        builder = builder.header(&k, &v);
    }

    let request_id = req.request_id.clone();
    let (tx, rx) = tokio::sync::oneshot::channel::<()>();

    if let Some(ref rid) = request_id {
        let map = get_cancel_map();
        if let Ok(mut lock) = map.lock() {
            lock.insert(rid.clone(), tx);
        }
    }

    struct CancelCleanup(Option<String>);
    impl Drop for CancelCleanup {
        fn drop(&mut self) {
            if let Some(ref rid) = self.0 {
                if let Ok(mut lock) = get_cancel_map().lock() {
                    lock.remove(rid);
                }
            }
        }
    }
    let _cleanup = CancelCleanup(request_id.clone());

    let resp_res = tokio::select! {
        res = builder.json(&req.body_json).send() => {
            res.map_err(|e| format!("LLM 네트워크 요청 실패: {}", e))?
        }
        _ = rx => {
            return Err("LLM 요청이 사용자에 의해 취소되었습니다.".to_string());
        }
    };

    let status = resp_res.status().as_u16();
    let text = resp_res.text().await.map_err(|e| e.to_string())?;
    let body: serde_json::Value = serde_json::from_str(&text)
        .unwrap_or_else(|_| serde_json::json!({ "raw_text": text }));

    Ok(LlmProxyResponse { status, body })
}
