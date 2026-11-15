//! Guard for ADR 0019: the root crate has no SQL. HTTP handlers talk to the
//! `Db` trait, each backend crate owns its own checked SQL. This fails when
//! SQL, sqlx or the SQLite backend sneaks back into `src/` (the binaries under
//! `src/bin` construct the backend and are exempt) or when `db-core` grows
//! SQL of its own. The scanning functions are unit-tested on samples below.

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

/// Lines in `src/` that are allowed to name `sqlx` (file suffix, line fragment).
const SQLX_ALLOWED: &[(&str, &str)] = &[
    // Builds a `DbError` to test the 500 mapping; needs a concrete sqlx error.
    ("src/http_server.rs", "sqlx::Error::RowNotFound"),
];

/// Lines in non-test `src/` code that may name `db_sqlite`.
const BACKEND_ALLOWED: &[(&str, &str)] = &[
    // None: `seed::check_file` builds its scratch database through
    // `db_inmemory`, which this guard does not mind.
];

const CORE_FORBIDDEN: &[&str] = &[
    "query!",
    "query_as!",
    "query_scalar!",
    "sqlx::query",
    "raw_sql",
];

fn allowed(table: &[(&str, &str)], label: &str, code: &str) -> bool {
    table
        .iter()
        .any(|(file, fragment)| label.ends_with(file) && code.contains(fragment))
}

/// What is wrong with a file of the root crate's `src/`. `label` is the
/// `/`-separated path relative to the repository root.
fn root_violations(label: &str, text: &str) -> Vec<String> {
    let mut found = Vec::new();
    let mut in_test_module = false;
    for (number, line) in text.lines().enumerate() {
        if line.trim() == "#[cfg(test)]" {
            in_test_module = true;
        }
        let code = line.split("//").next().unwrap_or("");
        let mut report = |word: &str| found.push(format!("{label}:{}: {word}", number + 1));
        if code.contains("sqlx") && !allowed(SQLX_ALLOWED, label, code) {
            report("sqlx");
        }
        // Unit tests may build a backend; production code only in the
        // allowlist (the binaries are not scanned at all).
        if code.contains("db_sqlite") && !in_test_module && !allowed(BACKEND_ALLOWED, label, code) {
            report("db_sqlite");
        }
        for word in [
            "SqlitePool",
            "SqliteConnection",
            "ReloadableSqlite",
            "raw_sql",
        ] {
            if code.contains(word) {
                report(word);
            }
        }
    }
    found
}

fn core_violations(label: &str, text: &str) -> Vec<String> {
    let mut found = Vec::new();
    for (number, line) in text.lines().enumerate() {
        let code = line.split("//").next().unwrap_or("");
        for word in CORE_FORBIDDEN {
            if code.contains(word) {
                found.push(format!("{label}:{}: {word}", number + 1));
            }
        }
    }
    found
}

fn scan(sub: &str, check: fn(&str, &str) -> Vec<String>) -> Vec<String> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut files = Vec::new();
    rust_files(&root.join(sub), &mut files);
    files
        .into_iter()
        .flat_map(|path| {
            let text = std::fs::read_to_string(&path).unwrap();
            let label = path
                .strip_prefix(root)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/");
            check(&label, &text)
        })
        .collect()
}

#[test]
fn the_root_crate_has_no_sql_sqlx_or_backend() {
    let found = scan("src", root_violations);
    assert!(
        found.is_empty(),
        "SQL and the backend belong in a backend crate behind the Db trait (ADR 0019):\n{}",
        found.join("\n")
    );
}

#[test]
fn db_core_has_no_sql_of_its_own() {
    let found = scan("crates/db-core/src", core_violations);
    assert!(
        found.is_empty(),
        "db-core defines the traits, each backend owns its SQL (ADR 0019):\n{}",
        found.join("\n")
    );
}

fn root_catches(sample: &str) -> bool {
    !root_violations("src/http_server/routes/x.rs", sample).is_empty()
}

#[test]
fn the_guard_catches_every_known_bypass() {
    for sample in [
        "let _ = sqlx::query(\"select 1\");",
        "use sqlx::{query, SqlitePool};",
        "use sqlx::Executor;",
        "sqlx::raw_sql(\"select 1\")",
        "sqlx::query!(\"select 1\")",
        "use db_sqlite::SqliteRepo;",
        "let x = db_sqlite::reloadable_sqlite::in_memory().await;",
        "use db_sqlite::{db_backup, reloadable_sqlite};",
        "let p: SqlitePool;",
    ] {
        assert!(root_catches(sample), "not caught: {sample}");
    }
    for sample in [
        "query!(\"select 1\")",
        "sqlx::query_as!(Row, \"select 1\")",
        "query_scalar!(\"select 1\")",
        "sqlx::query(\"select 1\")",
        "sqlx::raw_sql(\"select 1\")",
        "use sqlx::raw_sql;",
    ] {
        assert!(
            !core_violations("crates/db-core/src/x.rs", sample).is_empty(),
            "not caught in db-core: {sample}"
        );
    }
}

#[test]
fn the_guard_allows_its_exceptions_and_comments() {
    assert!(!root_catches("// sqlx is only mentioned in a comment"));
    assert!(root_violations("src/http_server.rs", "let e = sqlx::Error::RowNotFound;").is_empty());
    // the same line anywhere else is caught
    assert!(root_catches("let e = sqlx::Error::RowNotFound;"));
    assert!(root_violations("src/seed.rs", "let repo = db_inmemory::new().await;").is_empty());
    assert!(
        root_violations(
            "src/player/mod.rs",
            "#[cfg(test)]\nmod tests {\n let db = db_sqlite::SqliteRepo::from_pool(pool);\n}"
        )
        .is_empty()
    );
    assert!(core_violations("crates/db-core/src/ids.rs", "//! `query!` binds").is_empty());
}
