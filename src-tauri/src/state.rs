use cfb::CompoundFile;
use hopclaw_types::{
    BoxFuture, DocFormat, EngineBackend, ErrorCode, ErrorModel, OpenedDocSession,
    SessionSnapshotInfo, SessionStorage, TrialStore, TrialWorkspace,
};
use std::collections::{HashMap, VecDeque};
use std::fs;
use std::future::Future;
use std::io::{Cursor, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use tempfile::NamedTempFile;
use uuid::Uuid;

pub const DEFAULT_MAX_UNDO_REVISIONS: usize = 50;
pub const DEFAULT_MAX_UNDO_TOTAL_BYTES: u64 = 512 * 1024 * 1024;
const CFB_MAGIC: [u8; 8] = [0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1];
const HWP5_FILE_HEADER_SIGNATURE: &[u8] = b"HWP Document File";

/// Detect container format (`Hwpx` vs `Hwp5`) from file bytes and optional extension hint.
pub fn detect_document_format<F>(
    bytes: &[u8],
    requested_hwpx: bool,
    hwpx_validator: F,
) -> Result<DocFormat, ErrorModel>
where
    F: FnOnce(&[u8]) -> bool,
{
    if bytes.starts_with(&[0x50, 0x4b, 0x03, 0x04]) {
        if !hwpx_validator(bytes) {
            return Err(ErrorModel::new(
                ErrorCode::E_DOC_INVALID,
                "유효한 HWPX(OWPML) 패키지가 아닙니다. 일반 ZIP 또는 손상된 문서는 열 수 없습니다.",
            ));
        }
        return Ok(DocFormat::Hwpx);
    }

    if requested_hwpx {
        return Err(ErrorModel::new(
            ErrorCode::E_DOC_INVALID,
            "HWPX 파일의 ZIP/OWPML 패키지 구조가 손상되었거나 올바르지 않습니다.",
        ));
    }

    if !bytes.starts_with(&CFB_MAGIC) {
        return Err(ErrorModel::new(
            ErrorCode::E_DOC_INVALID,
            "유효한 HWP/HWPX 문서 시그니처가 아닙니다.",
        ));
    }

    let mut comp = open_cfb_padded(bytes).map_err(|err| {
        ErrorModel::new(
            ErrorCode::E_DOC_INVALID,
            "HWP 5.0 CFB 컨테이너를 열 수 없습니다.",
        )
        .with_detail(err.to_string())
    })?;

    let mut stream = comp
        .open_stream("/FileHeader")
        .or_else(|_| comp.open_stream("FileHeader"))
        .map_err(|err| {
            ErrorModel::new(
                ErrorCode::E_DOC_INVALID,
                "HWP 5.0 FileHeader 스트림을 찾을 수 없습니다.",
            )
            .with_detail(err.to_string())
        })?;

    let mut header_buf = [0_u8; 32];
    let n = stream.read(&mut header_buf).map_err(|err| {
        ErrorModel::new(
            ErrorCode::E_DOC_INVALID,
            "HWP 5.0 FileHeader 스트림을 읽을 수 없습니다.",
        )
        .with_detail(err.to_string())
    })?;

    if n < HWP5_FILE_HEADER_SIGNATURE.len()
        || &header_buf[..HWP5_FILE_HEADER_SIGNATURE.len()] != HWP5_FILE_HEADER_SIGNATURE
    {
        return Err(ErrorModel::new(
            ErrorCode::E_DOC_INVALID,
            "유효한 HWP 5.0 문서가 아닙니다 (FileHeader 서명 불일치).",
        ));
    }

    Ok(DocFormat::Hwp5)
}

/// Open a CFB container, padding unpadded trailing FAT-sector bytes if Hancom omitted
/// unused `FREESECT` padding at EOF (`cfb 0.14.0` requires `fat.len() <= num_sectors()`).
pub fn open_cfb_padded(bytes: &[u8]) -> std::io::Result<CompoundFile<Cursor<Vec<u8>>>> {
    let mut buf = bytes.to_vec();
    if buf.len() >= 512 && buf.starts_with(&CFB_MAGIC) {
        let sector_shift = u16::from_le_bytes([buf[30], buf[31]]);
        let sector_len: usize = match sector_shift {
            12 => 4096,
            _ => 512,
        };
        let num_fat_sectors = u32::from_le_bytes([buf[44], buf[45], buf[46], buf[47]]) as usize;
        let entries_per_fat_sector = sector_len / 4;
        if let Some(fat_entries) = num_fat_sectors.checked_mul(entries_per_fat_sector) {
            if let Some(needed_len) = fat_entries
                .checked_add(1)
                .and_then(|total_sectors| total_sectors.checked_mul(sector_len))
            {
                let max_pad = sector_len * entries_per_fat_sector;
                if needed_len > buf.len() && needed_len - buf.len() <= max_pad {
                    buf.resize(needed_len, 0);
                }
            }
        }
    }
    CompoundFile::open(Cursor::new(buf))
}

/// Write `bytes` atomically to `target_path` via a sibling `NamedTempFile`, `sync_all`, and `persist`.
pub fn write_bytes_atomically(target_path: &Path, bytes: &[u8]) -> Result<(), ErrorModel> {
    let parent = target_path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent).map_err(|err| {
        ErrorModel::new(
            ErrorCode::E_DOC_INVALID,
            format!("대상 디렉터리를 생성할 수 없습니다: {}", parent.display()),
        )
        .with_detail(err.to_string())
    })?;

    let mut temp = NamedTempFile::new_in(parent).map_err(|err| {
        ErrorModel::new(ErrorCode::E_DOC_INVALID, "임시 파일을 생성할 수 없습니다.")
            .with_detail(err.to_string())
    })?;
    temp.write_all(bytes).map_err(|err| {
        ErrorModel::new(ErrorCode::E_DOC_INVALID, "임시 파일 기록에 실패했습니다.")
            .with_detail(err.to_string())
    })?;
    temp.as_file_mut().flush().map_err(|err| {
        ErrorModel::new(ErrorCode::E_DOC_INVALID, "임시 파일 flush에 실패했습니다.")
            .with_detail(err.to_string())
    })?;
    temp.as_file().sync_all().map_err(|err| {
        ErrorModel::new(ErrorCode::E_DOC_INVALID, "임시 파일 동기화에 실패했습니다.")
            .with_detail(err.to_string())
    })?;
    temp.persist(target_path).map_err(|err| {
        ErrorModel::new(
            ErrorCode::E_DOC_INVALID,
            format!("파일 저장 교체에 실패했습니다: {}", target_path.display()),
        )
        .with_detail(err.error.to_string())
    })?;

    if let Ok(dir) = fs::File::open(parent) {
        let _ = dir.sync_all();
    }
    Ok(())
}

