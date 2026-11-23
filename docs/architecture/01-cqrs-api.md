# 01 · CQRS API

## Rules

1. **Command** = `POST /api/command/<verb>_<noun>`. Takes a JSON body (or `multipart/form-data` for a file upload). Changes state. Answers with an *acknowledgement*: `{}`, the ID of an entity it created, warnings that didn't stop the save, or an outcome enum that says which branch ran. It doesn't answer with a read model; the client re-queries.
2. **Query** = `GET /api/query/<get|list>_<noun>`. Takes query-string params only. Changes nothing (no SQL writes, no file writes, no outbound calls with side effects). May be called any number of times.
3. **No third kind** under `/api`. Everything else is a websocket or an asset (see [07](07-non-cqrs-surfaces.md)).
4. Every command and query is in the OpenAPI document, so the frontend calls it through the generated client (`frontend/src/ts/gen`), never through a hand-written `fetch`.

## Where the API stands

The mechanical split is clean:

- 97 command handlers in `src/http_server/routes/command/`, all `post`, all under `/api/command` (`src/http_server/routes/command.rs`).
- 43 query handlers in `src/http_server/routes/query/`, all `get`, all under `/api/query` (`src/http_server/routes/query.rs`).
- No query file contains `INSERT`, `UPDATE`, `DELETE` or `REPLACE`.
- The frontend uses the generated kubb clients in 45 files. Hand-written `fetch` calls against `/api/query` exist only for `whoami` (`components/elements/cup-context-user.ts:189`, `components/views/cup-view-setup.ts:60`) and `export_competition` (download link).

## Deviations

### Queries sent as commands

| Endpoint | Why it's a query | Fix |
|---|---|---|
| `POST /api/command/preview_club_mail` | Renders mails, writes nothing, returns `PreparedMails` | The body is large (templates). Either accept a "query with a body" category explicitly (see open questions) or rename the route group to make the exception visible. |
| `POST /api/command/preview_competition_import` | Parses a competition file, writes nothing, returns `Preview` | Same: a whole competition file doesn't fit a query string. |

Both are legitimate needs. The rule needs a stated exception for **read-only requests whose input is too large for a query string**, otherwise every future preview will repeat the question.

### Commands that answer with a read model

| Command | Returns | Note |
|---|---|---|
| `set_image_source` | `images: CompetitionImages` | Same data as `query/get_competition_images` |
| `refresh_images` | `images: CompetitionImages` | Same |
| `send_club_mail` | `SentClubMail` (whole record, templates included) | Same data as `query/list_club_mails`; `{ id }` would do |
| `restore_db_backup` | `missing_songs: Vec<MissingSong>` | Same data as `query/list_missing_songs` |
| `import_competition` | `outcome`, `dropped`, `remap_file` | An outcome report, arguably OK; `dropped` is a read model of what the import skipped |

Commands that echo an ID the client already sent (no new information):

- `rename_club` → `{ club_id }`, `set_club_responsible` → `{ club_id }`.

Commands that return a server-minted ID (`add_rider`, `create_club`, `create_booking`, `add_category`, `create_club_group`, `add_club_judge`, `add_timeplan_entry`, `add_sponsor`, …) are fine under the rules above. ADR 0018 lets the app mint UUIDv7s anywhere, so client-minted IDs would allow `{}` everywhere; that is a separate decision (see open questions).

### Request-shape deviations inside commands

| Command | Deviation |
|---|---|
| `reload_db` | Takes `token` as a **query param** on a `POST`; its OpenAPI `request_body=Upload` is an empty schema copied from `save_act_song`. |
| `save_act_song` | Takes `act_id` as a **query param** next to the multipart body. The other uploads (`set_competition_logo`, `upload_sponsor_image`, …) put everything in the multipart form. |
| `login` | Declares `401` with `body=String`, but answers with `ClientError` like everyone else. |

### Read-only routes outside `/api/query`

These return data that changes with state, so they're queries in substance, not assets:

| Route | File | What it is |
|---|---|---|
| `GET /api/photos/newest`, `/api/photos/random` | `routes/photos.rs` | Redirect to the current photo; the choice depends on state |
| `GET /api/db-backups/{filename}` | `routes/db_backups.rs` | Authenticated download of a backup file |
| `GET /api/setup/session`, `/api/setup/status` | `src/setup.rs:587` | Setup state machine, not in OpenAPI |
| `POST /api/setup/session`, `/api/setup/test-mail`, `/api/setup/complete` | `src/setup.rs:587` | Setup commands, REST-style (`GET` + `POST` on one path), hand-written `fetch` in `cup-setup*.ts` |

The photo redirects and the backup download produce **binary responses**, which the JSON query handlers don't. That is the real reason they sit outside. The cleanest rule: binary downloads are *assets*, addressed by an immutable ID (`/api/photos/{id}`, `/api/db-backups/{filename}`), and the *choice* of which one ("newest", "random") is a query that returns the ID. See [07](07-non-cqrs-surfaces.md) for setup.

## Audit commands

```bash
# Methods per side (expect only post / only get)
grep -A2 -h '#\[utoipa::path' src/http_server/routes/command/*.rs | grep -oE '\b(get|post|put|delete|patch)\b' | sort | uniq -c
grep -A2 -h '#\[utoipa::path' src/http_server/routes/query/*.rs | grep -oE '\b(get|post|put|delete|patch)\b' | sort | uniq -c
# Writes in queries (expect nothing)
grep -nE '\b(INSERT|UPDATE|DELETE FROM|REPLACE INTO)\b' src/http_server/routes/query/*.rs
# Routes outside the CQRS routers
grep -n '\.route(' src/http_server/routes.rs src/setup.rs
```
