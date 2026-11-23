# 02 · Handler anatomy

## The canonical command

The majority shape, taken from `command/set_act_withdrawn.rs` and `command/assign_judge.rs`:

```rust
#[derive(Debug, Serialize, ToSchema)]
pub struct SetActWithdrawnResponse {}

#[derive(Debug, Deserialize, ToSchema)]
pub struct SetActWithdrawnBody { act_id: ActId, withdrawn: bool }

/// What the command does, in domain words. Who may call it.
#[utoipa::path(
    post,
    tags=["command", "<area>"],
    path="/set_act_withdrawn",
    request_body=SetActWithdrawnBody,
    responses(
        (status=200, content_type="application/json", body=SetActWithdrawnResponse),
        (status=4xx/500, content_type="application/json", body=ClientError),
    ),
)]
#[instrument(skip(db))]
#[axum::debug_handler]
pub async fn set_act_withdrawn(
    Extension(db): Extension<ReloadableSqlite>,
    _: Granted<permit::ManageRegistrations>,
    Json(body): Json<SetActWithdrawnBody>,
) -> Result<Json<SetActWithdrawnResponse>, HttpError> {
    let db = db.get().await.clone();
    let mut transaction = begin_write(&db).await?;
    info!("…");
    // checks, then writes
    transaction.commit().await?;
    Ok(Json(SetActWithdrawnResponse {}))
}
```

The canonical query is the same without `debug_handler`, `begin_write` and `info!`, with `params(<Name>Query)` and `Query(query): Query<<Name>Query>` when it takes params.

## Where handlers differ

| Aspect | Majority | Deviations |
|---|---|---|
| `#[axum::debug_handler]` | 91 / 97 commands | Only 1 / 43 queries. It's a compile-time aid only; pick one rule (all handlers, or none). |
| `#[instrument(skip(db))]` | All 140 handlers have `instrument` | 18 different `skip(...)` lists. See **secrets in spans** below. |
| Transaction for multi-statement writes | 61 commands use `begin_write` | Multiple `sqlx::query` calls without one: `register`, `rename_club`, `request_password_reset`, `resend_mail_validation`, `reset_password`, `save_act_song`, `verify_email`. A crash between statements leaves half a write. |
| `info!` log line per command | 58 / 97 | 39 commands log nothing (list via the audit command). Either every command logs what it did, or none does and the span is enough. |
| Request type name | `<Name>Body` (80 commands), `<Name>Query` (13 queries) | `BookingInput`, `JuryUnitFields`, `ClubMailInput`, `CompetitionFileBody` are shared across commands (fine, but the rule should say so); `ReloadDBQuery` and `SaveActSongQuery` are query params on commands. |
| Response type | `Json<<Name>Response>` | `preview_club_mail` → `PreparedMails`, `send_club_mail` → `SentClubMail`, `set_jury_sessions` → `SplitResult`, `preview_competition_import` → bare `CompetitionFile`/`Preview`: domain types leak into the API contract, so a domain rename changes the generated client. |
| DB-row structs | Named `DB<Thing>` at module level | `query/startlist.rs` declares `DBStartlistAct` and its `From` impl *inside* the handler body. |
| Handler → handler calls | None | `query/get_startlist_csv.rs`, `src/jury.rs:23` and `src/metrics.rs:37` import `routes::query::predict_timeplan::predict`. The 567-line `predict_timeplan.rs` is a domain module living in the route tree (`pub(crate) mod`). `src/timeplan.rs` already exists for that. |
| Unit tests in handler files | None, tests live in `tests/` | `query/get_startlist_csv.rs` has a `mod tests`. Fine for a pure helper (`csv_field`), but note it. |
| `.unwrap()` in handlers | — | `save_act_song.rs` (`path.parent().unwrap()`), `get_startlist_csv.rs` (format `unwrap`, `expect("Invalid Offset")`). |
| Query snapshot | One connection per query | `query/juryplan.rs` reads with `conn`, drops it, then reads more through the pool: two reads may see different states. Queries assembling several reads should use one read transaction. |

## Secrets in spans (security)

`#[instrument(skip(db))]` records every other argument with `Debug`, which includes `Json(body)`. Bodies with secrets derive `Debug`:

- `command/login.rs`: `LoginBody { password }`; on an unknown email the handler logs `tracing::error!` inside the span, and the `FmtSubscriber` prints the span fields, so the typed password lands in the log.
- `command/register.rs`: `password`.
- `command/reset_password.rs`: `new_password` (plus the reset token).
- `command/verify_email.rs`: the verification `token`.
- `command/reload_db.rs`: the reload `token` query param.

Fix: `skip_all` plus explicit `fields(...)`, or a manual `Debug` that redacts. Then make "bodies with secrets never derive `Debug`" a rule.

## Audit commands

```bash
cd src/http_server/routes
# Commands without a transaction but with several statements
for f in command/*.rs; do n=$(grep -c 'sqlx::query' $f); grep -q begin_write $f || [ $n -lt 2 ] || echo "$f ($n)"; done
# Commands without an info! line
for f in command/*.rs; do grep -q 'info!' $f || echo $f; done
# instrument variants
grep -ohE '#\[instrument[^]]*\]' command/*.rs query/*.rs | sort | uniq -c
```