#[derive(Debug, Clone)]
struct RevisionSnapshot {
    revision: u64,
    snapshot_path: PathBuf,
    byte_len: u64,
}

/// Disk-backed session state holding revision history (`rev_<n>`) and active staging file.
#[derive(Debug)]
pub struct DocumentSession {
    pub session_id: String,
    pub original_path: String,
    pub file_name: String,
    pub format: DocFormat,
    pub session_dir: PathBuf,
    pub staging_path: PathBuf,
    history: VecDeque<RevisionSnapshot>,
    cursor: usize,
    next_revision: u64,
    total_snapshot_bytes: u64,
}

impl DocumentSession {
    fn current_snapshot(&self) -> Option<&RevisionSnapshot> {
        self.history.get(self.cursor)
    }

    pub fn current_revision(&self) -> u64 {
        self.current_snapshot().map(|s| s.revision).unwrap_or(0)
    }

    pub fn current_byte_len(&self) -> u64 {
        self.current_snapshot().map(|s| s.byte_len).unwrap_or(0)
    }

    pub fn snapshot_count(&self) -> usize {
        self.history.len()
    }

    pub fn total_snapshot_bytes(&self) -> u64 {
        self.total_snapshot_bytes
    }

    pub fn can_undo(&self) -> bool {
        self.cursor > 0
    }

    pub fn can_redo(&self) -> bool {
        self.cursor + 1 < self.history.len()
    }

    pub fn snapshot_info(&self) -> SessionSnapshotInfo {
        SessionSnapshotInfo {
            session_id: self.session_id.clone(),
            revision: self.current_revision(),
            format: self.format,
            staging_path: self.staging_path.clone(),
            byte_len: self.current_byte_len(),
        }
    }

    fn snapshot_file_path(&self, rev: u64) -> PathBuf {
        self.session_dir.join(format!("rev_{rev}"))
    }

    fn push_revision(
        &mut self,
        bytes: &[u8],
        max_revisions: usize,
        max_total_bytes: u64,
    ) -> Result<SessionSnapshotInfo, ErrorModel> {
        // Drop any undone redo tail before committing a diverging revision.
        while self.history.len() > self.cursor + 1 {
            if let Some(removed) = self.history.pop_back() {
                self.total_snapshot_bytes =
                    self.total_snapshot_bytes.saturating_sub(removed.byte_len);
                let _ = fs::remove_file(&removed.snapshot_path);
            }
        }

        let rev = self.next_revision;
        self.next_revision += 1;

        let snap_path = self.snapshot_file_path(rev);
        write_bytes_atomically(&snap_path, bytes)?;
        write_bytes_atomically(&self.staging_path, bytes)?;

        let byte_len = bytes.len() as u64;
        self.history.push_back(RevisionSnapshot {
            revision: rev,
            snapshot_path: snap_path,
            byte_len,
        });
        self.cursor = self.history.len() - 1;
        self.total_snapshot_bytes += byte_len;

        self.enforce_caps(max_revisions, max_total_bytes);
        Ok(self.snapshot_info())
    }

