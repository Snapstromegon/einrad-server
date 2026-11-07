# Decisions

Answers to the questions of the 2027 planning, kept as a record. The plans, open questions and todos they came from moved into GitHub issues on 2026-10-22 (#223–#238; the jury decisions are in #225, the result import in #235, FinTS in #237, PostgreSQL in #238). Open work is tracked there, not here.

Status: **done** = implemented on branch 2027 · **applies** = decided for an item that isn't built yet.

## Real event (RE), answered 2026-10-13

| ID | Question | Decision | Status |
|---|---|---|---|
| RE-1 | Age day: the announcement (12.02.2027 for singles and pairs, 01.06.2026 for groups) or the Konzept's 2027 rule? | 01.06.2027 for everyone. | done (no change needed) |
| RE-2 | Finals: in the app or as custom timeplan entries? | Only reserve finalist slots („1. Finalist", „2. Finalist", …). Two finals each for Einzel M, Einzel W and Paar: the first with the best 5 of U11, U13 and U15; the second with the best 4 of U17, U19 and U21 plus the best 4 of U23 and up. Each final has one warm-up. Which age classes run on Friday or Saturday isn't known beforehand, so the days must be plannable in the app. | applies (item "Finals planning") |
| RE-3 | Warm-up model: block, rolling or mixed? | Block warm-up per category as today; small consecutive categories share one via Einfahrzeit 0. | done (no change needed) |
| RE-4 | Payment deadline and account | The account (Peter Kaufmann, the 2025 IBAN) stays. The deadline is still open, so it's a setting. | done (`[competition.payment] deadline` is optional) |
| RE-5 | Do replacement riders pay and count for the group's age? | Yes, both. | done |
| RE-6 | Jury: cross-club groups and the 7 € fee | Show the expected mandatory minimums, configurable. An act counts for the owner club's jury requirement. Postponed to the judge item. | applies (judge item) |
| RE-7 | Withdrawals after the Meldeschluss | The fee stays unless an admin waives it. | done (per act: `set_act_withdrawn`) |
| RE-8 | Production `base_url` | Not fixed in the code, since it changes per event. This time `anmeldung.lmfreestyle27.de`. | done (`BASE_URL` per deployment) |
| RE-9 | Cross-club pairs only by admins | That's enough. | done (no change) |

## Judge registration (J)

| ID | Question | Decision | Status |
|---|---|---|---|
| J-3 | Roles T/P/A, and per discipline? | T/P/A stay, always per category. | applies (judge item) |

## Empty categories (EC), answered 2026-10-13

| ID | Question | Decision | Status |
|---|---|---|---|
| EC-1 | Shared warm-ups (Einfahrzeit 0) | Still used, when two small age groups start one after the other (e.g. two categories with one starter each). An empty category's warm-up therefore moves to the next non-empty category sharing it. | done (`timeplan::effective_warmups`) |
| EC-2 | Empty blocks in the music control sidebar | Hide them. | done |
| EC-3 | Does a category without checked-in riders count as empty? | No: check-in stays advisory, acts are withdrawn explicitly. | done |

## Payments (PAY), answered 2026-10-13

| ID | Question | Decision | Status |
|---|---|---|---|
| PAY-1 | A club code in the reference? | Yes, e.g. `Startgeld: SSV Nümbrecht (EV-K7Q3)`. The QR code's reference also contains „QR", so its use can be detected. | done (`utils::club_code`, `epc.ts`) |
| PAY-2 | QR code at the check-in desk? | No: payment is due long before the event. | done (not shown there) |
| PAY-3 | Text for overpayments | „Meldet euch bitte bei <Kontakt-Mail>", with the mail address as a setting. | done (`contact_email`) |
| PAY-4 | Part 2 (FinTS): go, and the spike? | Later; skip for now. The open FinTS questions are in #237 (FIN). | applies (FinTS postponed) |

## Loudness-normalized music (MUS), answered 2026-10-13

| ID | Question | Decision | Status |
|---|---|---|---|
| MUS-1 | Target loudness | −14 LUFS. | done |
| MUS-2 | ffmpeg on the production host? | Assume it is, and document the requirement in the README. | done |
| MUS-3 | What do clubs see? | A note that the music gets normalized, and players for both versions. | done |

## Prometheus metrics (MET), defaults built 2026-10-13

Not answered; built with the plan's defaults, which can still be changed.

| ID | Question | Decision | Status |
|---|---|---|---|
| MET-1 | Who scrapes? | Any Prometheus that can reach the server: the hosted one over https, or one on a laptop at the event. The README has a scrape config. | done |
| MET-2 | Protection | Bearer token from `metrics_token` / `METRICS_TOKEN`; without a token the endpoint doesn't exist (404). | done |
| MET-3 | Money and club labels | Both: payments received, fees due (the fee rule now also exists in Rust) and starters per club. | done |

## Waveform (WAV), built 2026-10-13

| ID | Question | Decision | Status |
|---|---|---|---|
| – | Where the waveform comes from | Decoded in the browser (Web Audio), no server involved (your call). | done (`cup-waveform`, `waveform.ts`) |
| WAV-1 | Where | Music control and the acts overview's player (default). | done |
| WAV-2 | Seeking | Click or drag on the waveform (default). | done |
| WAV-3 | Markers | The loudest 5 s (highest RMS energy) highlighted (background and bars) and marked with a line and a chip below that jumps there, silence at the start and end shaded; replaced the last-30-s highlight (your call, 2026-10-13). | done |
| WAV-4 | Act time | The act timer counts from the act's first music start (`acts.music_started_at`), not from "Weiter"; acts without music from "Weiter". The waveform marks the category's act time from the song's start with a dashed line and shades the rest red, in the music control and the acts overview (your call, 2026-10-13). | done |

## Remote music playback (RMP), built 2026-10-13

| ID | Question | Decision | Status |
|---|---|---|---|
| – | Player types | Web player, server player and CLI player (`cup-player`), one active at a time; "Weiter" cues, the operator starts. | done |
| – | How players log in | An admin login for every player, also `cup-player` (email and password); no separate player tokens (default). | done |
| – | Where `cup-player` keeps the songs | In memory: the cued song and the ones coming up, downloaded from `/songs` (default). | done |
| – | Disconnected players | Removed automatically after 20 s, so a short drop keeps the active role; the music control can remove one at once (your call, 2026-10-13). | done |
| – | Music for testing and the demo | Songs by TheFatRat, downloaded by `seed_demo` into `demo-songs/` and never committed, since their terms don't allow passing them on (your call, 2026-10-13). | done |

## Moderation screen (MOD), built 2026-10-13

Not answered; built with the plan's defaults, which can still be changed.

| ID | Question | Decision | Status |
|---|---|---|---|
| MOD-1 | Public or login? | Public, like the info board; the playback query has no file names. | done |
| MOD-2 | What "progress of the current act" means | Both: the act time from the music start, and the song's waveform while it's loaded. | done |
| MOD-3 | The delay timer | The music control's words and colours, plus "Ende ca." in small. | done |

## Award ceremony music queue (AQ), built 2026-10-13

Not answered; built with the plan's defaults, which can still be changed.

| ID | Question | Decision | Status |
|---|---|---|---|
| AQ-1 | Only act songs? | Only act songs; other media later. | done |
| AQ-2 | Where | A view of its own, `/admin-awards`, linked from the music control during custom timeplan entries (your call, 2026-10-13; first built as a panel in the music control). | done |
| AQ-3 | Playing on demand | Click or "Nächster", never automatically. | done |
| AQ-4 | Keeping the queue | In the server's memory, shared by all music controls. | done |
| AQ-5 | Where a song starts | From the beginning; the waveform's chip jumps to the loudest moment. | done |

## Photos on the info board (PHOTO), built 2026-10-13

Not answered; built with the plan's defaults, which can still be changed.

| ID | Question | Decision | Status |
|---|---|---|---|
| PHOTO-1 | Where the photos are | All three: a Nextcloud account folder (WebDAV with an app password), a share link, or a local folder. | done |
| PHOTO-2 | Subfolders | The folder and its direct subfolders. | done |
| PHOTO-3 | Hiding a photo | Yes, "Nicht zeigen" on the music control's preview, which sits below the act (your call); kept until the server restarts. | done |
| PHOTO-4 | Breaks | Random photos from the 30 newest, as before. | done |

## Re-planning single acts (ART), built 2026-10-15

From `notes/act-timeplan-plan.md`; the two decisions of the plan plus the implementation calls (my calls).

| ID | Question | Decision | Status |
|---|---|---|---|
| ART-1 | Breaks in the live flow | Every break has a planned duration, shown as a countdown to the next item; started and ended by hand (Weiter / Zurück), never automatically. Ending a break starts the next slot in the same step. | done |
| ART-2 | Reset | "Reihenfolge zurücksetzen" only for blocks that haven't started. | done |
| ART-3 | Schema change | A new migration (`20261015120000_timeplan_slots`), not the baseline: the baseline is installed already (local, demo). | done |
| ART-4 | Commands | One `set_block_order` for moving acts (also between blocks), adding, editing and removing breaks; `reset_block_order`; `add_timeplan_entry` with `kind`, `einfahrzeit_seconds`, `after_id`. No separate `move_act` / break commands. | done |
| ART-5 | Acts left out of `set_block_order` | They go back to their home block (the first started block of their category, otherwise its first block), to its end. Refused if that block has ended; the same for reset, delete and turning a block into a custom entry. | done |
| ART-6 | Order of started slots | Started slots always come first, in their planned order (not by start time, which would trust the clock and imported data). | done |
| ART-7 | Acts overview | Shuffling and moving there still set the category order; blocks re-planned by hand ignore it and are marked "im Zeitplan umgeplant". | done |

## Group type (GT), answered 2026-10-15

| ID | Question | Decision | Status |
|---|---|---|---|
| GT-1 | A group outside its type's size range (Kleingruppe over 8, Großgruppe under 9 regular riders) | It keeps the category of the type the club chose, so it's planned in the timeplan from the start; clubs and admins see a warning until it's fixed. Groups under 3 regular riders still get no category, whatever their type (my call). | done (the warning comes from the frontend) |
| GT-2 | Who may change the type later | The owner club while it may still edit its groups, admins always (default). | done (`set_club_group_type`) |

## Finals (FN), built 2026-10-16

Implementation calls (mine) for the item "Finals planning" (RE-2).

| ID | Question | Decision | Status |
|---|---|---|---|
| FN-1 | Where finals live | Tables `finals` and `final_slots` (one row per reserved slot, holding its live state like an act); a final's block is a `kind = 'acts'` timeplan entry with `final_name`, so the `kind` CHECK stays and no table rebuild is needed. Each final runs in at most one block. | done |
| FN-2 | Slot names | "3. Finalist (U17–U21)": numbered within each group, groups in the order given ("5 × U11–U15, 4 × U23+"). | done |
| FN-3 | Running a final | A finalist slot runs and is judged like an act, with the final's durations; no song is cued. The music control links to the music search for a finalist's song. | done |
| FN-4 | Changing a final | Its slots can't change once one has started, and a started final can't be deleted; deleting a final deletes its timeplan block. Final blocks aren't re-planned by hand (no breaks, no moving). | done |
| FN-5 | Startlist | The CSV lists a final's slots as `#` comment lines; the public startlist leaves finals out until finalists are assigned. | done |
| FN-6 | Planning days | Day headings in the admin timeplan from the planned start; a block moves to another day by moving it and giving the day's first entry an earliest start. | done |

## Result import (RES), answered 2026-10-15

| ID | Question | Decision | Status |
|---|---|---|---|
| RES-1 | The jury system's export | It can't export CSV yet, so we define the format; the proposal is in the plan's section "Result import". | applies |
| RES-2 | Matching rows to acts | The jury system reuses our startlist. Acts get fixed start numbers and rows are matched by them. The numbers that exist can be frozen, and late registrations still get new ones. | applies |
| RES-3 | What the results are for | Filling the finals' slots, a public results page, the info board and certificates. Not the medals: they're prepared before the event. | applies |
| RES-4 | Publishing | Private until an admin publishes them (per category or final, default). | applies |

## Küren and Starter views (VIEW), answered 2026-10-15

| ID | Question | Decision | Status |
|---|---|---|---|
| VIEW-1 | Combine the Küren and Starter views into one? | No: both views stay as they are; the plan is dropped. | done (no change) |

## TypeScript 7 and API tooling (TOOL), answered 2026-10-21

| ID | Question | Decision | Status |
|---|---|---|---|
| TOOL-1 | When to replace openapi-typescript / openapi-fetch? | Now, with Kubb (`@kubb/plugin-ts`, `@kubb/plugin-fetch`) instead of an own generator. | done |

## FinTS (FIN), for when Part 2 is picked up again

| ID | Question | Decision | Status |
|---|---|---|---|
| FIN-3 | Who registers the FinTS product? | The registration is pending (by you). | applies |
| FIN-4 | Where the PIN lives | Encrypted in the DB, with a key derived with argon2id. | applies |
| FIN-5 | Poll interval | About every 6 hours for now. | applies |
| FIN-6 | Book automatically? | Book automatically only on perfect (100 %) matches, and keep a log of all incoming transfers and how they were applied. | applies |
| FIN-8 | How long bank data is kept | Keep everything for now, with only amount, reference, date and source. The DB is deleted after the event anyway. | applies |

## Review follow-ups (BL)

| ID | Question | Decision | Status |
|---|---|---|---|
| BL-1 | Owner club can't remove other clubs' riders or hand a group over; the same person registered by two clubs isn't detected | Both are fine as they are; admins handle such cases. | done (no change) |

## Jury registration (JR), answered 2026-10-23

The design decisions of the jury registration are in #225; these came up afterwards.

| ID | Question | Decision | Status |
|---|---|---|---|
| JR-1 | Do incomplete groups (no category yet) count as Küren for the jury obligation? | Yes, for the owner club, like they pay start fees (from one regular rider on). | done |
| JR-2 | Publish the Juryplan only after the judge deadline? | Yes; before it, clubs can still change their judges. | done |
