# Data files live in per-kind folders under data_path

Files under `data_path` are sorted into one folder per kind: `songs/uploaded`, `songs/normalized`, `photos/original` and `photos/resized`. Kürfotos (#441) are the first files in this layout.

**The database stores bare file names; the folder follows from the kind.** A song's original is a name in `songs/uploaded`, its normalized copy a name in `songs/normalized`. Because no path is stored, a folder, or later a storage engine per kind (S3, files embedded in the database), can change without a database migration.

**The subfolders are fixed, not configurable.** Only `data_path` is a setting. An operator who wants a kind on another disk mounts or symlinks that folder.

**`photos/resized` is a cache.** It can always be rebuilt from `photos/original` and may be deleted at any time. The same holds for `songs/normalized`, which is rebuilt from `songs/uploaded`. Backups therefore need to cover only `songs/uploaded` and `photos/original` and can skip the caches.

**Startup creates `photos/original` and `photos/resized` if missing**, so an existing data folder needs no manual step.

**Songs move later.** Songs still sit directly in `data_path` and move into `songs/uploaded` and `songs/normalized` in a separate issue, with a migration of the stored names. This ADR only fixes the target layout.

**No storage interface per kind is built yet.** A trait per kind would be speculative while there is only the local disk. The bare-name rule is what keeps that door open; the interface is added when a second engine is actually needed.