    fn enforce_caps(&mut self, max_revisions: usize, max_total_bytes: u64) {
        let min_revisions = max_revisions.max(1);
        while self.cursor > 0
            && (self.history.len() > min_revisions || self.total_snapshot_bytes > max_total_bytes)
        {
            if let Some(oldest) = self.history.pop_front() {
                self.cursor -= 1;
                self.total_snapshot_bytes =
                    self.total_snapshot_bytes.saturating_sub(oldest.byte_len);
                let _ = fs::remove_file(&oldest.snapshot_path);
            }
        }
    }
}

/// Manages active [`DocumentSession`] instances and disk-backed Undo/Redo snapshots.
#[derive(Debug)]
pub struct SessionManager {
    staging_root: PathBuf,
    max_revisions: usize,
    max_total_bytes: u64,
    sessions: Mutex<HashMap<String, DocumentSession>>,
    path_to_session: Mutex<HashMap<PathBuf, String>>,
}

impl SessionManager {
    pub fn new(staging_root: PathBuf) -> Self {
        Self::with_limits(
            staging_root,
            DEFAULT_MAX_UNDO_REVISIONS,
            DEFAULT_MAX_UNDO_TOTAL_BYTES,
        )
    }

    pub fn with_limits(staging_root: PathBuf, max_revisions: usize, max_total_bytes: u64) -> Self {
        Self {
            staging_root,
            max_revisions: max_revisions.max(1),
            max_total_bytes,
            sessions: Mutex::new(HashMap::new()),
            path_to_session: Mutex::new(HashMap::new()),
        }
    }

    pub fn staging_root(&self) -> &Path {
        &self.staging_root
    }

    /// Open a session from verified document bytes, creating `rev_0` and staging copy.
    pub fn open_from_bytes(
        &self,
        original_path: String,
        file_name: String,
        format: DocFormat,
        bytes: Vec<u8>,
    ) -> Result<OpenedDocSession, ErrorModel> {
        let session_id = Uuid::new_v4().to_string();
        let is_hwpx = format == DocFormat::Hwpx;
        let ext = if is_hwpx { "hwpx" } else { "hwp" };

        let session_dir = self.staging_root.join(&session_id);
        fs::create_dir_all(&session_dir).map_err(|err| {
            ErrorModel::new(
                ErrorCode::E_DOC_INVALID,
                "세션 스테이징 디렉터리를 생성할 수 없습니다.",
            )
            .with_detail(err.to_string())
        })?;

        let staging_path = session_dir.join(format!("staging_{session_id}.{ext}"));
        let rev0_path = session_dir.join("rev_0");
        write_bytes_atomically(&rev0_path, &bytes)?;
        write_bytes_atomically(&staging_path, &bytes)?;

        let byte_len = bytes.len() as u64;
        let mut history = VecDeque::new();
        history.push_back(RevisionSnapshot {
            revision: 0,
            snapshot_path: rev0_path,
            byte_len,
        });

        let session = DocumentSession {
            session_id: session_id.clone(),
            original_path: original_path.clone(),
            file_name: file_name.clone(),
            format,
            session_dir,
            staging_path: staging_path.clone(),
            history,
            cursor: 0,
            next_revision: 1,
            total_snapshot_bytes: byte_len,
        };

        self.register_session(session)?;

        Ok(OpenedDocSession {
            original_path,
            staging_path: staging_path.to_string_lossy().into_owned(),
            file_name,
            is_hwpx,
            bytes,
            session_id: Some(session_id),
            revision: Some(0),
        })
    }

    /// Create an empty/blank session slot (retaining `bytes: []` until populated or templated).
    pub fn create_blank(&self, is_hwpx: bool) -> Result<OpenedDocSession, ErrorModel> {
        let session_id = Uuid::new_v4().to_string();
        let format = if is_hwpx {
            DocFormat::Hwpx
        } else {
            DocFormat::Hwp5
        };
        let ext = if is_hwpx { "hwpx" } else { "hwp" };

        let session_dir = self.staging_root.join(&session_id);
        fs::create_dir_all(&session_dir).map_err(|err| {
            ErrorModel::new(
                ErrorCode::E_DOC_INVALID,
                "빈 문서 스테이징 디렉터리를 생성할 수 없습니다.",
            )
            .with_detail(err.to_string())
        })?;

        let staging_path = session_dir.join(format!("new_{session_id}.{ext}"));
        let file_name = format!("새 문서.{ext}");

        let session = DocumentSession {
            session_id: session_id.clone(),
            original_path: String::new(),
            file_name: file_name.clone(),
            format,
            session_dir,
            staging_path: staging_path.clone(),
            history: VecDeque::new(),
            cursor: 0,
            next_revision: 0,
            total_snapshot_bytes: 0,
        };

        self.register_session(session)?;

        Ok(OpenedDocSession {
            original_path: String::new(),
            staging_path: staging_path.to_string_lossy().into_owned(),
            file_name,
            is_hwpx,
            bytes: Vec::new(),
            session_id: Some(session_id),
            revision: Some(0),
        })
    }

