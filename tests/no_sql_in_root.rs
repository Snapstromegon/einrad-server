//! Guard for ADR 0019: the root crate has no SQL. HTTP handlers talk to the
//! `Db` trait, each backend crate owns its own checked SQL. This fails when
//! SQL or a SQLite pool sneaks back into `src/` (the binaries under
//! `src/bin` construct the backend and are exempt).

use std::path::{Path, PathBuf};

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            if path.file_name().is_some_and(|name| name == "bin") {
                continue;
            }
            rust_files(&path, out);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            out.push(path);
        }
    }
}

fn sources(sub: &str) -> Vec<(PathBuf, String)> {
    let mut files = Vec::new();
    rust_files(&Path::new(env!("CARGO_MANIFEST_DIR")).join(sub), &mut files);
    files
        .into_iter()
        .map(|path| {
            let text = std::fs::read_to_string(&path).unwrap();
            (path, text)
        })
        .collect()
}

fn offenders(sub: &str, forbidden: &[&str]) -> Vec<String> {
    let mut found = Vec::new();
    for (path, text) in sources(sub) {
        for (number, line) in text.lines().enumerate() {
            let code = line.split("//").next().unwrap_or("");
            if let Some(word) = forbidden.iter().find(|word| code.contains(**word)) {
                found.push(format!("{}:{}: {word}", path.display(), number + 1));
            }
        }
    }
    found
}

#[test]
fn the_root_crate_has_no_sql_and_no_sqlite_pool() {
    let found = offenders(
        "src",
        &[
            "sqlx::query",
            "query!(",
            "query_scalar!(",
            "query_as!(",
            "SqlitePool",
            "SqliteConnection",
            "ReloadableSqlite",
        ],
    );
    assert!(
        found.is_empty(),
        "SQL belongs in a backend crate behind the Db trait (ADR 0019):\n{}",
        found.join("\n")
    );
}

#[test]
fn http_handlers_do_not_use_sqlx_at_all() {
    let found = offenders("src/http_server", &["sqlx::", "sqlx ", "use sqlx"]);
    assert!(
        found.is_empty(),
        "HTTP handlers call Db trait methods, never sqlx (ADR 0019):\n{}",
        found.join("\n")
    );
}
