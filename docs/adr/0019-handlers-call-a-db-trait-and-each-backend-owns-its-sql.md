# Handlers call a `Db` trait; each backend owns its SQL

The server was written against SQLite: about 350 checked `sqlx::query!` macros and more than a hundred unchecked queries sit in route handlers and domain modules, and 134 files take the SQLite pool directly. A second backend (PostgreSQL, #238) cannot be added while the SQL is spread over the whole application. From now on HTTP handlers contain no SQL, and each database backend owns its own (#238).

**Handlers contain no SQL.** A handler, extractor or domain module outside a backend crate does not import `sqlx`. It takes `Extension<DbHandle>` (an `Arc<dyn Db>`) and calls domain methods such as `find_user` or `set_roles`.

**`Db` is made of per-domain sub-traits.** `Db` is a supertrait over `AuthRepo`, `UsersRepo`, `SettingsRepo` and so on, one trait per domain, each backend implementing all of them (`SqliteRepo` in `crates/db-sqlite`). A method is named for what the domain does, not for the query behind it. Trait signatures use domain types only: no pool, no connection, no transaction, no sqlx type.

**Each backend owns its SQL.** Every backend crate writes its own `query!`-checked SQL against its own schema and migrations, using the dialect's own functions. Nothing is shared between backends except the trait. A backend's schema is checked at compile time in that crate alone.

**Transactions stay inside the implementation.** One trait method is one unit of work; the implementation decides how to make it atomic (for SQLite `BEGIN IMMEDIATE`). Registering a user and storing the mail token is one method, not two calls wrapped by the handler.

**Row and domain types live in `db-core`.** The types the methods take and return (IDs, `Role`, `User`, and the rows of later groups) are plain structs in `crates/db-core`, which has no database driver. Backends map their query results onto them.

**Capabilities are not part of every backend.** Features that only make sense for one backend, such as file backups and restore (`VACUUM INTO`, replacing the file), are an optional capability of the backend, not a method every backend must fake. Routes that need one answer "not implemented" when the backend lacks it; `reload()` is a no-op where there is no file.

**Migration is group by group.** `SqliteRepo` wraps the existing `ReloadableSqlite`, so handlers can move to the trait one domain at a time while the rest still use the pool. When the last group has moved, handlers lose the pool extension and the root crate drops its SQLite driver.

**Why not one shared SQL layer?** A portable-SQL subset would block features both backends can do better in their own dialect (`julianday`, `json_group_array`, `NULLS` ordering), and a query builder would give up the compile-time checks that catch schema drift. Why not a generic repository over `sqlx::Database`: it leaks sqlx into every signature, which is what this record removes.
