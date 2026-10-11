//! Housekeeping for the on-disk SVG caches used by Mermaid and LaTeX blocks.
//!
//! Rendered SVGs are keyed by content and visual parameters, so every edit,
//! theme switch or font-size change leaves another file behind. Files are
//! pruned once they have not been used for [`SVG_CACHE_MAX_AGE`]; cache hits
//! refresh the modification time so diagrams in active documents survive.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use directories::ProjectDirs;

/// Unused cache files older than this are deleted on startup.
pub(crate) const SVG_CACHE_MAX_AGE: Duration = Duration::from_secs(30 * 24 * 60 * 60);
/// Cache hits refresh a file's modification time once it is this old, which
/// keeps the extra write rare while still marking the file as in use.
const SVG_CACHE_REFRESH_AFTER: Duration = Duration::from_secs(7 * 24 * 60 * 60);
/// Delay before the startup cleanup runs, so documents opened at launch get to
/// refresh the files they use before stale files are judged.
const SVG_CACHE_CLEANUP_DELAY: Duration = Duration::from_secs(30);

/// Root of Velotype's per-user cache directory.
pub(crate) fn velotype_cache_root() -> PathBuf {
    ProjectDirs::from("com", "manyougz", "Velotype")
        .map(|dirs| dirs.cache_dir().to_path_buf())
        .unwrap_or_else(|| std::env::temp_dir().join("Velotype"))
}

fn svg_cache_dirs() -> [PathBuf; 2] {
    let root = velotype_cache_root();
    [root.join("mermaid-svg"), root.join("latex-svg")]
}

/// Marks a cache file as recently used after a cache hit.
pub(crate) fn mark_svg_cache_hit(path: &Path) {
    let Ok(metadata) = fs::metadata(path) else {
        return;
    };
    let now = SystemTime::now();
    let stale = metadata
        .modified()
        .ok()
        .and_then(|modified| now.duration_since(modified).ok())
        .is_some_and(|age| age >= SVG_CACHE_REFRESH_AFTER);
    if stale && let Ok(file) = fs::File::options().append(true).open(path) {
        let _ = file.set_modified(now);
    }
}

/// Deletes `.svg` files under `dir` (recursively) whose modification time is
/// older than `max_age` relative to `now`. Returns the number of removed files.
pub(crate) fn prune_stale_svg_files(dir: &Path, max_age: Duration, now: SystemTime) -> usize {
    let Ok(entries) = fs::read_dir(dir) else {
        return 0;
    };
    let mut removed = 0;
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if file_type.is_dir() {
            removed += prune_stale_svg_files(&path, max_age, now);
            continue;
        }
        if !file_type.is_file()
            || !path
                .extension()
                .is_some_and(|extension| extension.eq_ignore_ascii_case("svg"))
        {
            continue;
        }
        let stale = entry
            .metadata()
            .and_then(|metadata| metadata.modified())
            .ok()
            .and_then(|modified| now.duration_since(modified).ok())
            .is_some_and(|age| age > max_age);
        if stale && fs::remove_file(&path).is_ok() {
            removed += 1;
        }
    }
    removed
}

/// Prunes stale Mermaid and LaTeX SVG cache files on a background thread.
pub(crate) fn spawn_svg_cache_cleanup() {
    let spawned = std::thread::Builder::new()
        .name("svg-cache-cleanup".into())
        .spawn(|| {
            std::thread::sleep(SVG_CACHE_CLEANUP_DELAY);
            let now = SystemTime::now();
            for dir in svg_cache_dirs() {
                prune_stale_svg_files(&dir, SVG_CACHE_MAX_AGE, now);
            }
        });
    if let Err(err) = spawned {
        eprintln!("failed to start SVG cache cleanup: {err}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_cache_dir(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "velotype-svg-cache-{label}-{}",
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(dir.join("nested")).expect("create temp cache dir");
        dir
    }

    fn write_with_age(path: &Path, age: Duration) {
        fs::write(path, "<svg/>").expect("write cache file");
        let file = fs::File::options()
            .append(true)
            .open(path)
            .expect("open cache file");
        file.set_modified(SystemTime::now() - age)
            .expect("set modified time");
    }

    #[test]
    fn prunes_only_stale_svg_files_recursively() {
        let dir = temp_cache_dir("prune");
        let day = Duration::from_secs(24 * 60 * 60);
        let stale = dir.join("stale.svg");
        let fresh = dir.join("fresh.svg");
        let nested_stale = dir.join("nested").join("old.svg");
        let other = dir.join("notes.txt");
        write_with_age(&stale, day * 40);
        write_with_age(&fresh, day);
        write_with_age(&nested_stale, day * 31);
        write_with_age(&other, day * 90);

        let removed = prune_stale_svg_files(&dir, SVG_CACHE_MAX_AGE, SystemTime::now());

        assert_eq!(removed, 2);
        assert!(!stale.exists());
        assert!(!nested_stale.exists());
        assert!(fresh.exists());
        assert!(other.exists(), "non-SVG files are left alone");
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn pruning_a_missing_dir_is_a_no_op() {
        let dir = std::env::temp_dir().join(format!("velotype-missing-{}", uuid::Uuid::new_v4()));
        assert_eq!(
            prune_stale_svg_files(&dir, SVG_CACHE_MAX_AGE, SystemTime::now()),
            0
        );
    }

    #[test]
    fn cache_hit_refreshes_old_files_so_they_survive_pruning() {
        let dir = temp_cache_dir("hit");
        let used = dir.join("used.svg");
        write_with_age(&used, Duration::from_secs(45 * 24 * 60 * 60));

        mark_svg_cache_hit(&used);
        let removed = prune_stale_svg_files(&dir, SVG_CACHE_MAX_AGE, SystemTime::now());

        assert_eq!(removed, 0);
        assert!(used.exists());
        let _ = fs::remove_dir_all(dir);
    }
}
