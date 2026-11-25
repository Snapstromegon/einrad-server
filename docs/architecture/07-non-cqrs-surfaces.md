# 07 · Surfaces outside commands and queries

Allowed by the target: **websockets** and **static assets**, files that are part of the build. A file that comes from state (uploaded, generated or stored at runtime) is **not** an asset. It's a query for a file ([01](01-cqrs-api.md), rule 3), even when it's binary, cacheable or loaded by an `<img>`/`<audio>`/`<link>`.

The test: if the bytes can change without a new release, it's a query.

Everything registered in `src/http_server/routes.rs` and `src/setup.rs` outside `/api/command` and `/api/query`:

| Route | Kind | Verdict |
|---|---|---|
| `/` fallback, `ServeEmbed<StaticAssets>` | Embedded frontend build | ✅ static asset |
| `GET /swagger`, `/openapi.json` | API docs, generated from the code at startup | ✅ static (changes only with a release) |
| `GET /api/ws/player` | Websocket (remote playback) | ✅ websocket. Its message types are in the OpenAPI components (`ControllerMessage`, `ToPlayer`, …), so the frontend gets typed messages. |
| `GET /api/competition/logo`, `/break-image`, `/sponsors/{id}/image` | Images from the database, with ETag | ❌ file from state, becomes a query (see the table in [01](01-cqrs-api.md)) |
| `GET /api/competition/colors.css` | CSS generated from the competition's colors, with ETag | ❌ file from state, becomes a query |
| `GET /api/photos/{id}` | Photo from the photo source | ❌ file from state, becomes a query |
| `GET /api/photos/newest`, `/api/photos/random` | 302 to `/api/photos/{id}`, `no-store` | ❌ file from state; the shape is open question Q5 |
| `GET /api/db-backups/{filename}` | Admin download of a backup file | ❌ file from state, becomes `export_db_backup` |
| ~~`/songs/*` → `ServeDir(data_path)`~~ | Song files | ✅ moved to `get_song` with login and a song-name allowlist (#433) |
| `GET /metrics` | Prometheus text, bearer token | ⚠️ derived from state, but its path and format are set by Prometheus. Proposed as the one documented exception, see open question Q7. |
| `/api/setup/session` (GET+POST), `/status`, `/test-mail`, `/complete` | First-run setup (ADR 0011), own router state, own `SetupAuth` | ❌ REST-style, not in OpenAPI, called with hand-written `fetch` in `cup-setup.ts` and `cup-setup-wizard.ts`. |

## Files from state as queries

What a file query keeps compared to a JSON query:

- **Bytes and content type**, declared in OpenAPI as a binary body per content type (`get_song` declares `audio/mpeg` and `audio/wav`).
- **HTTP file semantics**: `ServeFile` for files on disk (Range/206, `Last-Modified`, conditional requests); the existing ETag helper (`routes/etag.rs`) for bytes from the database.
- **Caching**: `Cache-Control` as today. The response header doesn't care whether the path says `query`.
- **A URL helper in the frontend** per file query (`songUrl(file)`), because `<img src>`, `<audio src>` and `<link href>` need a URL, not a client call.

## Setup

The setup router runs before the database exists, so it can't use the normal `Extension` layers or `Granted`. That justifies a **separate router**, not a separate API style. Target:

- `GET /api/setup/query/get_session`, `get_status`; `POST /api/setup/command/start_session` (today `POST /session`, the token exchange), `send_test_mail`, `complete_setup`.
- Added to the OpenAPI document (a second `OpenApiRouter` merged into the spec), so the setup screens use the generated client too.

## Live state

Today only remote playback uses a websocket; everything else polls queries (`PollController` in `frontend/src/ts/api.ts`). That's consistent with CQRS (queries are cheap to repeat). If live updates for the info board, music control or moderation screen move to push later (#377, event sourcing), the rule stays: the websocket pushes *events* or *"query X changed"* notices, the client still reads through queries.
