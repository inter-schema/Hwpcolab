use chrono::Utc;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
use uuid::Uuid;

use crate::error::MemoryError;
use crate::frontmatter::{
    normalize_memory_body, parse_memory_markdown, serialize_memory_markdown,
};
use crate::types::{BoxFuture, MemoryDoc, MemoryMeta, PiiRedactor};

pub const MEM_INDEX_KEY: &str = "hop_mem_index";
pub const MEM_DOC_PREFIX: &str = "hop_mem_";
pub const MAX_REVISIONS: usize = 5;

pub trait MemoryStorageBackend: Send + Sync {
    fn get_item<'a>(&'a self, key: &'a str) -> BoxFuture<'a, Result<Option<String>, MemoryError>>;
    fn set_item<'a>(
        &'a self,
        key: &'a str,
        value: &'a str,
    ) -> BoxFuture<'a, Result<(), MemoryError>>;
    fn remove_item<'a>(&'a self, key: &'a str) -> BoxFuture<'a, Result<(), MemoryError>>;
    fn keys(&self) -> BoxFuture<'_, Result<Vec<String>, MemoryError>>;
}

#[derive(Debug, Default)]
pub struct InMemoryStorage {
    map: RwLock<HashMap<String, String>>,
}

impl InMemoryStorage {
    pub fn new() -> Self {
        Self::default()
    }
}

impl MemoryStorageBackend for InMemoryStorage {
    fn get_item<'a>(&'a self, key: &'a str) -> BoxFuture<'a, Result<Option<String>, MemoryError>> {
        Box::pin(async move {
            let guard = self
                .map
                .read()
                .map_err(|e| MemoryError::Storage(format!("lock poisoned: {e}")))?;
            Ok(guard.get(key).cloned())
        })
    }

    fn set_item<'a>(
        &'a self,
        key: &'a str,
        value: &'a str,
    ) -> BoxFuture<'a, Result<(), MemoryError>> {
        Box::pin(async move {
            let mut guard = self
                .map
                .write()
                .map_err(|e| MemoryError::Storage(format!("lock poisoned: {e}")))?;
            guard.insert(key.to_string(), value.to_string());
            Ok(())
        })
    }

    fn remove_item<'a>(&'a self, key: &'a str) -> BoxFuture<'a, Result<(), MemoryError>> {
        Box::pin(async move {
            let mut guard = self
                .map
                .write()
                .map_err(|e| MemoryError::Storage(format!("lock poisoned: {e}")))?;
            guard.remove(key);
            Ok(())
        })
    }

    fn keys(&self) -> BoxFuture<'_, Result<Vec<String>, MemoryError>> {
        Box::pin(async move {
            let guard = self
                .map
                .read()
                .map_err(|e| MemoryError::Storage(format!("lock poisoned: {e}")))?;
            Ok(guard.keys().cloned().collect())
        })
    }
}

/// Directory-backed file storage for Writing Memory markdown & index files.
#[derive(Debug, Clone)]
pub struct FileMemoryStorage {
    root_dir: PathBuf,
}

impl FileMemoryStorage {
    pub fn new(root_dir: impl Into<PathBuf>) -> Self {
        Self {
            root_dir: root_dir.into(),
        }
    }

    fn key_to_path(&self, key: &str) -> Result<PathBuf, MemoryError> {
        if key.is_empty()
            || key.contains('/')
            || key.contains('\\')
            || key.contains("..")
        {
            return Err(MemoryError::Validation(format!(
                "잘못된 메모리 키입니다: {key}"
            )));
        }
        let ext = if key == MEM_INDEX_KEY { "json" } else { "md" };
        Ok(self.root_dir.join(format!("{key}.{ext}")))
    }
}

impl MemoryStorageBackend for FileMemoryStorage {
    fn get_item<'a>(&'a self, key: &'a str) -> BoxFuture<'a, Result<Option<String>, MemoryError>> {
        Box::pin(async move {
            let path = self.key_to_path(key)?;
            match tokio::fs::read_to_string(&path).await {
                Ok(s) => Ok(Some(s)),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
                Err(e) => Err(MemoryError::Io(e)),
            }
        })
    }