    fn register_session(&self, session: DocumentSession) -> Result<(), ErrorModel> {
        let path_key = Self::normalize_lookup_path(&session.staging_path);
        let id = session.session_id.clone();
        self.sessions
            .lock()
            .map_err(|_| ErrorModel::new(ErrorCode::E_INTERNAL_PANIC, "세션 잠금 오류"))?
            .insert(id.clone(), session);
        self.path_to_session
            .lock()
            .map_err(|_| ErrorModel::new(ErrorCode::E_INTERNAL_PANIC, "세션 경로 맵 잠금 오류"))?
            .insert(path_key, id);
        Ok(())
    }

    fn normalize_lookup_path(path: &Path) -> PathBuf {
        fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
    }

    /// Look up session by its staging path, and if tracked, commit a new disk revision.
    pub fn record_write_if_tracked(
        &self,
        path: &Path,
        new_bytes: &[u8],
    ) -> Result<Option<SessionSnapshotInfo>, ErrorModel> {
        let session_id = {
            let map = self
                .path_to_session
                .lock()
                .map_err(|_| ErrorModel::new(ErrorCode::E_INTERNAL_PANIC, "세션 경로 맵 잠금 오류"))?;
            let direct = map.get(path).cloned();
            direct.or_else(|| map.get(&Self::normalize_lookup_path(path)).cloned())
        };

        let Some(session_id) = session_id else {
            return Ok(None);
        };

        let info = self.commit_revision_sync(&session_id, None, new_bytes)?;
        Ok(Some(info))
    }

    pub fn snapshot_info_sync(&self, session_id: &str) -> Result<SessionSnapshotInfo, ErrorModel> {
        let sessions = self
            .sessions
            .lock()
            .map_err(|_| ErrorModel::new(ErrorCode::E_INTERNAL_PANIC, "세션 잠금 오류"))?;
        let sess = sessions.get(session_id).ok_or_else(|| {
            ErrorModel::new(
                ErrorCode::E_DOC_INVALID,
                format!("세션을 찾을 수 없습니다: {session_id}"),
            )
        })?;
        Ok(sess.snapshot_info())
    }

    pub fn read_staging_bytes_sync(&self, session_id: &str) -> Result<Vec<u8>, ErrorModel> {
        let staging_path = self.snapshot_info_sync(session_id)?.staging_path;
        fs::read(&staging_path).map_err(|err| {
            ErrorModel::new(
                ErrorCode::E_DOC_INVALID,
                format!("스테이징 파일을 읽을 수 없습니다: {}", staging_path.display()),
            )
            .with_detail(err.to_string())
        })
    }

    pub fn commit_revision_sync(
        &self,
        session_id: &str,
        expected_revision: Option<u64>,
        new_bytes: &[u8],
    ) -> Result<SessionSnapshotInfo, ErrorModel> {
        let mut sessions = self
            .sessions
            .lock()
            .map_err(|_| ErrorModel::new(ErrorCode::E_INTERNAL_PANIC, "세션 잠금 오류"))?;
        let sess = sessions.get_mut(session_id).ok_or_else(|| {
            ErrorModel::new(
                ErrorCode::E_DOC_INVALID,
                format!("세션을 찾을 수 없습니다: {session_id}"),
            )
        })?;

        if let Some(expected) = expected_revision {
            let actual = sess.current_revision();
            if actual != expected {
                return Err(ErrorModel::new(
                    ErrorCode::E_STALE_REVISION,
                    format!(
                        "문서 리비전이 변경되었습니다 (기대 리비전: {expected}, 현재 리비전: {actual}). 재검증이 필요합니다."
                    ),
                ));
            }
        }

        sess.push_revision(new_bytes, self.max_revisions, self.max_total_bytes)
    }

    pub fn undo_sync(&self, session_id: &str) -> Result<SessionSnapshotInfo, ErrorModel> {
        let mut sessions = self
            .sessions
            .lock()
            .map_err(|_| ErrorModel::new(ErrorCode::E_INTERNAL_PANIC, "세션 잠금 오류"))?;
        let sess = sessions.get_mut(session_id).ok_or_else(|| {
            ErrorModel::new(
                ErrorCode::E_DOC_INVALID,
                format!("세션을 찾을 수 없습니다: {session_id}"),
            )
        })?;

        if !sess.can_undo() {
            return Err(ErrorModel::new(
                ErrorCode::E_DOC_INVALID,
                "되돌릴 이전 리비전이 없습니다.",
            ));
        }

        let target_idx = sess.cursor - 1;
        let target_snap = sess.history[target_idx].snapshot_path.clone();
        let bytes = fs::read(&target_snap).map_err(|err| {
            ErrorModel::new(
                ErrorCode::E_DOC_INVALID,
                format!("Undo 스냅샷을 읽을 수 없습니다: {}", target_snap.display()),
            )
            .with_detail(err.to_string())
        })?;
        write_bytes_atomically(&sess.staging_path, &bytes)?;
        sess.cursor = target_idx;
        Ok(sess.snapshot_info())
    }

