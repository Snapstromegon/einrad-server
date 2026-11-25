# Architecture

This folder states the target architecture of the server and records where the code drifts from it, so the drift can be worked off piece by piece.

Audit: 2026-11-23, against `main` at `5694f32`.

## Target, in one paragraph

The HTTP API is **CQRS-shaped**: every API call is either a **command** (`POST /api/command/<verb>_<noun>`, JSON body, changes state) or a **query** (`GET /api/query/<get|list>_<noun>`, query-string params, changes nothing). One handler per file, one file per endpoint, registered in `command.rs` / `query.rs`, described by `utoipa` so the frontend client is generated (kubb). The only HTTP surfaces outside that shape are **websockets** and **static assets**: files that are part of the build (the embedded frontend). A file that comes from state, meaning uploaded, generated or stored at runtime (songs, logos, sponsor images, photos, the competition CSS, backups), is a **query for a file**: `GET /api/query/get_<noun>` with params, answering with the file's bytes and content type instead of JSON.

## Files

| File | What it covers |
|---|---|
| [01-cqrs-api.md](01-cqrs-api.md) | The CQRS rules, how well the API meets them, every endpoint that deviates |
| [02-handler-anatomy.md](02-handler-anatomy.md) | The canonical handler shape and where handlers differ from it |
| [03-errors.md](03-errors.md) | `HttpError` / `ClientError`, status codes, messages |
| [04-auth.md](04-auth.md) | `Granted<P>`, `Auth`, access helpers, ad-hoc checks |
| [05-logic-and-persistence.md](05-logic-and-persistence.md) | Where logic and SQL live: handler style vs. helper modules, reconciled with #238/#370 in ADR 0025 |
| [06-naming.md](06-naming.md) | Endpoint verbs, DTO names, OpenAPI tags |
| [07-non-cqrs-surfaces.md](07-non-cqrs-surfaces.md) | Websocket, static assets, files from state, setup, metrics |
| [backlog.md](backlog.md) | Every action item, prioritised, ready to turn into issues |
| [open-questions.md](open-questions.md) | Decisions only the maintainer can make before some items can start |

## Conformance at a glance

| Rule | Conforms | Deviates |
|---|---|---|
| Commands are `POST` under `/api/command` | 97 / 97 | — |
| Queries are `GET` under `/api/query` | 43 / 43 | — |
| Queries don't write | 43 / 43 (no `INSERT`/`UPDATE`/`DELETE`) | — |
| Commands don't return read models | 91 / 97 | 2 previews are queries; 4 return read data (see 01) |
| Non-WS, non-static API outside `/api/{command,query}` | — | `/api/setup/*`; files from state: `/api/competition/{logo,break-image,colors.css,sponsors/{id}/image}`, `/api/photos/*`, `/api/db-backups/{file}` (songs moved in #433) |
| Validation errors get a 4xx | — | `HttpError::ErrorMessages` answers **500** (45 commands) |
| Writes with >1 statement run in one transaction | 61 commands use `begin_write` | 7 multi-statement commands don't |
| Handler holds its own logic (#419) | 79 / 140 route files run their own SQL | 15+ helper modules with SQL (groups, jury, jury_sessions, heats, timeplan, …) |

The CQRS split itself is in good shape. Most of the inconsistency is *inside* the split: error mapping, auth checks, where logic lives, and naming.

## How to use this folder

- Pick an item from [backlog.md](backlog.md), turn it into a GitHub issue (see `docs/agents/issue-tracker.md`), and tick it off here when it's merged.
- When a rule here becomes a decision worth keeping, record it as an ADR in `docs/adr/` and link it from the matching file. The CQRS rule itself has no ADR yet (see open questions).
- Re-run the audit commands at the end of each file to see whether a number moved.
