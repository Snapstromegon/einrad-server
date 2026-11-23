# 06 · Naming

The route name is the handler name is the file name is the generated client function. That chain holds everywhere; what drifts is the **vocabulary**.

## Command verbs in use

`set` 23 · `edit` 11 · `delete` 9 · `add` 7 · `move` 5 · `create` 5 · `remove` 3 · `rename` 2 · `reset` 2 · `request` 2 · `preview` 2 · one each of `update`, `upload`, `save`, `clear`, `reorder`, `refresh`, `restore`, `resolve`, `resend`, `replace`, `assign`, `unassign`, `seat`, `link`, `leave`, `dissolve`, `apply`, `import`, `send`, `verify`, `register`, `login`, `logout`, `reload` · none: `timeplan_forward`, `timeplan_backward`.

Domain verbs (`seat`, `link`, `dissolve`, `withdraw`, `register`, `verify`) are good: they name what happens in the competition. The noise is in the CRUD verbs, which have synonyms:

| Meaning | Used | Proposal |
|---|---|---|
| Make a new entity | `add_category`, `add_heat`, `add_final`, `add_rider`, `add_sponsor`, `add_timeplan_entry`, `add_club_judge` / `create_club`, `create_club_group`, `create_booking`, `create_competition`, `create_db_backup` | One of them. `create_` reads better for top-level entities, `add_` for entries in a list; if both stay, write the rule down. |
| Remove | `delete_*` ×9 / `remove_sponsor`, `remove_competition_logo`, `remove_break_image` / `clear_act_song` | `delete_` for entities, `remove_`/`clear_` for an attribute (logo, song). That's what the code does; write it down. |
| Change fields | `edit_*` ×11 / `update_sponsor` | `edit_` (rename `update_sponsor`). |
| Change one fact | `set_*` ×23, `rename_*`, `save_act_song`, `upload_sponsor_image` vs `set_competition_logo`, `set_break_image` | `set_` for one fact; `upload_sponsor_image` → `set_sponsor_image`, `save_act_song` → `set_act_song` (pairs with `clear_act_song`). |
| Reorder | `move_category_up/down`, `move_timeplan_up/down`, `reorder_sponsors`, `set_act_order`, `set_block_order`, `reset_block_order` | Two patterns are fine (step vs. whole list), but name them the same way: `move_<x>_up/down` and `set_<x>_order`. `reorder_sponsors` → `set_sponsor_order`. |
| Shift the timeplan | `timeplan_forward`, `timeplan_backward` | Missing verb: `shift_timeplan_forward/backward`. |

## Query names

`list_*` ×26, `get_*` ×11, and bare: `startlist`, `juryplan`, `whoami`, `predict_timeplan`, `check_db_backup`, `export_competition`.

| Issue | Proposal |
|---|---|
| `startlist` and `juryplan` are the *public* views; `list_jury_plan` is the club/admin view | `get_public_startlist`, `get_public_jury_plan`; the pair `juryplan` / `list_jury_plan` is the most confusing name in the API. |
| `get_startlist_csv`, `export_competition` return files | Fine as queries; prefix rule: `export_*` for downloads. `get_startlist_csv` → `export_startlist_csv`. |
| `list_*` vs `get_*` | `list_` returns a collection, `get_` one thing or a composite. `get_structure`, `get_competition_settings`, `get_system_status` follow it; keep. |

## DTO names

| Kind | Rule in practice | Deviations |
|---|---|---|
| Command body | `<Handler>Body` | Shared inputs `BookingInput`, `JuryUnitFields`, `ClubMailInput`, `CompetitionFileBody` |
| Query params | `<Handler>Query` | none in queries; `ReloadDBQuery`, `SaveActSongQuery` are on commands |
| Response | `<Handler>Response` | Domain types returned directly (see [02](02-handler-anatomy.md)); `ReloadDBResponse`/`ReloadDBQuery` spell `DB` while `CreateDbBackupResponse` spells `Db` |

## OpenAPI tags

First tag is always `command` or `query`: consistent. The second tag drifts:

- `act` (`get_act`) vs `acts` (`list_acts`, `startlist`).
- `group` (`list_groups`) vs `club`+`group` (`list_club_groups`, the five group commands).
- 5 queries carry no area tag: `list_riders`, `get_system_status`, `list_categories`, `list_timeplan`, `list_pair_requests`.
- `reload_db` is tagged `club`; `set_act_withdrawn` is tagged `admin` while other act commands are tagged `club`.
- `admin` is used both as an area and as an audience.

Proposal: second tag = **aggregate** (`act`, `rider`, `club`, `group`, `category`, `heat`, `final`, `timeplan`, `jury`, `booking`, `competition`, `user`, `system`), singular, exactly one. Audience follows from the permission, not a tag.

## Renames are breaking

Every rename changes the generated client and any open browser tab. Batch them, regenerate (`gen_openapi` + kubb), and ship in one release; the competition calendar decides when (not during a running event).
