# 03 · Errors

## Today

`src/http_server.rs` has two enums:

- `HttpError` (what handlers return; `From<sqlx::Error>`, `From<MailError>`, `From<askama::Error>`).
- `ClientError` (what goes over the wire, `{ "type": …, "message": … }`), with the status code picked in `IntoResponse`.

Handlers reach for five different ways to say "no":

| Way | Uses | Status | Message for the user |
|---|---|---|---|
| `HttpError::ErrorMessages(String)` | 45 commands, 2 queries | **500** (becomes `ClientError::Generic`) | German, but `Debug`-quoted (`"\"Text\""`) |
| `HttpError::StatusCode(StatusCode::X)` | 53 commands, 1 query (FORBIDDEN ×26, NOT_FOUND ×30, BAD_REQUEST ×10, …) | X | none |
| `HttpError::Conflict(String)` | 8 commands | 409 | German |
| `HttpError::NotFound` | 15 files | 404 | none |
| `HttpError::ForbiddenMessage` / `NotFoundMessage` | 1 / 0 handlers (more in helper modules) | 403 / 404 | German |

## Problems

1. **Validation errors answer 500.** `ErrorMessages` is the variant for "your input is wrong" (`normalize_name(...).map_err(HttpError::ErrorMessages)`, `check_password`, …), but `ClientError::Generic` maps to `INTERNAL_SERVER_ERROR`. Monitoring counts every typo as a server fault, and the client can't tell a bug from a rejected input.
2. **Messages are `Debug`-formatted.** `ClientError::Generic(format!("{e:?}"))` wraps the message in quotes and escapes; `frontend/src/ts/api.ts` has an `undebug` helper to undo it.
3. **Internals leak.** `DBError` and `TemplateError` become `Generic("Database error: …")` with the full `sqlx::Error` `Debug`, which names tables, columns and constraints.
4. **Two parallel vocabularies.** `NotFound` vs `StatusCode(NOT_FOUND)` vs `NotFoundMessage`; `StatusCode(FORBIDDEN)` vs `ForbiddenMessage`. Which one a handler uses depends on who wrote it.
5. **OpenAPI drifts from reality.** `status=401` is declared in 22 queries but only 10 commands, although every `Granted`/`Auth` handler can answer 401. `login` declares `401 body=String`.
6. **Domain modules return `HttpError`.** `groups.rs`, `jury.rs`, `jury_sessions.rs`, `rider_registration.rs`, `heats.rs`, `people.rs`, `setup.rs` construct HTTP errors, so their logic can't be reused outside a request (seed, CLI, a future `Db` trait) without dragging axum in.

## Target

One variant per *meaning*, each with an optional German message, each with exactly one status:

| Meaning | Variant (proposal) | Status |
|---|---|---|
| Input invalid | `Invalid(String)` | 400 (or 422) |
| Not logged in | `Unauthenticated` | 401 |
| Not allowed | `Forbidden(Option<String>)` | 403 |
| Not found | `NotFound(Option<String>)` | 404 |
| Clashes with state | `Conflict(String)` | 409 |
| Too large | `TooLarge` | 413 |
| Our fault | `Internal` (log the cause, send no detail) | 500 |

Messages go out as plain strings (`Display`, not `Debug`). `StatusCode(..)` and `ErrorMessages(..)` disappear. A `#[derive(IntoResponses)]`-style helper or a shared `responses(...)` macro keeps the OpenAPI list identical for every handler with the same extractors.

## Audit commands

```bash
cd src/http_server/routes
grep -ohE 'HttpError::[A-Za-z]+' command/*.rs query/*.rs ../../*.rs | sort | uniq -c | sort -rn
grep -L 'status=401' command/*.rs query/*.rs | xargs grep -lE 'Granted<|: Auth'
```