    fn set_item<'a>(
        &'a self,
        key: &'a str,
        value: &'a str,
    ) -> BoxFuture<'a, Result<(), MemoryError>> {
        Box::pin(async move {
            tokio::fs::create_dir_all(&self.root_dir).await?;
            let path = self.key_to_path(key)?;
            let tmp_path = self.root_dir.join(format!(".tmp_{}_{}", key, Uuid::new_v4()));
            tokio::fs::write(&tmp_path, value.as_bytes()).await?;
            tokio::fs::rename(&tmp_path, &path).await?;
            Ok(())
        })
    }

    fn remove_item<'a>(&'a self, key: &'a str) -> BoxFuture<'a, Result<(), MemoryError>> {
        Box::pin(async move {
            let path = self.key_to_path(key)?;
            match tokio::fs::remove_file(&path).await {
                Ok(()) => Ok(()),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
                Err(e) => Err(MemoryError::Io(e)),
            }
        })
    }

    fn keys(&self) -> BoxFuture<'_, Result<Vec<String>, MemoryError>> {
        Box::pin(async move {
            let mut out = Vec::new();
            let mut entries = match tokio::fs::read_dir(&self.root_dir).await {
                Ok(rd) => rd,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(out),
                Err(e) => return Err(MemoryError::Io(e)),
            };
            while let Some(entry) = entries.next_entry().await? {
                if let Some(stem) = entry
                    .path()
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .filter(|s| !s.starts_with(".tmp_"))
                {
                    out.push(stem.to_string());
                }
            }
            Ok(out)
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LegacyImportReport {
    pub imported_docs: usize,
    pub imported_revisions: usize,
    pub backup_path: Option<PathBuf>,
}

pub struct MemoryStore {
    storage: Arc<dyn MemoryStorageBackend>,
    redactor: Arc<dyn PiiRedactor>,
}

impl MemoryStore {
    pub fn new(
        storage: Arc<dyn MemoryStorageBackend>,
        redactor: Arc<dyn PiiRedactor>,
    ) -> Self {
        Self { storage, redactor }
    }

    pub async fn list(&self) -> Result<Vec<MemoryMeta>, MemoryError> {
        let Some(raw) = self.storage.get_item(MEM_INDEX_KEY).await? else {
            return Ok(Vec::new());
        };
        let Ok(mut parsed) = serde_json::from_str::<Vec<MemoryMeta>>(&raw) else {
            return Ok(Vec::new());
        };
        parsed.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        Ok(parsed)
    }

    pub async fn get(&self, id: &str) -> Result<Option<MemoryDoc>, MemoryError> {
        let Some(raw) = self
            .storage
            .get_item(&format!("{MEM_DOC_PREFIX}{id}"))
            .await?
        else {
            return Ok(None);
        };
        Ok(Some(parse_memory_markdown(&raw).doc))
    }

    pub async fn save(&self, doc: &MemoryDoc, record_revision: bool) -> Result<(), MemoryError> {
        let sanitized_name = self.redactor.scan_and_redact(&doc.meta.name).text;
        let sanitized_desc = self.redactor.scan_and_redact(&doc.meta.description).text;
        let sanitized_body =
            normalize_memory_body(&self.redactor.scan_and_redact(&doc.body).text);

        let existing = self.get(&doc.meta.id).await?;
        let next_revision = if let Some(ref existing_doc) = existing {
            if record_revision {
                self.storage
                    .set_item(
                        &format!("{MEM_DOC_PREFIX}{}_rev{}", doc.meta.id, existing_doc.meta.revision),
                        &serialize_memory_markdown(existing_doc),
                    )
                    .await?;

                let current_revs = self.list_revisions(&doc.meta.id).await?;
                if current_revs.len() > MAX_REVISIONS {
                    for rev in &current_revs[MAX_REVISIONS..] {
                        self.storage
                            .remove_item(&format!("{MEM_DOC_PREFIX}{}_rev{}", doc.meta.id, rev))
                            .await?;
                    }
                }
                existing_doc.meta.revision + 1
            } else {
                existing_doc.meta.revision
            }
        } else {
            doc.meta.revision.max(1)
        };

        let now = Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        let created_at = if doc.meta.created_at.is_empty() {
            now.clone()
        } else {
            doc.meta.created_at.clone()
        };

        let doc_to_save = MemoryDoc {
            meta: MemoryMeta {
                schema_version: 1,
                id: doc.meta.id.clone(),
                name: sanitized_name,
                description: sanitized_desc,
                revision: next_revision,
                created_at,
                updated_at: now,
                source_docs: doc.meta.source_docs.clone(),
                masked: true,
            },
            body: sanitized_body,
        };

        self.storage
            .set_item(
                &format!("{MEM_DOC_PREFIX}{}", doc_to_save.meta.id),
                &serialize_memory_markdown(&doc_to_save),
            )
            .await?;

        let mut index_list = self.list().await?;
        if let Some(pos) = index_list
            .iter()
            .position(|m| m.id == doc_to_save.meta.id)
        {
            index_list[pos] = doc_to_save.meta.clone();
        } else {
            index_list.push(doc_to_save.meta.clone());
        }
        index_list.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        self.storage
            .set_item(MEM_INDEX_KEY, &serde_json::to_string(&index_list)?)
            .await?;

        Ok(())
    }

    pub async fn delete(&self, id: &str) -> Result<(), MemoryError> {
        self.storage
            .remove_item(&format!("{MEM_DOC_PREFIX}{id}"))
            .await?;

        let rev_prefix = format!("{MEM_DOC_PREFIX}{id}_rev");
        for k in self.storage.keys().await? {
            if let Some(suffix) = k.strip_prefix(&rev_prefix) {
                if !suffix.is_empty() && suffix.chars().all(|c| c.is_ascii_digit()) {
                    self.storage.remove_item(&k).await?;
                }
            }
        }

        let index_list = self.list().await?;
        let filtered: Vec<MemoryMeta> = index_list.into_iter().filter(|m| m.id != id).collect();
        self.storage
            .set_item(MEM_INDEX_KEY, &serde_json::to_string(&filtered)?)
            .await?;
        Ok(())
    }

    pub async fn list_revisions(&self, id: &str) -> Result<Vec<u64>, MemoryError> {
        let rev_prefix = format!("{MEM_DOC_PREFIX}{id}_rev");
        let mut revs = Vec::new();
        for k in self.storage.keys().await? {
            if let Some(suffix) = k.strip_prefix(&rev_prefix) {
                if !suffix.is_empty() && suffix.chars().all(|c| c.is_ascii_digit()) {
                    if let Ok(rev) = suffix.parse::<u64>() {
                        revs.push(rev);
                    }
                }
            }
        }
        revs.sort_by(|a, b| b.cmp(a));
        Ok(revs)
    }

    pub async fn get_revision(
        &self,
        id: &str,
        rev: u64,
    ) -> Result<Option<MemoryDoc>, MemoryError> {
        let Some(raw) = self
            .storage
            .get_item(&format!("{MEM_DOC_PREFIX}{id}_rev{rev}"))
            .await?
        else {
            return Ok(None);
        };
        Ok(Some(parse_memory_markdown(&raw).doc))
    }

    pub async fn rollback_to_revision(
        &self,
        id: &str,
        rev: u64,
    ) -> Result<MemoryDoc, MemoryError> {
        let existing = self
            .get(id)
            .await?
            .ok_or_else(|| MemoryError::NotFound(format!("작성 선호사항을 찾을 수 없습니다: {id}")))?;
        let target = self
            .get_revision(id, rev)
            .await?
            .ok_or_else(|| MemoryError::NotFound(format!("리비전 {rev}를 찾을 수 없습니다: {id}")))?;

        let now = Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        let updated = MemoryDoc {
            meta: MemoryMeta {
                description: target.meta.description,
                updated_at: now,
                ..existing.meta
            },
            body: target.body,
        };
        self.save(&updated, true).await?;

        self.get(id)
            .await?
            .ok_or_else(|| MemoryError::NotFound(format!("롤백 후 작성 선호사항 조회 실패: {id}")))
    }

    pub async fn export_markdown(&self, id: &str) -> Result<String, MemoryError> {
        let doc = self
            .get(id)
            .await?
            .ok_or_else(|| MemoryError::NotFound(format!("작성 선호사항을 찾을 수 없습니다: {id}")))?;
        let re_checked = MemoryDoc {
            meta: MemoryMeta {
                name: self.redactor.scan_and_redact(&doc.meta.name).text,
                description: self.redactor.scan_and_redact(&doc.meta.description).text,
                masked: true,
                ..doc.meta
            },
            body: normalize_memory_body(&self.redactor.scan_and_redact(&doc.body).text),
        };
        Ok(serialize_memory_markdown(&re_checked))
    }

    pub async fn import_markdown(
        &self,
        raw_md: &str,
    ) -> Result<(MemoryDoc, Vec<String>), MemoryError> {
        let parsed = parse_memory_markdown(raw_md);
        let name_scan = self.redactor.scan_and_redact(&parsed.doc.meta.name);
        let desc_scan = self.redactor.scan_and_redact(&parsed.doc.meta.description);
        let body_scan = self.redactor.scan_and_redact(&parsed.doc.body);

        let existing = self.get(&parsed.doc.meta.id).await?;
        let final_id = if existing.is_some() {
            format!("mem_{}", Uuid::new_v4())
        } else {
            parsed.doc.meta.id
        };

        let mut warnings = Vec::new();
        let total_count = name_scan.total_count + desc_scan.total_count + body_scan.total_count;
        if total_count > 0 {
            warnings.push(format!(
                "가져온 파일에서 개인정보 패턴 {total_count}건을 자동 비식별화했습니다."
            ));
        }

        let sanitized = MemoryDoc {
            meta: MemoryMeta {
                id: final_id,
                name: name_scan.text,
                description: desc_scan.text,
                masked: true,
                ..parsed.doc.meta
            },
            body: normalize_memory_body(&body_scan.text),
        };

        self.save(&sanitized, false).await?;
        Ok((sanitized, warnings))
    }

    /// Imports legacy `localStorage` key/value snapshots (`hop_mem_*` and `hop_mem_index`),
    /// optionally writing a JSON backup file before applying entries.
    pub async fn import_legacy_local_storage(
        &self,
        entries: &HashMap<String, String>,
        backup_dir: Option<&Path>,
    ) -> Result<LegacyImportReport, MemoryError> {
        let backup_path = if let Some(dir) = backup_dir {
            tokio::fs::create_dir_all(dir).await?;
            let path = dir.join(format!(
                "legacy_localstorage_backup_{}.json",
                Utc::now().format("%Y%m%dT%H%M%S")
            ));
            let json = serde_json::to_string_pretty(entries)?;
            tokio::fs::write(&path, json.as_bytes()).await?;
            Some(path)
        } else {
            None
        };

        let mut imported_docs = 0usize;
        let mut imported_revisions = 0usize;

        for (k, v) in entries {
            if k == MEM_INDEX_KEY {
                continue;
            }
            if let Some(rest) = k.strip_prefix(MEM_DOC_PREFIX) {
                if rest.contains("_rev") {
                    self.storage.set_item(k, v).await?;
                    imported_revisions += 1;
                } else {
                    let parsed = parse_memory_markdown(v).doc;
                    self.save(&parsed, false).await?;
                    imported_docs += 1;
                }
            }
        }

        Ok(LegacyImportReport {
            imported_docs,
            imported_revisions,
            backup_path,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frontmatter::create_default_memory_body;
    use crate::types::{NoopRedactor, RedactOutcome, SourceDocRef};

    struct TestPiiRedactor;

    impl PiiRedactor for TestPiiRedactor {
        fn scan_and_redact(&self, input: &str) -> RedactOutcome {
            let mut text = input.to_string();
            let mut total = 0usize;
            for (needle, repl) in [
                ("홍길동", "<이름>"),
                ("010-1234-5678", "<전화번호>"),
                ("test@example.com", "<이메일>"),
                ("123-456-789012", "<계좌번호>"),
            ] {
                if text.contains(needle) {
                    text = text.replace(needle, repl);
                    total += 1;
                }
            }
            RedactOutcome {
                text,
                hits: Vec::new(),
                total_count: total,
            }
        }
    }

    fn make_doc(id: &str, name: &str, desc: &str) -> MemoryDoc {
        MemoryDoc {
            meta: MemoryMeta {
                schema_version: 1,
                id: id.to_string(),
                name: name.to_string(),
                description: desc.to_string(),
                revision: 1,
                created_at: "2026-11-05T00:00:00.000Z".to_string(),
                updated_at: "2026-11-05T00:00:00.000Z".to_string(),
                source_docs: vec![SourceDocRef {
                    name: "doc1.hwpx".to_string(),
                    sha256_8: "aabbccdd".to_string(),
                }],
                masked: true,
            },
            body: create_default_memory_body(),
        }
    }

    #[tokio::test]
    async fn crud_operations_work() {
        let store = MemoryStore::new(Arc::new(InMemoryStorage::new()), Arc::new(NoopRedactor));
        let doc = make_doc("mem_crud_1", "기본 공문서 양식", "행정기관 공문서 서식");

        store.save(&doc, true).await.unwrap();
        let retrieved = store.get("mem_crud_1").await.unwrap().unwrap();
        assert_eq!(retrieved.meta.id, "mem_crud_1");
        assert_eq!(retrieved.meta.name, "기본 공문서 양식");
        assert_eq!(retrieved.meta.revision, 1);

        let list = store.list().await.unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].id, "mem_crud_1");

        store.delete("mem_crud_1").await.unwrap();
        assert!(store.get("mem_crud_1").await.unwrap().is_none());
        assert!(store.list().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn retains_at_most_5_revisions_on_6th_edit() {
        let store = MemoryStore::new(Arc::new(InMemoryStorage::new()), Arc::new(NoopRedactor));
        let doc = make_doc("mem_rev_test", "개정 테스트", "버전 관리");
        store.save(&doc, false).await.unwrap();
        assert_eq!(store.get("mem_rev_test").await.unwrap().unwrap().meta.revision, 1);
        assert!(store.list_revisions("mem_rev_test").await.unwrap().is_empty());

        for i in 2..=7 {
            let mut current = store.get("mem_rev_test").await.unwrap().unwrap();
            current.meta.description = format!("설명 개정 {i}");
            store.save(&current, true).await.unwrap();
        }

        let latest = store.get("mem_rev_test").await.unwrap().unwrap();
        assert_eq!(latest.meta.revision, 7);

        let revs = store.list_revisions("mem_rev_test").await.unwrap();
        assert_eq!(revs, vec![6, 5, 4, 3, 2]);
        assert_eq!(
            store.get_revision("mem_rev_test", 6).await.unwrap().unwrap().meta.revision,
            6
        );
        assert!(store.get_revision("mem_rev_test", 1).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn rollback_preserves_pre_rollback_state_as_revision() {
        let store = MemoryStore::new(Arc::new(InMemoryStorage::new()), Arc::new(NoopRedactor));
        let initial = make_doc("mem_rollback_test", "롤백 테스트", "초기 버전 1");
        store.save(&initial, false).await.unwrap();

        let mut v2 = initial.clone();
        v2.meta.description = "수정 버전 2".to_string();
        v2.body = create_default_memory_body().replace(
            "## 어투·문체\n- (내용 없음)",
            "## 어투·문체\n- v2 특별 어투",
        );
        store.save(&v2, true).await.unwrap();

        let rolled_back = store.rollback_to_revision("mem_rollback_test", 1).await.unwrap();
        assert_eq!(rolled_back.meta.revision, 3);
        assert!(rolled_back.body.contains("- (내용 없음)"));
        assert!(!rolled_back.body.contains("v2 특별 어투"));

        let rev2 = store
            .get_revision("mem_rollback_test", 2)
            .await
            .unwrap()
            .unwrap();
        assert!(rev2.body.contains("v2 특별 어투"));
    }

    #[tokio::test]
    async fn export_and_import_redacts_pii_and_prevents_duplicate_ids() {
        let store = MemoryStore::new(
            Arc::new(InMemoryStorage::new()),
            Arc::new(TestPiiRedactor),
        );
        let mut doc = make_doc("mem_export_1", "기존 메모리 (담당: 홍길동)", "전화번호: 010-1234-5678");
        doc.body = create_default_memory_body().replace(
            "## 어투·문체\n- (내용 없음)",
            "## 어투·문체\n- 문의처: test@example.com, 계좌: 123-456-789012",
        );
        store.save(&doc, false).await.unwrap();

        let exported = store.export_markdown("mem_export_1").await.unwrap();
        assert!(exported.contains("<이름>"));
        assert!(exported.contains("<전화번호>"));
        assert!(exported.contains("<이메일>"));
        assert!(exported.contains("<계좌번호>"));

        let (imported, _warnings) = store.import_markdown(&exported).await.unwrap();
        assert_ne!(imported.meta.id, "mem_export_1");
        assert!(imported.meta.id.starts_with("mem_"));
        assert_eq!(store.list().await.unwrap().len(), 2);
    }

    #[tokio::test]
    async fn file_storage_and_legacy_backup_import_work() {
        let tmp = tempfile::tempdir().unwrap();
        let store = MemoryStore::new(
            Arc::new(FileMemoryStorage::new(tmp.path().join("mem"))),
            Arc::new(NoopRedactor),
        );
        let doc = make_doc("mem_legacy_1", "레거시 스타일", "설명");
        let md = serialize_memory_markdown(&doc);

        let mut legacy = HashMap::new();
        legacy.insert(format!("{MEM_DOC_PREFIX}mem_legacy_1"), md.clone());
        legacy.insert(format!("{MEM_DOC_PREFIX}mem_legacy_1_rev1"), md);

        let report = store
            .import_legacy_local_storage(&legacy, Some(&tmp.path().join("backup")))
            .await
            .unwrap();
        assert_eq!(report.imported_docs, 1);
        assert_eq!(report.imported_revisions, 1);
        assert!(report.backup_path.unwrap().exists());
        assert!(store.get("mem_legacy_1").await.unwrap().is_some());
    }
}
