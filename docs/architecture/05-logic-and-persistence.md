# 05 · Where logic and SQL live

## The intended style (#419)

> It was an intentional architectural decision to have the implementation of all methods in the handler, to see the whole logic there and not build an ORM-like behavior.

So: one command or query = one file, and reading that file tells you everything the endpoint does, SQL included.

## Where the code stands

- **SQL in route files**: 57 of 97 command files and 22 of 43 query files contain `sqlx::query*` calls.
- **Helper modules with SQL**, and how many route files call them:

| Module | `pub async fn`s | Route files using it | Character |
|---|---|---|---|
| `groups.rs` | 7 | 27 | Access checks (`ensure_*_access`) + group rules, returns `HttpError` |
| `jury.rs` | 17 | 26 | Getters (`list_judges`, `plan_assignments`, `list_units_in`, …), returns `HttpError` |
| `competition.rs` | 5 | 21 | Settings getters |
| `images.rs` | 12 | 15 | Image storage |
| `timeplan.rs` | 11 | 13 | Timeplan rules |
| `heats.rs` | 11 | 10 | Getters (`name_of`, `code_of`, `categories`, `finals_of`) + validation |
| `finals.rs` | 6 | 9 | |
| `jury_sessions.rs` | 18 | 8 | Whole commands (`assign`, seat, split), 1300 lines |
| `rider_registration.rs` | 9 | 8 | Returns `HttpError` |
| `bookings.rs` | 6 | 7 | |
| `fees.rs`, `fee_waivers.rs`, `people.rs`, `club_mails.rs`, `normalize.rs` | 1–7 | 1–6 | |

- **A domain module inside the route tree**: `query/predict_timeplan.rs` (567 lines, `pub(crate)`) is called by `get_startlist_csv`, `src/jury.rs` and `src/metrics.rs`.

The helper modules come in two kinds, and #419 treats them alike although they differ:

1. **ORM-like getters and setters** (`heats::name_of`, `heats::code_of`, `jury::list_judges`, `competition::*` getters, `fee_waivers::set_waiver`). These hide one `SELECT` or `UPDATE` behind a name. This is exactly what #419 wants gone.
2. **Shared domain rules and pure functions** (`heats::normalize_name`, `groups::normalize_group_name`, `timeplan` prediction, `fees` calculation, `ensure_*_access`). Inlining these would copy the rule into every handler that needs it, and then the copies diverge. These should stay shared, but **pure** (no SQL, no `HttpError`) or, for access checks, move to the extractor layer ([04](04-auth.md)).

`jury_sessions.rs` is a third kind: whole command bodies moved out of their handlers. Under #419 they move back into `command/assign_judge.rs`, `seat_judge.rs`, `set_jury_sessions.rs`, … .

## Decided: handler now, one trait method per endpoint later

[ADR 0025](../adr/0025-each-endpoint-owns-its-logic-a-db-trait-gets-one-method-per-endpoint.md) reconciles #419 with the Postgres plan (#238), multi-event (#370) and ADR 0024 (on the `multi-event-planning` branch). The trait will have **one method per command and per query, not per table**:

```
command/assign_judge.rs             → HTTP: extractors, auth, body → db.assign_judge(cmd) → response
db-sqlite/commands/assign_judge.rs  → that command's SQL, in one transaction
```

Until the trait exists, refactors under #419 move SQL **into** the handlers, create **no** new cross-handler SQL helpers, and leave pure rules in tested domain modules.

## Rules either way

- No module outside `http_server/` returns `HttpError` or imports `axum`. Domain functions return a domain error (or `sqlx::Error`) and the handler maps it.
- No route file imports another route file. Shared code lives outside `routes/`.
- Read-model row structs (`DB<Thing>`) live at module level of the file that runs the query.
- Pure rules (normalization, validation, fee math, timeplan prediction) are pure functions with unit tests next to them.

## Audit commands

```bash
# Route files with SQL
grep -lE 'sqlx::query' src/http_server/routes/{command,query}/*.rs | wc -l
# Domain modules leaking HTTP
grep -lE 'HttpError|axum::' src/*.rs
# Route-to-route imports
grep -rnE 'routes::(query|command)::' src --include=*.rs
```
