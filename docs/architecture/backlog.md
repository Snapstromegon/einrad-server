# Backlog

Ordered by priority within each tier. Each item is sized for one issue / one PR. "Depends" names an [open question](open-questions.md). Tick an item when its PR is merged.

## P0 · Security

- [ ] **Stop logging secrets in spans.** `login`, `register`, `reset_password`, `verify_email`, `reload_db` record passwords/tokens through `#[instrument(skip(db))]`. Use `skip_all` + explicit `fields`, or redact `Debug`. Add a test that the login span doesn't contain the password. → [02](02-handler-anatomy.md#secrets-in-spans-security)
- [ ] **Restrict `/songs`.** It serves all of `data_path` without auth. Serve only song files (by act ID) or move everything else out of `data_path`; check `data_path/remap/`. → [07](07-non-cqrs-surfaces.md)
- [ ] **Stop leaking DB/template errors to clients.** `DBError`/`TemplateError` go out as `Generic("Database error: …")`. Log the cause, answer a plain 500. → [03](03-errors.md)

## P1 · Correctness

- [ ] **Validation errors answer 4xx, not 500.** Replace `HttpError::ErrorMessages` (45 commands) with an `Invalid(String)` variant; send messages with `Display`, then delete `undebug` in `frontend/src/ts/api.ts`. Depends: Q6. → [03](03-errors.md)
- [ ] **Transactions for multi-write commands.** `register`, `rename_club`, `request_password_reset`, `resend_mail_validation`, `reset_password`, `save_act_song`, `verify_email` write several statements without `begin_write`; wrap each in one transaction. → [02](02-handler-anatomy.md)
- [ ] **One snapshot per composite query.** `juryplan` (and any query that drops its connection between reads) reads in one transaction. → [02](02-handler-anatomy.md)
- [ ] **OpenAPI responses match reality.** Every handler with `Auth`/`Granted` declares 401 and 403; `login` declares `ClientError`, not `String`; `reload_db`/`save_act_song` drop the copied `request_body=Upload`. Consider a shared responses helper. → [03](03-errors.md)

## P2 · Consistency (no API change)

- [ ] **Collapse the error vocabulary** to one variant per meaning (`NotFound(Option<String>)`, `Forbidden(Option<String>)`, …); remove `HttpError::StatusCode`. → [03](03-errors.md)
- [ ] **Typed phase gates.** Replace the 25 inline `capabilities.can_*` checks with extractors like `JuryEnabled`, each with a German 403 message. → [04](04-auth.md)
- [ ] **Move access checks out of `groups.rs`** into `http_server/extractor/` (or `access.rs`). → [04](04-auth.md)
- [ ] **Move `predict_timeplan` out of the route tree.** The prediction goes to `src/timeplan.rs`; `query/predict_timeplan.rs` keeps only the handler. Removes the route→route imports in `get_startlist_csv`, `jury.rs`, `metrics.rs`. → [05](05-logic-and-persistence.md)
- [ ] **No `HttpError`/`axum` in domain modules** (`groups`, `jury`, `jury_sessions`, `rider_registration`, `heats`, `people`). → [05](05-logic-and-persistence.md)
- [ ] **Uniform handler attributes**: `debug_handler` on all handlers or none; one `instrument` convention; one `info!` per command or none. → [02](02-handler-anatomy.md)
- [ ] **Remove `.unwrap()`/`expect` from handlers** (`save_act_song`, `get_startlist_csv`).
- [ ] **Hoist `DBStartlistAct`** out of the `startlist` handler body.

## P3 · #419 handler style

Decided in ADR 0025. One PR per module, smallest first. Each PR moves the module's SQL getters and setters back into the handlers that call them, keeps or extracts the pure rules (no SQL, no `HttpError`, unit-tested), and deletes what's left.

- [ ] `fee_waivers.rs` (1 caller) · [ ] `fees.rs` (2) · [ ] `people.rs` (2) · [ ] `bookings.rs` (7) · [ ] `rider_registration.rs` (8) · [ ] `jury_sessions.rs` (8) · [ ] `finals.rs` (9) · [ ] `heats.rs` (10) · [ ] `timeplan.rs` (13, keep the pure prediction) · [ ] `jury.rs` (26) · [ ] `groups.rs` (27, after the access-check move)

## P4 · API shape (breaking, batch into one release)

Depends: Q2–Q5. Regenerate the client once per batch; not during a running event.

- [ ] **Setup into the CQRS shape + OpenAPI**, generated client in `cup-setup*.ts`. → [07](07-non-cqrs-surfaces.md)
- [ ] **Previews**: settle the "query with body" rule, move/rename `preview_club_mail`, `preview_competition_import`. Depends: Q3.
- [ ] **Commands stop returning read models**: `set_image_source`, `refresh_images`, `send_club_mail`, `restore_db_backup`; drop echoed IDs in `rename_club`, `set_club_responsible`. Depends: Q4. → [01](01-cqrs-api.md)
- [ ] **Request shape**: `save_act_song` takes `act_id` in the form, `reload_db` takes its token as a header (or drops it). → [01](01-cqrs-api.md)
- [ ] **Renames**: CRUD verbs, `timeplan_forward/backward`, `juryplan`/`startlist` → `get_public_*`, `get_startlist_csv` → `export_startlist_csv`. → [06](06-naming.md)
- [ ] **Tags**: one singular aggregate tag per endpoint. Not breaking for the client; can go first. → [06](06-naming.md)
- [ ] **Response types own their names**: wrap `PreparedMails`, `SentClubMail`, `SplitResult`, `Preview` in `<Handler>Response`. → [02](02-handler-anatomy.md)
- [ ] **`whoami` through the generated client** in `cup-context-user.ts` and `cup-view-setup.ts`.
- [ ] **Songs into a file query**: `get_song` with login and a song-name allowlist. In #433; tick when merged.
- [ ] **Competition images and CSS into file queries**: `get_competition_logo`, `get_break_image`, `get_sponsor_image`, `get_competition_colors_css`. Keep the ETags and `Cache-Control`; add a frontend URL helper each; update the info board, the admin views and the `<link>` in `index.html`. → [01](01-cqrs-api.md), [07](07-non-cqrs-surfaces.md)
- [ ] **Photos into file queries**: `get_photo?photo_id=…`, plus newest/random in the shape Q5 picks. Depends: Q5.
- [ ] **Backup download into a file query**: `export_db_backup?filename=…`, admin only, as today.
- [ ] **ADR for the CQRS rule.** Depends: Q2.
