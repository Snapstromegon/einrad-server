# 07 · Surfaces outside commands and queries

Allowed by the target: **websockets** and **static or statically linked assets**. Everything registered in `src/http_server/routes.rs` and `src/setup.rs` outside `/api/command` and `/api/query`:

| Route | Kind | Verdict |
|---|---|---|
| `/` fallback, `ServeEmbed<StaticAssets>` | Embedded frontend | ✅ static asset |
| `GET /api/ws/player` | Websocket (remote playback) | ✅ websocket. Its message types are in the OpenAPI components (`ControllerMessage`, `ToPlayer`, …), so the frontend gets typed messages. |
| `GET /api/competition/logo`, `/break-image`, `/sponsors/{id}/image` | Images with ETag | ✅ asset |
| `GET /api/competition/colors.css` | Generated CSS with ETag | ✅ asset (derived from state, but cacheable and addressed like a file) |
| `GET /api/photos/{id}` | Image, immutable | ✅ asset |
| `GET /api/photos/newest`, `/api/photos/random` | 302 to `/api/photos/{id}`, `no-store` | ⚠️ a query in disguise: the choice depends on state. Acceptable as an `<img src>` convenience for the info board; document it as the one exception, or replace with `query/get_photo` returning the ID. |
| `GET /api/db-backups/{filename}` | Authenticated binary download | ⚠️ an asset behind auth. Fine, but it's the only asset with a permission check; write that down. |
| `/songs/*` → `ServeDir(data_path)` | Song files | ⚠️ serves the **whole data directory without authentication**: songs (named by act UUID), normalized copies, and anything else written there, e.g. `data_path/remap/jury-registrations-<timestamp>.toml` from `seed.rs:1836` (judge registrations, a guessable file name; check whether a running server ever writes it there). Restrict to song files, or move other files out of `data_path`. Also: not under `/api` and not using the ETag helper the other assets use. |
| `GET /metrics` | Prometheus, bearer token | ✅ operational endpoint; Prometheus defines its shape. |
| `GET /swagger`, `/openapi.json` | API docs | ✅ static (generated at startup) |
| `/api/setup/session` (GET+POST), `/status`, `/test-mail`, `/complete` | First-run setup (ADR 0011), own router state, own `SetupAuth` | ❌ REST-style, not in OpenAPI, called with hand-written `fetch` in `cup-setup.ts` and `cup-setup-wizard.ts`. |

## Setup

The setup router runs before the database exists, so it can't use the normal `Extension` layers or `Granted`. That justifies a **separate router**, not a separate API style. Target:

- `GET /api/setup/query/get_session`, `get_status`; `POST /api/setup/command/start_session` (today `POST /session`, the token exchange), `send_test_mail`, `complete_setup`.
- Added to the OpenAPI document (a second `OpenApiRouter` merged into the spec), so the setup screens use the generated client too.

## Live state

Today only remote playback uses a websocket; everything else polls queries (`PollController` in `frontend/src/ts/api.ts`). That's consistent with CQRS (queries are cheap to repeat). If live updates for the info board, music control or moderation screen move to push later (#377, event sourcing), the rule stays: the websocket pushes *events* or *"query X changed"* notices, the client still reads through queries.
