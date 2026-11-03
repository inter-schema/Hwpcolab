use fontdb::{self, Source};
use serde::Serialize;
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LocalFontEntry {
    pub family: String,
    pub post_script_name: String,
    pub style: String,
    pub weight: u16,
    pub source_kind: String,
    pub path: Option<String>,
}

/// User-level font directories not guaranteed to be included by fontdb's
/// platform system-font discovery.
pub fn desktop_extra_font_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();

    #[cfg(target_os = "macos")]
    if let Some(home) = env_path("HOME") {
        dirs.push(home.join("Library/Fonts"));
    }

    #[cfg(target_os = "linux")]
    {
        if let Some(home) = env_path("HOME") {
            dirs.push(home.join(".local/share/fonts"));
            dirs.push(home.join(".fonts"));
        }
        let wsl_windows_fonts = PathBuf::from("/mnt/c/Windows/Fonts");
        if wsl_windows_fonts.is_dir() {
            dirs.push(wsl_windows_fonts);
        }
    }

    #[cfg(windows)]
    if let Some(local_app_data) = env_path("LOCALAPPDATA") {
        dirs.extend(windows_user_font_dirs(&local_app_data));
    }

    dedupe_existing_dirs(dirs)
}

pub fn collect_desktop_local_font_entries() -> Vec<LocalFontEntry> {
    let extra_dirs = desktop_extra_font_dirs();
    let mut database = fontdb::Database::new();
    database.load_system_fonts();

    for directory in &extra_dirs {
        if directory.is_dir() {
            database.load_fonts_dir(directory);
        }
    }

    let mut seen = BTreeSet::new();
    let mut entries = Vec::new();

    for face in database.faces() {
        let path = source_path(&face.source);
        let source_kind = classify_source(path.as_deref(), &extra_dirs);
        let style = style_name(face.style);
        let mut families = BTreeSet::new();

        for (family, _) in &face.families {
            let family = family.trim();
            if !family.is_empty() {
                families.insert(family.to_owned());
            }
        }

        for family in families {
            let key = (
                family.clone(),
                face.post_script_name.clone(),
                style,
                face.weight.0,
                source_kind,
                path.clone(),
            );
            if !seen.insert(key) {
                continue;
            }

            entries.push(LocalFontEntry {
                family,
                post_script_name: face.post_script_name.clone(),
                style: style.to_owned(),
                weight: face.weight.0,
                source_kind: source_kind.to_owned(),
                path: path.clone(),
            });
        }
    }

    entries.sort_by(|left, right| {
        left.family
            .cmp(&right.family)
            .then(left.weight.cmp(&right.weight))
            .then(left.style.cmp(&right.style))
            .then(left.post_script_name.cmp(&right.post_script_name))
    });
    entries
}

/// Read only font files below approved, canonical user-font roots. Canonical
/// paths prevent traversal and symlink escapes from bypassing the allow-list.
pub fn read_desktop_local_font(requested_path: &Path) -> Result<Vec<u8>, String> {
    let canonical_path = fs::canonicalize(requested_path).map_err(|error| {
        format!(
            "로컬 폰트 파일을 찾을 수 없습니다: {} ({})",
            requested_path.display(),
            error
        )
    })?;

    if !canonical_path.is_file() {
        return Err(format!(
            "로컬 폰트 경로가 일반 파일이 아닙니다: {}",
            canonical_path.display()
        ));
    }

    if !has_supported_font_extension(&canonical_path) {
        return Err(format!(
            "지원하지 않는 로컬 폰트 확장자입니다: {}",
            canonical_path.display()
        ));
    }

    let allowed_roots = desktop_extra_font_dirs();
    if !allowed_roots
        .iter()
        .any(|root| path_is_within_root(&canonical_path, root))
    {
        return Err(format!(
            "허용된 사용자 폰트 디렉터리 밖의 파일입니다: {}",
            canonical_path.display()
        ));
    }

    fs::read(&canonical_path).map_err(|error| {
        format!(
            "로컬 폰트 파일을 읽을 수 없습니다: {} ({})",
            canonical_path.display(),
            error
        )
    })
}

fn source_path(source: &Source) -> Option<String> {
    match source {
        Source::File(path) | Source::SharedFile(path, _) => {
            Some(path.to_string_lossy().into_owned())
        }
        Source::Binary(_) => None,
    }
}

fn classify_source(path: Option<&str>, extra_font_dirs: &[PathBuf]) -> &'static str {
    let Some(path) = path else {
        return "system-installed";
    };
    let candidate = normalize_existing_path(Path::new(path))
        .unwrap_or_else(|| PathBuf::from(path));
    if extra_font_dirs.iter().any(|root| path_is_within_root(&candidate, root)) {
        "file-backed"
    } else {
        "system-installed"
    }
}