    pub fn redo_sync(&self, session_id: &str) -> Result<SessionSnapshotInfo, ErrorModel> {
        let mut sessions = self
            .sessions
            .lock()
            .map_err(|_| ErrorModel::new(ErrorCode::E_INTERNAL_PANIC, "세션 잠금 오류"))?;
        let sess = sessions.get_mut(session_id).ok_or_else(|| {
            ErrorModel::new(
                ErrorCode::E_DOC_INVALID,
                format!("세션을 찾을 수 없습니다: {session_id}"),
            )
        })?;

        if !sess.can_redo() {
            return Err(ErrorModel::new(
                ErrorCode::E_DOC_INVALID,
                "다시 실행할 다음 리비전이 없습니다.",
            ));
        }

        let target_idx = sess.cursor + 1;
        let target_snap = sess.history[target_idx].snapshot_path.clone();
        let bytes = fs::read(&target_snap).map_err(|err| {
            ErrorModel::new(
                ErrorCode::E_DOC_INVALID,
                format!("Redo 스냅샷을 읽을 수 없습니다: {}", target_snap.display()),
            )
            .with_detail(err.to_string())
        })?;
        write_bytes_atomically(&sess.staging_path, &bytes)?;
        sess.cursor = target_idx;
        Ok(sess.snapshot_info())
    }

    /// Inspect snapshot count & total retained bytes for diagnostics/tests.
    pub fn session_metrics(&self, session_id: &str) -> Option<(usize, u64, bool, bool)> {
        self.sessions.lock().ok().and_then(|map| {
            map.get(session_id).map(|s| {
                (
                    s.snapshot_count(),
                    s.total_snapshot_bytes(),
                    s.can_undo(),
                    s.can_redo(),
                )
            })
        })
    }
}

impl SessionStorage for SessionManager {
    fn snapshot_info<'a>(
        &'a self,
        session_id: &'a str,
    ) -> BoxFuture<'a, Result<SessionSnapshotInfo, ErrorModel>> {
        Box::pin(async move { self.snapshot_info_sync(session_id) })
    }

    fn read_staging_bytes<'a>(
        &'a self,
        session_id: &'a str,
    ) -> BoxFuture<'a, Result<Vec<u8>, ErrorModel>> {
        Box::pin(async move { self.read_staging_bytes_sync(session_id) })
    }

    fn commit_revision<'a>(
        &'a self,
        session_id: &'a str,
        expected_revision: Option<u64>,
        new_bytes: &'a [u8],
    ) -> BoxFuture<'a, Result<SessionSnapshotInfo, ErrorModel>> {
        Box::pin(async move { self.commit_revision_sync(session_id, expected_revision, new_bytes) })
    }

    fn undo<'a>(
        &'a self,
        session_id: &'a str,
    ) -> BoxFuture<'a, Result<SessionSnapshotInfo, ErrorModel>> {
        Box::pin(async move { self.undo_sync(session_id) })
    }

    fn redo<'a>(
        &'a self,
        session_id: &'a str,
    ) -> BoxFuture<'a, Result<SessionSnapshotInfo, ErrorModel>> {
        Box::pin(async move { self.redo_sync(session_id) })
    }
}

/// Filesystem-backed isolated trial store for `hopclaw-ai` trial edits & auto-verification.
#[derive(Debug)]
pub struct FsTrialStore {
    trials_root: PathBuf,
    counter: AtomicU64,
    tracked: Mutex<HashMap<String, Vec<PathBuf>>>,
}

impl FsTrialStore {
    pub fn new(trials_root: PathBuf) -> Self {
        Self {
            trials_root,
            counter: AtomicU64::new(1),
            tracked: Mutex::new(HashMap::new()),
        }
    }

    fn sanitize_id(raw: &str) -> String {
        let cleaned: String = raw
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
            .collect();
        if cleaned.is_empty() {
            "default".to_string()
        } else {
            cleaned
        }
    }
}

