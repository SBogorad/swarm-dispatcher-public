//! Read-only исходники репозитория в промпте воркера.
//!
//! Живой урок `agent_memory#1` (2026-10-18, граф `51e29279`): задача просила
//! поправить `scripts/acceptance_allowlist.txt`, а контейнер воркера этот файл
//! не видел. Итог — `blocked (contract_error: missing_input: ... нет доступа к
//! рабочей директории)`. Воркер не мог ни прочитать файл, ни построить
//! корректный unified diff, поэтому выдумывал якоря `@@ ... @@` вслепую.
//!
//! Здесь — узкий, явно ограниченный канал: постановка задачи упоминает пути,
//! воркер читает **только их** из `SWARM_REPO_DIR`, только на чтение и только
//! если путь остаётся внутри корня. Никакого обхода дерева: список путей
//! берётся из текста контракта, а не из файловой системы.
//!
//! Почему не «прочитать всё»: контейнер `--read-only` с лимитом промпта, а
//! репозиторий содержит `.env` с боевыми секретами. Чтение ограничено
//! разрешёнными путями, запретными шаблонами и потолком на файл.

use std::path::{Component, Path, PathBuf};

/// Потолок на один файл (байты UTF-8).
///
/// 24 КБ — типичный исходник проекта целиком, но не «весь файл на 200 КБ»:
/// промпт с шестью такими файлами остаётся в пределах разумного бюджета.
pub const DEFAULT_MAX_FILE_BYTES: usize = 24 * 1024;

/// Сколько файлов максимум подставлять в один промпт.
pub const DEFAULT_MAX_FILES: usize = 6;

/// Каталоги репозитория, упоминание которых считаем ссылкой на файл.
const PATH_PREFIXES: [&str; 12] = [
    "crates/",
    "scripts/",
    "schema/",
    "docs/",
    "tests/",
    "step2/",
    "step2b/",
    "ops/",
    "demo/",
    ".github/",
    "migrations/",
    "src/",
];

/// Файлы в корне, которые называются без каталога.
const ROOT_FILES: [&str; 8] = [
    "Cargo.toml",
    "Cargo.lock",
    "README.md",
    "Makefile",
    "docker-compose.yml",
    "docker-compose.demo.yml",
    "rust-toolchain.toml",
    "CHANGELOG.md",
];

/// Запретные компоненты пути: секреты и служебные каталоги.
const FORBIDDEN_COMPONENTS: [&str; 7] = [
    ".git",
    ".env",
    "secrets",
    "secret",
    "id_rsa",
    "id_ed25519",
    ".ssh",
];

/// Запретные расширения (ключи и хранилища секретов).
const FORBIDDEN_EXTS: [&str; 6] = ["pem", "key", "p12", "pfx", "keystore", "jks"];

/// Прочитанный файл репозитория.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoFile {
    /// Путь относительно корня репозитория (как он был в постановке).
    pub path: String,
    pub content: String,
    /// Содержимое обрезано по потолку `max_file_bytes`.
    pub truncated: bool,
}

/// Ограниченный читатель репозитория.
#[derive(Debug, Clone)]
pub struct RepoContext {
    root: PathBuf,
    max_file_bytes: usize,
    max_files: usize,
}

impl RepoContext {
    pub fn new(root: impl Into<PathBuf>, max_file_bytes: usize, max_files: usize) -> Self {
        Self {
            root: root.into(),
            max_file_bytes,
            max_files,
        }
    }