fn style_name(style: fontdb::Style) -> &'static str {
    match style {
        fontdb::Style::Normal => "normal",
        fontdb::Style::Italic => "italic",
        fontdb::Style::Oblique => "oblique",
    }
}

fn env_path(name: &str) -> Option<PathBuf> {
    std::env::var_os(name).map(PathBuf::from)
}

#[cfg(any(windows, test))]
fn windows_user_font_dirs(local_app_data: &Path) -> Vec<PathBuf> {
    vec![local_app_data.join("Microsoft/Windows/Fonts")]
}

fn dedupe_existing_dirs(dirs: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut seen = BTreeSet::new();
    let mut result = Vec::new();
    for directory in dirs {
        let Some(canonical) = normalize_existing_path(&directory) else {
            continue;
        };
        if canonical.is_dir() && seen.insert(canonical.clone()) {
            result.push(canonical);
        }
    }
    result
}

fn normalize_existing_path(path: &Path) -> Option<PathBuf> {
    fs::canonicalize(path).ok()
}

fn path_is_within_root(path: &Path, root: &Path) -> bool {
    let Some(canonical_path) = normalize_existing_path(path) else {
        return false;
    };
    let Some(canonical_root) = normalize_existing_path(root) else {
        return false;
    };
    canonical_path.starts_with(&canonical_root)
}

fn has_supported_font_extension(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| {
            matches!(
                extension.to_ascii_lowercase().as_str(),
                "ttf" | "otf" | "ttc" | "otc" | "woff" | "woff2"
            )
        })
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_path_allow_list_rejects_sibling_prefix_directories() {
        let temp = tempfile::tempdir().unwrap();
        let fonts_root = temp.path().join("fonts");
        let sibling_root = temp.path().join("fonts-private");
        fs::create_dir_all(&fonts_root).unwrap();
        fs::create_dir_all(&sibling_root).unwrap();
        let allowed = fonts_root.join("sample.ttf");
        let sibling = sibling_root.join("sample.ttf");
        fs::write(&allowed, b"font").unwrap();
        fs::write(&sibling, b"font").unwrap();

        assert!(path_is_within_root(&allowed, &fonts_root));
        assert!(!path_is_within_root(&sibling, &fonts_root));
    }

    #[test]
    fn font_extension_allow_list_is_case_insensitive_and_rejects_other_files() {
        assert!(has_supported_font_extension(Path::new("sample.TTF")));
        assert!(has_supported_font_extension(Path::new("sample.woff2")));
        assert!(!has_supported_font_extension(Path::new("readme.txt")));
        assert!(!has_supported_font_extension(Path::new("font.ttf.exe")));
    }

    #[cfg(unix)]
    #[test]
    fn canonical_path_allow_list_rejects_symlink_escape() {
        use std::os::unix::fs::symlink;

        let temp = tempfile::tempdir().unwrap();
        let fonts_root = temp.path().join("fonts");
        let outside_root = temp.path().join("outside");
        fs::create_dir_all(&fonts_root).unwrap();
        fs::create_dir_all(&outside_root).unwrap();
        let outside_font = outside_root.join("outside.ttf");
        let link = fonts_root.join("linked.ttf");
        fs::write(&outside_font, b"outside").unwrap();
        symlink(&outside_font, &link).unwrap();

        assert!(!path_is_within_root(&link, &fonts_root));
    }

    #[cfg(windows)]
    #[test]
    fn canonical_path_allow_list_rejects_symlink_escape() {
        use std::os::windows::fs::symlink_file;

        let temp = tempfile::tempdir().unwrap();
        let fonts_root = temp.path().join("fonts");
        let outside_root = temp.path().join("outside");
        fs::create_dir_all(&fonts_root).unwrap();
        fs::create_dir_all(&outside_root).unwrap();
        let outside_font = outside_root.join("outside.ttf");
        let link = fonts_root.join("linked.ttf");
        fs::write(&outside_font, b"outside").unwrap();
        if symlink_file(&outside_font, &link).is_err() {
            // Some Windows runners do not grant symlink creation to the user.
            return;
        }

        assert!(!path_is_within_root(&link, &fonts_root));
    }

    #[test]
    fn windows_user_font_directory_uses_local_app_data_root() {
        let root = PathBuf::from("C:/Users/test/AppData/Local");
        assert_eq!(
            windows_user_font_dirs(&root),
            vec![PathBuf::from(
                "C:/Users/test/AppData/Local/Microsoft/Windows/Fonts"
            )]
        );
    }
}