impl TrialStore for FsTrialStore {
    fn prepare_trial<'a>(
        &'a self,
        request_id: &'a str,
        format: DocFormat,
        base_bytes: &'a [u8],
        base_revision: u64,
    ) -> BoxFuture<'a, Result<TrialWorkspace, ErrorModel>> {
        Box::pin(async move {
            let safe_req = Self::sanitize_id(request_id);
            let seq = self.counter.fetch_add(1, Ordering::Relaxed);
            let ext = match format {
                DocFormat::Hwpx => "hwpx",
                DocFormat::Hwp5 => "hwp",
            };
            let req_dir = self.trials_root.join(&safe_req);
            fs::create_dir_all(&req_dir).map_err(|err| {
                ErrorModel::new(
                    ErrorCode::E_DOC_INVALID,
                    "trial 작업 디렉터리를 생성할 수 없습니다.",
                )
                .with_detail(err.to_string())
            })?;

            let trial_id = format!("trial_{safe_req}_{seq}");
            let path = req_dir.join(format!("{trial_id}.{ext}"));
            write_bytes_atomically(&path, base_bytes)?;

            if let Ok(mut guard) = self.tracked.lock() {
                guard.entry(safe_req).or_default().push(path.clone());
            }

            Ok(TrialWorkspace {
                trial_id,
                path,
                format,
                base_revision,
            })
        })
    }

    fn read_trial_bytes<'a>(
        &'a self,
        trial: &'a TrialWorkspace,
    ) -> BoxFuture<'a, Result<Vec<u8>, ErrorModel>> {
        Box::pin(async move {
            fs::read(&trial.path).map_err(|err| {
                ErrorModel::new(
                    ErrorCode::E_DOC_INVALID,
                    format!("trial 파일을 읽을 수 없습니다: {}", trial.path.display()),
                )
                .with_detail(err.to_string())
            })
        })
    }

    fn write_trial_bytes<'a>(
        &'a self,
        trial: &'a TrialWorkspace,
        bytes: &'a [u8],
    ) -> BoxFuture<'a, Result<(), ErrorModel>> {
        Box::pin(async move { write_bytes_atomically(&trial.path, bytes) })
    }

    fn cleanup_trial<'a>(
        &'a self,
        trial: &'a TrialWorkspace,
    ) -> BoxFuture<'a, Result<(), ErrorModel>> {
        Box::pin(async move {
            if trial.path.exists() {
                fs::remove_file(&trial.path).map_err(|err| {
                    ErrorModel::new(
                        ErrorCode::E_DOC_INVALID,
                        format!("trial 파일을 삭제할 수 없습니다: {}", trial.path.display()),
                    )
                    .with_detail(err.to_string())
                })?;
            }
            Ok(())
        })
    }

    fn cleanup_request_trials<'a>(
        &'a self,
        request_id: &'a str,
    ) -> BoxFuture<'a, Result<(), ErrorModel>> {
        Box::pin(async move {
            let safe_req = Self::sanitize_id(request_id);
            if let Ok(mut guard) = self.tracked.lock() {
                if let Some(paths) = guard.remove(&safe_req) {
                    for p in paths {
                        let _ = fs::remove_file(p);
                    }
                }
            }
            let req_dir = self.trials_root.join(&safe_req);
            if req_dir.is_dir() {
                let _ = fs::remove_dir_all(&req_dir);
            }
            Ok(())
        })
    }
}

/// Parse `HOP_ENGINE_BACKEND` (`node` | `rust` | `shadow`, case-insensitive).
pub fn parse_engine_backend(raw: &str) -> Option<EngineBackend> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "node" => Some(EngineBackend::Node),
        "rust" => Some(EngineBackend::Rust),
        "shadow" => Some(EngineBackend::Shadow),
        _ => None,
    }
}

/// Strangler-pattern router hook used by command bridges (`claw_bridge.rs`) to dispatch
/// between Node sidecar, pure Rust crates, or Shadow differential comparison.
pub async fn route_by_backend<T, FN, FR, FutN, FutR, FC>(
    backend: EngineBackend,
    op_name: &str,
    node_fn: FN,
    rust_fn: FR,
    compare_fn: FC,
) -> Result<T, ErrorModel>
where
    FN: FnOnce() -> FutN,
    FR: FnOnce() -> FutR,
    FutN: Future<Output = Result<T, ErrorModel>>,
    FutR: Future<Output = Result<T, ErrorModel>>,
    FC: FnOnce(&T, &T) -> Option<String>,
{
    match backend {
        EngineBackend::Node => node_fn().await,
        EngineBackend::Rust => rust_fn().await,
        EngineBackend::Shadow => {
            let (node_res, rust_res) = tokio::join!(node_fn(), rust_fn());
            let node_val = node_res?;
            match rust_res {
                Ok(rust_val) => {
                    if let Some(diff) = compare_fn(&node_val, &rust_val) {
                        eprintln!("[shadow:{op_name}] parity mismatch: {diff}");
                    }
                }
                Err(err) => {
                    eprintln!("[shadow:{op_name}] rust candidate error: {err}");
                }
            }
            Ok(node_val)
        }
    }
}

/// Global Tauri application state holding session manager, trial store, engine routing flag,
/// and active LLM request cancellation tokens.
pub struct AppState {
    pub active_llm_cancels: Arc<Mutex<HashMap<String, tokio::sync::oneshot::Sender<()>>>>,
    pub sessions: Arc<SessionManager>,
    pub trials: Arc<FsTrialStore>,
    engine_backend: Arc<RwLock<EngineBackend>>,
}

impl AppState {
    pub fn new() -> Self {
        let base_root = std::env::temp_dir().join("hop-claw-staging");
        Self::with_staging_root(base_root)
    }