    /// Контекст из окружения: `SWARM_REPO_DIR` задан и это каталог.
    ///
    /// `None` — поведение воркера не меняется (как до этого модуля): старые
    /// контуры без смонтированного репозитория продолжают работать.
    pub fn from_env() -> Option<Self> {
        let root = std::env::var("SWARM_REPO_DIR").ok()?;
        if root.trim().is_empty() {
            return None;
        }
        let path = PathBuf::from(root);
        if !path.is_dir() {
            return None;
        }
        // Потолок на файл настраивается: боевые исходники бывают крупнее
        // дефолтных 24 КБ (`swarm-reviewer/src/main.rs` — 34 КБ,
        // `swarm-worker/src/prompt.rs` — 51 КБ). Без этого воркер видел
        // только начало файла и не мог править хвост: любой его diff по
        // таким файлам был фантазией о невидимом коде.
        let max_bytes = std::env::var("SWARM_REPO_MAX_FILE_BYTES")
            .ok()
            .and_then(|v| v.parse::<usize>().ok())
            .filter(|n| *n >= 1024)
            .unwrap_or(DEFAULT_MAX_FILE_BYTES);
        Some(Self::new(path, max_bytes, DEFAULT_MAX_FILES))
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Собрать файлы, упомянутые в постановке задачи.
    ///
    /// Порядок — порядок упоминания (важное в тексте обычно идёт первым);
    /// дубликаты схлопываются. Отсутствующие и запретные пути молча
    /// пропускаются: это не ошибка задачи, а деградация контекста.
    pub fn collect(&self, contract: &serde_json::Value) -> Vec<RepoFile> {
        let text = contract_text(contract);
        let mut out = Vec::new();
        for rel in extract_paths(&text) {
            if out.len() >= self.max_files {
                break;
            }
            if out.iter().any(|f: &RepoFile| f.path == rel) {
                continue;
            }
            if let Some(file) = self.read_path(&rel) {
                out.push(file);
            }
        }
        out
    }

    /// Прочитать один путь, если он безопасен и существует.
    pub fn read_path(&self, rel: &str) -> Option<RepoFile> {
        let rel = normalize_rel(rel)?;
        if is_forbidden(&rel) {
            return None;
        }
        let root = self.root.canonicalize().ok()?;
        let candidate = root.join(&rel);
        let real = candidate.canonicalize().ok()?;
        // Симлинк или `..` не должны вывести за корень.
        if !real.starts_with(&root) {
            return None;
        }
        let meta = std::fs::metadata(&real).ok()?;
        if !meta.is_file() {
            return None;
        }
        let bytes = std::fs::read(&real).ok()?;
        let mut end = bytes.len().min(self.max_file_bytes);
        while end > 0 && std::str::from_utf8(&bytes[..end]).is_err() {
            end -= 1;
        }
        Some(RepoFile {
            path: rel,
            content: String::from_utf8_lossy(&bytes[..end]).into_owned(),
            truncated: bytes.len() > end,
        })
    }
}

/// Текст постановки: `description` + `goal`.
fn contract_text(contract: &serde_json::Value) -> String {
    let mut text = String::new();
    for key in ["goal", "description"] {
        if let Some(v) = contract.get(key).and_then(|v| v.as_str()) {
            text.push_str(v);
            text.push('\n');
        }
    }
    text
}

/// Извлечь относительные пути к файлам из свободного текста.
///
/// Токен засчитывается, если он начинается с известного каталога репозитория
/// либо совпадает с известным файлом корня. Суффикс `:42` (номер строки)
/// отбрасывается — читаем файл целиком.
pub fn extract_paths(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for raw in text.split(|c: char| {
        c.is_whitespace()
            || matches!(
                c,
                ',' | ';'
                    | '('
                    | ')'
                    | '['
                    | ']'
                    | '{'
                    | '}'
                    | '<'
                    | '>'
                    | '"'
                    | '\''
                    | '`'
                    | '*'
                    | '|'
            )
    }) {
        let token = raw.trim_matches(|c: char| matches!(c, '.' | ':' | '!' | '?' | '-' | '—'));
        if token.is_empty() {
            continue;
        }
        let candidate = strip_line_suffix(token);
        if candidate.is_empty() || candidate.contains("..") {
            continue;
        }
        let looks_like_path = PATH_PREFIXES
            .iter()
            .any(|p| candidate.as_str().starts_with(p))
            || ROOT_FILES.contains(&candidate.as_str());
        if !looks_like_path {
            continue;
        }
        // Только относительные пути: абсолютный путь — не наш случай.
        if candidate.starts_with('/') {
            continue;
        }
        if !out.iter().any(|p| p == &candidate) {
            out.push(candidate);
        }
    }
    out
}

/// `scripts/x.sh:42` → `scripts/x.sh`.
fn strip_line_suffix(token: &str) -> String {
    let token = token.trim_end_matches('/');
    match token.rsplit_once(':') {
        Some((head, tail))
            if !head.is_empty() && !tail.is_empty() && tail.chars().all(|c| c.is_ascii_digit()) =>
        {
            head.to_string()
        }
        _ => token.to_string(),
    }
}

/// Привести путь к безопасной относительной форме.
///
/// `None` — путь абсолютный, содержит `..` или не является обычным
/// относительным путём.
fn normalize_rel(rel: &str) -> Option<String> {
    let path = Path::new(rel);
    if path.is_absolute() {
        return None;
    }
    let mut parts: Vec<String> = Vec::new();
    for c in path.components() {
        match c {
            Component::Normal(s) => parts.push(s.to_string_lossy().into_owned()),
            Component::CurDir => {}
            // `..`, корень и префиксы — за пределами допустимого.
            _ => return None,
        }
    }
    if parts.is_empty() {
        return None;
    }
    Some(parts.join("/"))
}

/// Запретный ли путь: секреты и служебные каталоги.
pub fn is_forbidden(rel: &str) -> bool {
    let low = rel.to_ascii_lowercase();
    for comp in low.split('/') {
        if FORBIDDEN_COMPONENTS.contains(&comp) {
            return true;
        }
        // `.env`, `.env.local`, `.env.production` и прочие варианты.
        if comp.starts_with(".env") {
            return true;
        }
        if let Some(ext) = comp.rsplit_once('.').map(|(_, e)| e) {
            if FORBIDDEN_EXTS.contains(&ext) {
                return true;
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn temp_repo(files: &[(&str, &str)]) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "swarm_repo_ctx_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        for (rel, body) in files {
            let path = dir.join(rel);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, body).unwrap();
        }
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn extract_paths_finds_repo_files_and_strips_line_numbers() {
        let text = "Поправь scripts/acceptance_harness.sh:42 и crates/swarm-core/src/lib.rs, \
                    а также Cargo.toml. Не трогай /etc/passwd и ../../etc/shadow.";
        let paths = extract_paths(text);
        assert_eq!(
            paths,
            vec![
                "scripts/acceptance_harness.sh",
                "crates/swarm-core/src/lib.rs",
                "Cargo.toml"
            ]
        );
    }

    #[test]
    fn extract_paths_ignores_prose_and_absolute_paths() {
        let paths = extract_paths("Просто текст без файлов. /usr/bin/cargo и http://x/y");
        assert!(paths.is_empty(), "лишние пути: {paths:?}");
    }

    #[test]
    fn read_path_rejects_traversal_and_secrets() {
        let dir = temp_repo(&[
            ("scripts/ok.sh", "echo ok"),
            (".env", "SECRET=1"),
            ("secrets/token.txt", "tok"),
            ("certs/key.pem", "key"),
        ]);
        let ctx = RepoContext::new(&dir, DEFAULT_MAX_FILE_BYTES, DEFAULT_MAX_FILES);
        assert!(ctx.read_path("scripts/ok.sh").is_some());
        assert!(ctx.read_path("../etc/passwd").is_none());
        assert!(ctx.read_path("/etc/passwd").is_none());
        assert!(ctx.read_path(".env").is_none());
        assert!(ctx.read_path(".env.local").is_none());
        assert!(ctx.read_path("secrets/token.txt").is_none());
        assert!(ctx.read_path("certs/key.pem").is_none());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn collect_reads_only_mentioned_files_in_order() {
        let dir = temp_repo(&[
            ("scripts/a.sh", "A"),
            ("scripts/b.sh", "B"),
            ("docs/never.md", "N"),
        ]);
        let ctx = RepoContext::new(&dir, DEFAULT_MAX_FILE_BYTES, DEFAULT_MAX_FILES);
        let contract = json!({
            "goal": "правка scripts/b.sh",
            "description": "см. scripts/a.sh и scripts/a.sh ещё раз",
        });
        let files = ctx.collect(&contract);
        let paths: Vec<&str> = files.iter().map(|f| f.path.as_str()).collect();
        assert_eq!(paths, vec!["scripts/b.sh", "scripts/a.sh"]);
        assert_eq!(files[0].content, "B");
        assert!(!files[0].truncated);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn collect_respects_max_files_and_truncates_at_char_boundary() {
        let dir = temp_repo(&[
            ("scripts/a.sh", "aaaaaaaa"),
            ("scripts/b.sh", "bbbbbbbb"),
            ("scripts/c.sh", "cccccccc"),
        ]);
        let ctx = RepoContext::new(&dir, 4, 2);
        let contract = json!({"description": "scripts/a.sh scripts/b.sh scripts/c.sh"});
        let files = ctx.collect(&contract);
        assert_eq!(files.len(), 2, "потолок файлов не соблюдён");
        assert!(files[0].truncated);
        assert_eq!(files[0].content, "aaaa");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn truncation_keeps_valid_utf8() {
        // Кириллица: 2 байта на символ; потолок 5 байт режет посередине символа.
        let dir = temp_repo(&[("docs/x.md", "абвгд")]);
        let ctx = RepoContext::new(&dir, 5, DEFAULT_MAX_FILES);
        let f = ctx.read_path("docs/x.md").unwrap();
        assert!(f.truncated);
        assert!(f.content.is_char_boundary(f.content.len()));
        std::fs::remove_dir_all(&dir).ok();
    }
}
