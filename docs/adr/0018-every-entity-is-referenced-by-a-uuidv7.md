# Every entity is referenced by a UUIDv7; names and codes are editable

Heats and finals got a UUID `id` in #347 (ADR 0016, ADR 0017), but their `name` stayed the primary key, and categories are keyed by `name` outright. Renaming one is fragile, because everything that points at it has to follow. Every database entity now has a UUIDv7 as its key, and human-readable names and codes are plain attributes, never keys (#351).

**Every primary key is a UUIDv7.** Stored as a `BLOB`, minted by the app with `Uuid::now_v7()`. No database default or trigger mints IDs. The exceptions are the tables nothing human-editable keys: integer-keyed log and sequence tables (`timeplan`, `timeplan_slots`, `booking_log`, `club_mail_messages`, `competition_image`, and the like) and composite join tables. The **competition** keeps its immutable slug `id`, which is part of its identity (the database is the competition, ADR 0001).

**Names and codes are editable.** A name or code is unique where the domain needs it (ADR 0017: heat and final names and codes, per table), and a rename or recode is one `UPDATE` of one row; every reference is by ID.

**Files and metrics stay human.** The competition file keeps its human keys: categories by `name`, heats and finals by `code` (ADR 0017); the import resolves them to IDs, and the export writes names and codes, never IDs. A rename therefore only matters for the next import or replace. Prometheus labels stay names (`club`, `category`): a rename splits a series, which is acceptable.

**Migration IDs are deterministic.** The migration mints IDs for existing rows from a fixed timestamp plus the row's position, with the remaining bits from a stable source, so the migration gives identical IDs on every copy of the database (server and laptop, see `make download` and `make sync-up`) and running it again changes nothing. A value that is already a valid v7 UUID is never replaced, so running the migration again changes nothing. The heats and finals from #347 had random-blob IDs (`randomblob`), not v7, so they are re-minted as well (HFID-2), and every reference follows; open tabs holding old heat or final IDs fail their next command (HFID-5).

**Why not random or integer IDs?** An integer key leaks creation order and collides when two copies of the database diverge; a random UUIDv4 fragments the index. A v7 sorts by creation, is safe to mint in the app on any copy, and, with the fixed timestamp, can be made reproducible for the migration. Why not keep names as keys: that is the fragility this record removes.