    pub fn with_staging_root(staging_root: PathBuf) -> Self {
        let initial_backend = std::env::var("HOP_ENGINE_BACKEND")
            .or_else(|_| std::env::var("HOPCLAW_ENGINE_BACKEND"))
            .ok()
            .and_then(|v| parse_engine_backend(&v))
            .unwrap_or(EngineBackend::Node);

        Self {
            active_llm_cancels: Arc::new(Mutex::new(HashMap::new())),
            sessions: Arc::new(SessionManager::new(staging_root.clone())),
            trials: Arc::new(FsTrialStore::new(staging_root.join("trials"))),
            engine_backend: Arc::new(RwLock::new(initial_backend)),
        }
    }

    pub fn engine_backend(&self) -> EngineBackend {
        self.engine_backend
            .read()
            .map(|guard| *guard)
            .unwrap_or(EngineBackend::Node)
    }

    pub fn set_engine_backend(&self, backend: EngineBackend) {
        if let Ok(mut guard) = self.engine_backend.write() {
            *guard = backend;
        }
    }
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hwpx_package::is_valid_hwpx_package;

    fn build_minimal_hwp5_cfb() -> Vec<u8> {
        let cursor = Cursor::new(Vec::new());
        let mut comp = CompoundFile::create(cursor).unwrap();
        let mut stream = comp.create_stream("/FileHeader").unwrap();
        let mut header = [0_u8; 256];
        header[..HWP5_FILE_HEADER_SIGNATURE.len()].copy_from_slice(HWP5_FILE_HEADER_SIGNATURE);
        stream.write_all(&header).unwrap();
        drop(stream);
        comp.into_inner().into_inner()
    }

    #[test]
    fn format_detection_distinguishes_hwpx_hwp5_and_invalid_containers() {
        let sample_hwpx = std::fs::read(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../samples/demo_official_report.hwpx"),
        )
        .unwrap();
        assert_eq!(
            detect_document_format(&sample_hwpx, true, is_valid_hwpx_package).unwrap(),
            DocFormat::Hwpx
        );

        let sample_hwp = std::fs::read(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../samples/demo_official_report.hwp"),
        )
        .unwrap();
        assert_eq!(
            detect_document_format(&sample_hwp, false, is_valid_hwpx_package).unwrap(),
            DocFormat::Hwp5
        );

        let sample_hwp2 = std::fs::read(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../samples/(날짜)(교시)(과목명)교수.학습과정안(이름).hwp"),
        )
        .unwrap();
        assert_eq!(
            detect_document_format(&sample_hwp2, false, is_valid_hwpx_package).unwrap(),
            DocFormat::Hwp5
        );

        let synthetic_hwp = build_minimal_hwp5_cfb();
        assert_eq!(
            detect_document_format(&synthetic_hwp, false, is_valid_hwpx_package).unwrap(),
            DocFormat::Hwp5
        );

        // Non-HWPX ZIP (xlsx) must fail with E_DOC_INVALID
        let xlsx = std::fs::read(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../samples/demo_budget_ref.xlsx"),
        )
        .unwrap();
        let err = detect_document_format(&xlsx, false, is_valid_hwpx_package).unwrap_err();
        assert_eq!(err.code, ErrorCode::E_DOC_INVALID);

        // CFB missing FileHeader must fail with E_DOC_INVALID
        let cursor = Cursor::new(Vec::new());
        let mut bad_cfb = CompoundFile::create(cursor).unwrap();
        bad_cfb.create_stream("/Other").unwrap();
        let bad_cfb_bytes = bad_cfb.into_inner().into_inner();
        assert_eq!(
            detect_document_format(&bad_cfb_bytes, false, is_valid_hwpx_package)
                .unwrap_err()
                .code,
            ErrorCode::E_DOC_INVALID
        );

        // Random bytes fail with E_DOC_INVALID
        assert_eq!(
            detect_document_format(b"not-a-document", false, is_valid_hwpx_package)
                .unwrap_err()
                .code,
            ErrorCode::E_DOC_INVALID
        );
    }

