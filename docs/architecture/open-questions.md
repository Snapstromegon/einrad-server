# Open questions

Decisions for the maintainer. Items in [backlog.md](backlog.md) that depend on one name it.

## Q1 · Handler style (#419) vs. `Db` trait (#238, #370) · ✅ decided 2026-11-23

**Decided: option A**, recorded in [ADR 0025](../adr/0025-each-endpoint-owns-its-logic-a-db-trait-gets-one-method-per-endpoint.md):

- #419 now: each handler owns its whole logic and SQL; ORM-like per-table helpers move back into their handlers.
- Shared rules stay shared **if pure** (no SQL, no `HttpError`), with unit tests. Access checks move to extractors.
- When the Db trait comes, each handler's SQL moves into one trait method named after the endpoint, **for queries as well as commands**. Rules buried in SQL are pulled out as pure functions on the way, because backends only load facts and write (#238).

Rejected: B (skip #419, trait now) and C (keep the helper modules).

## Q2 · A CQRS ADR

The CQRS rule has no ADR. Record one in `docs/adr/` from [01](01-cqrs-api.md) and [07](07-non-cqrs-surfaces.md)? It would also settle Q3–Q5.

## Q3 · Read-only requests with a large body

`preview_club_mail` and `preview_competition_import` are queries that need a body. Options: keep them as `POST /api/command/preview_*` with a written exception; a `POST /api/query/*` category for "query with body"; or `QUERY` method (not supported by browsers' `fetch` everywhere yet).

## Q4 · What a command may return

Strict (`{}` only, client-minted UUIDv7 IDs as ADR 0018 allows) or pragmatic (created ID, warnings, outcome enum; no read models)? The audit assumes pragmatic.

## Q5 · `/api/photos/newest|random`

Decided 2026-11-25 that files from state are queries ([01](01-cqrs-api.md), rule 3), so these become queries too. Open: their shape.

- **A.** File queries that redirect: `get_newest_photo` / `get_random_photo` answer 302 to `get_photo?photo_id=…`, as today. The info board keeps one `<img src>` URL. *Recommended*: least change for the info board, and `get_photo` stays cacheable.
- **B.** JSON queries returning the photo ID; the info board then sets `src` to `get_photo?photo_id=…` itself.
- **C.** One query, `get_photo?pick=newest|random|<id>`.

## Q6 · Error status for validation

`400` or `422` for invalid input? The frontend already has messages for both (`frontend/src/ts/api.ts`).

## Q7 · `/metrics`

`GET /metrics` is derived from state, so rule 3 would make it a query. But Prometheus scrapes a fixed path and format with a bearer token, and nothing in the app calls it. Keep it as the one documented exception (*recommended*), or move it to `/api/query/get_metrics` and point the scrape config there (`deploy/`)?
