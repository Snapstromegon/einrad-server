# 01 · CQRS API

## Rules

1. **Command** = `POST /api/command/<verb>_<noun>`. Takes a JSON body (or `multipart/form-data` for a file upload). Changes state. Answers with an *acknowledgement*: `{}`, the ID of an entity it created, warnings that didn't stop the save, or an outcome enum that says which branch ran. It doesn't answer with a read model; the client re-queries.
2. **Query** = `GET /api/query/<get|list>_<noun>`. Takes query-string params only. Changes nothing (no SQL writes, no file writes, no outbound calls with side effects). May be called any number of times.
3. **A file from state is a query.** Anything uploaded, generated or stored at runtime (songs, logos, sponsor images, photos, the competition CSS, CSV exports, backups) is served by a query, `GET /api/query/get_<noun>?…` or `export_<noun>`, that answers with the file's bytes and content type instead of JSON. It keeps what a file needs: Range requests, caching headers (`ETag`, `Cache-Control`), conditional requests. A login check or permission is an extractor, as on any query. Precedents: `get_song` (#433), `get_startlist_csv`, `export_competition`.
4. **No third kind** under `/api`. Everything else is a websocket or a **static asset**, a file that is part of the build (the embedded frontend). See [07](07-non-cqrs-surfaces.md).
5. Every command and query is in the OpenAPI document, so the frontend calls it through the generated client (`frontend/src/ts/gen`), never through a hand-written `fetch`. File queries are the exception in how they're *called*: an `<audio src>`, `<img src>`, CSS `<link>` or download link needs a URL. The frontend builds those URLs in one helper per file query (like `songUrl(file)`), never as string literals in components.

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

### Files from state outside `/api/query`

Under rule 3 these are queries. They sit outside only because they answer with bytes instead of JSON, and rule 3 allows that. Proposed names follow [06](06-naming.md):

| Route today | File | Proposed query | Access |
|---|---|---|---|
| `GET /api/competition/logo` | `routes/logo.rs` | `get_competition_logo` | public |
| `GET /api/competition/break-image` | `routes/logo.rs` | `get_break_image` | public |
| `GET /api/competition/sponsors/{id}/image` | `routes/logo.rs` | `get_sponsor_image?sponsor_id=…` | public |
| `GET /api/competition/colors.css` | `routes/colors.rs` | `get_competition_colors_css` | public |
| `GET /api/photos/{id}` | `routes/photos.rs` | `get_photo?photo_id=…` | public (info board) |
| `GET /api/photos/newest`, `/api/photos/random` | `routes/photos.rs` | see open question Q5 | public |
| `GET /api/db-backups/{filename}` | `routes/db_backups.rs` | `export_db_backup?filename=…` | admin |
| ~~`GET /songs/{file}`~~ | | `get_song?file=…` | logged in (done in #433) |

Keep the ETag and `Cache-Control` behaviour these routes have today (`routes/etag.rs`). A cache-busting version param, like the sponsor image's `?v=`, stays a query param.

### Setup outside `/api/command` and `/api/query`

| Route | File | What it is |
|---|---|---|
| `GET /api/setup/session`, `/api/setup/status` | `src/setup.rs:587` | Setup state machine, not in OpenAPI |
| `POST /api/setup/session`, `/api/setup/test-mail`, `/api/setup/complete` | `src/setup.rs:587` | Setup commands, REST-style (`GET` + `POST` on one path), hand-written `fetch` in `cup-setup*.ts` |

See [07](07-non-cqrs-surfaces.md) for setup.

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