    #[test]
    fn document_session_tracks_revisions_undo_redo_and_stale_guard() {
        let dir = tempfile::tempdir().unwrap();
        let mgr = SessionManager::new(dir.path().to_path_buf());
        let opened = mgr
            .open_from_bytes(
                "orig.hwp".into(),
                "orig.hwp".into(),
                DocFormat::Hwp5,
                b"rev0".to_vec(),
            )
            .unwrap();
        let sid = opened.session_id.unwrap();
        assert_eq!(opened.revision, Some(0));

        let s1 = mgr.commit_revision_sync(&sid, Some(0), b"rev1").unwrap();
        assert_eq!(s1.revision, 1);
        let s2 = mgr.commit_revision_sync(&sid, Some(1), b"rev2").unwrap();
        assert_eq!(s2.revision, 2);

        // Stale revision check
        let stale = mgr.commit_revision_sync(&sid, Some(1), b"stale").unwrap_err();
        assert_eq!(stale.code, ErrorCode::E_STALE_REVISION);

        // Undo to rev1, then rev0
        let u1 = mgr.undo_sync(&sid).unwrap();
        assert_eq!(u1.revision, 1);
        assert_eq!(mgr.read_staging_bytes_sync(&sid).unwrap(), b"rev1");

        // Redo back to rev2
        let r2 = mgr.redo_sync(&sid).unwrap();
        assert_eq!(r2.revision, 2);
        assert_eq!(mgr.read_staging_bytes_sync(&sid).unwrap(), b"rev2");

        // Undo to rev1 then commit diverging rev3 -> rev2 redo tail cleared
        mgr.undo_sync(&sid).unwrap();
        let s3 = mgr.commit_revision_sync(&sid, Some(1), b"rev3").unwrap();
        assert_eq!(s3.revision, 3);
        let (_, _, can_undo, can_redo) = mgr.session_metrics(&sid).unwrap();
        assert!(can_undo);
        assert!(!can_redo);
        assert_eq!(mgr.read_staging_bytes_sync(&sid).unwrap(), b"rev3");
    }

    #[test]
    fn document_session_evicts_oldest_snapshots_by_count_and_byte_caps() {
        let dir = tempfile::tempdir().unwrap();
        let mgr_count = SessionManager::with_limits(dir.path().join("by_count"), 3, 1024 * 1024);
        let opened = mgr_count
            .open_from_bytes("a.hwp".into(), "a.hwp".into(), DocFormat::Hwp5, b"r0".to_vec())
            .unwrap();
        let sid = opened.session_id.unwrap();
        for i in 1..=5 {
            mgr_count
                .commit_revision_sync(&sid, None, format!("r{i}").as_bytes())
                .unwrap();
        }
        let (count, _, _, _) = mgr_count.session_metrics(&sid).unwrap();
        assert_eq!(count, 3);
        assert!(!dir.path().join("by_count").join(&sid).join("rev_0").exists());
        assert!(!dir.path().join("by_count").join(&sid).join("rev_2").exists());
        assert!(dir.path().join("by_count").join(&sid).join("rev_5").exists());

        // Eviction by total byte cap (cap = 25 bytes, each rev = 10 bytes => max 2 kept)
        let mgr_bytes = SessionManager::with_limits(dir.path().join("by_bytes"), 50, 25);
        let opened_b = mgr_bytes
            .open_from_bytes(
                "b.hwp".into(),
                "b.hwp".into(),
                DocFormat::Hwp5,
                vec![b'0'; 10],
            )
            .unwrap();
        let sid_b = opened_b.session_id.unwrap();
        for i in 1..=4_u8 {
            mgr_bytes
                .commit_revision_sync(&sid_b, None, &[b'0' + i; 10])
                .unwrap();
        }
        let (count_b, total_b, _, _) = mgr_bytes.session_metrics(&sid_b).unwrap();
        assert!(total_b <= 25);
        assert_eq!(count_b, 2);
    }

    #[tokio::test]
    async fn trial_store_isolates_and_cleans_up_trials() {
        let dir = tempfile::tempdir().unwrap();
        let store = FsTrialStore::new(dir.path().join("trials"));
        let ws = store
            .prepare_trial("req-1/abc", DocFormat::Hwpx, b"base-bytes", 2)
            .await
            .unwrap();
        assert!(ws.path.exists());
        assert_eq!(store.read_trial_bytes(&ws).await.unwrap(), b"base-bytes");
        store.write_trial_bytes(&ws, b"mutated-trial").await.unwrap();
        assert_eq!(store.read_trial_bytes(&ws).await.unwrap(), b"mutated-trial");
        store.cleanup_request_trials("req-1/abc").await.unwrap();
        assert!(!ws.path.exists());
    }

    #[tokio::test]
    async fn strangler_router_dispatches_node_rust_and_shadow() {
        let n = route_by_backend(
            EngineBackend::Node,
            "inspect",
            || async { Ok("node") },
            || async { Ok("rust") },
            |_, _| None,
        )
        .await
        .unwrap();
        assert_eq!(n, "node");

        let r = route_by_backend(
            EngineBackend::Rust,
            "inspect",
            || async { Ok("node") },
            || async { Ok("rust") },
            |_, _| None,
        )
        .await
        .unwrap();
        assert_eq!(r, "rust");

        let compared = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let compared_clone = compared.clone();
        let s = route_by_backend(
            EngineBackend::Shadow,
            "inspect",
            || async { Ok("node") },
            || async { Ok("rust") },
            move |a, b| {
                compared_clone.store(true, Ordering::SeqCst);
                if a != b {
                    Some(format!("{a} != {b}"))
                } else {
                    None
                }
            },
        )
        .await
        .unwrap();
        assert_eq!(s, "node");
        assert!(compared.load(Ordering::SeqCst));
    }
}
