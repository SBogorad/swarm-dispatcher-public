//! Сохранение артефакта на диск: content-addressable по sha256, атомарная запись.

use crate::error::{Result, WorkerError};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// Путь к файлу артефакта по его ref (`sha256:<hex>` → `<dir>/<hex>.txt`).
pub fn artifact_path(dir: &Path, artifact_ref: &str) -> PathBuf {
    let hex = artifact_ref.strip_prefix("sha256:").unwrap_or(artifact_ref);
    dir.join(format!("{hex}.txt"))
}

/// sha256 содержимого в виде `sha256:<hex>`.
pub fn sha256_ref(content: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(content);
    format!("sha256:{:x}", h.finalize())
}

/// Прочитать файл и сверить его sha256 с ожидаемым ref.
///
/// Используется после записи, а также тестами для проверки подмены файла.
pub fn verify_artifact(path: &Path, expected_ref: &str) -> Result<()> {
    let bytes =
        std::fs::read(path).map_err(|e| WorkerError::Artifact(format!("read {path:?}: {e}")))?;
    let got = sha256_ref(&bytes);
    if got != expected_ref {
        return Err(WorkerError::Artifact(format!(
            "sha256 mismatch: expected {expected_ref}, got {got}"
        )));
    }
    Ok(())
}

/// Атомарно записать артефакт. Возвращает `(ref, path, size)`.
///
/// - Если файл уже существует с тем же именем — **не перезаписываем**
///   (content-addressable: одно содержимое = один файл).
/// - Запись идёт в `.tmp.<pid>`, затем `rename` (атомарно на POSIX).
/// - После записи файл перечитывается, sha256 сверяется с ref.
pub fn write_artifact(dir: &Path, content: &[u8]) -> Result<(String, PathBuf, u64)> {
    std::fs::create_dir_all(dir)
        .map_err(|e| WorkerError::Artifact(format!("create_dir_all {dir:?}: {e}")))?;

    let reference = sha256_ref(content);
    let final_path = artifact_path(dir, &reference);

    if final_path.exists() {
        // Уже есть — идемпотентность, не трогаем. Но имя = хэш содержимого:
        // если файл повреждён/подменён, честно сообщаем об ошибке (M2).
        verify_artifact(&final_path, &reference)?;
        let size = std::fs::metadata(&final_path)
            .map_err(|e| WorkerError::Artifact(format!("stat {final_path:?}: {e}")))?
            .len();
        return Ok((reference, final_path, size));
    }

    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or_default();
    let tmp = dir.join(format!(
        "{}.tmp.{}.{}",
        final_path.file_name().unwrap().to_string_lossy(),
        std::process::id(),
        nonce
    ));
    let result: std::result::Result<(), WorkerError> = (|| {
        use std::io::Write;
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&tmp)
            .map_err(|e| WorkerError::Artifact(format!("create {tmp:?}: {e}")))?;
        file.write_all(content)
            .map_err(|e| WorkerError::Artifact(format!("write {tmp:?}: {e}")))?;
        file.sync_all()
            .map_err(|e| WorkerError::Artifact(format!("sync {tmp:?}: {e}")))?;
        std::fs::rename(&tmp, &final_path)
            .map_err(|e| WorkerError::Artifact(format!("rename {tmp:?} -> {final_path:?}: {e}")))?;
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result?;

    // Проверка sha256 после записи.
    verify_artifact(&final_path, &reference)?;
    let size = std::fs::metadata(&final_path)
        .map_err(|e| WorkerError::Artifact(format!("stat {final_path:?}: {e}")))?
        .len();

    Ok((reference, final_path, size))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmpdir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!(
            "swarm_worker_test_{tag}_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn artifact_path_from_ref() {
        let p = artifact_path(Path::new("/var/swarm/artifacts"), "sha256:ab12cd");
        assert_eq!(p, PathBuf::from("/var/swarm/artifacts/ab12cd.txt"));
        // без префикса — тоже работает
        let p2 = artifact_path(Path::new("/x"), "deadbeef");
        assert_eq!(p2, PathBuf::from("/x/deadbeef.txt"));
    }

    #[test]
    fn atomic_write_leaves_no_tmp() {
        let dir = tmpdir("atomic");
        let (reference, path, size) = write_artifact(&dir, b"hello world").unwrap();
        assert!(reference.starts_with("sha256:"));
        assert_eq!(size, 11);
        assert!(path.exists());
        // .tmp-файлов не осталось
        let leftovers: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().contains(".tmp."))
            .collect();
        assert!(leftovers.is_empty(), "остались tmp-файлы: {leftovers:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn sha256_matches() {
        let dir = tmpdir("match");
        let (reference, path, _) = write_artifact(&dir, b"payload").unwrap();
        let on_disk = std::fs::read(&path).unwrap();
        assert_eq!(sha256_ref(&on_disk), reference);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn sha256_mismatch_fails() {
        let dir = tmpdir("mismatch");
        // (а) целостный файл проходит проверку
        let (reference, path, _) = write_artifact(&dir, b"payload").unwrap();
        verify_artifact(&path, &reference).unwrap();

        // (б) подмена содержимого ломает прямую проверку
        std::fs::write(&path, b"tampered").unwrap();
        assert!(
            verify_artifact(&path, &reference).is_err(),
            "подмена файла должна ломать sha256-проверку"
        );

        // (в) и тот же инвариант на пути write_artifact: если по адресу
        //     (имя = sha256) лежит повреждённый файл — ошибка, а не тихое «ок».
        let dir2 = tmpdir("mismatch2");
        let content = b"expected-content";
        let ref2 = sha256_ref(content);
        let p2 = artifact_path(&dir2, &ref2);
        std::fs::write(&p2, b"corrupted").unwrap();
        let err = write_artifact(&dir2, content);
        assert!(
            err.is_err(),
            "write_artifact обязан заметить повреждённый файл по адресу-хэшу"
        );

        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_dir_all(&dir2);
    }

    #[test]
    fn existing_file_not_overwritten() {
        let dir = tmpdir("idem");
        let (r1, p1, _) = write_artifact(&dir, b"same").unwrap();
        let mtime1 = std::fs::metadata(&p1).unwrap().modified().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(10));
        let (r2, p2, _) = write_artifact(&dir, b"same").unwrap();
        assert_eq!(r1, r2);
        assert_eq!(p1, p2);
        let mtime2 = std::fs::metadata(&p2).unwrap().modified().unwrap();
        assert_eq!(
            mtime1, mtime2,
            "существующий файл не должен перезаписываться"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
