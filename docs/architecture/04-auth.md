# 04 · Authorization

## The three layers that exist

1. **Organizer permission**: `Granted<permit::X>` (`src/http_server/extractor/auth.rs`). Rejects with 401/403 before the handler runs. `Option<Granted<P>>` for handlers whose answer depends on the viewer. Used in 92 handlers. Good: typed, declarative, visible in the signature (ADR 0003).
2. **Club scope**: `auth: Auth` plus `groups::ensure_club_access` (24 calls), `ensure_rider_access` (17), `ensure_act_access` (6), `ensure_group_owner` (6), `jury::ensure_judge_access` (2). Runs inside the handler.
3. **Feature / phase gates**: typed extractors for some (`JuryEnabled`, `StartlistAccess`, `ActListAccess`, `JuryPlanAccess`), but an inline boolean for others:

   ```rust
   capabilities: Capabilities,
   …
   if !capabilities.can_register_rider {
       return Err(HttpError::StatusCode(StatusCode::FORBIDDEN));
   }
   ```

   `can_register_rider` ×16, `can_register_judge` ×5, `can_upload_music` ×2, `can_register` ×1, `can_create_club` ×1.

## Inconsistencies

| Issue | Where | Target |
|---|---|---|
| Phase gates are half typed, half inline | 25 inline `capabilities.can_*` checks vs. `JuryEnabled` & co. | One extractor per gate (`RegistrationOpen`, `JudgeRegistrationOpen`, `MusicUploadOpen`, …), like `JuryEnabled`. The 403 gets a German message saying *why* ("Anmeldung geschlossen"). |
| Bare 403 without a reason | 26 `StatusCode(FORBIDDEN)` | `Forbidden(Some(msg))` (see [03](03-errors.md)). |
| Club-scope helpers live in `groups.rs` | `ensure_club_access`, `ensure_rider_access` are not about groups | Move to `extractor/` or an `access.rs`; they're authorization, not group logic. |
| Business rules inside "auth" | `set_user_roles` refuses removing your own admin role with a bare 403 | That's a domain rule, so it's a `Conflict`/`Invalid` with a message, not a permission error. |
| Reload token | `reload_db` accepts `?token=` on `POST` and then requires a login anyway | Decide whether the token is a credential (then `Authorization: Bearer`, like `/metrics`) or not needed. |
| The setup flow has its own auth | `SetupAuth` in `src/setup.rs:357` on its own router state | Acceptable while setup is a separate state machine (ADR 0011); note it in [07](07-non-cqrs-surfaces.md). |
| `Auth` loads the user per request | `extractor/auth.rs` runs a `sqlx::query!` | Fine today; it's the one SQL statement outside handlers/helper modules besides `utils.rs`, so it moves with whatever [05](05-logic-and-persistence.md) decides. |

## Rule to adopt

> Every precondition that doesn't depend on the request body is an extractor in the handler signature. Checks that depend on the body (which club, which rider) are `ensure_*` calls at the top of the handler, before any write.
