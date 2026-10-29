# Freestyle Cup Registration System

This project contains the registration system for the freestyle unicycling cup NRW.

## Requirements

- **ffmpeg** must be installed on the server (`ffmpeg` on the `PATH`, or the
  binary in the `FFMPEG` environment variable). Every uploaded song gets a
  loudness-normalized copy (−14 LUFS, EBU R128) made with ffmpeg in the
  background; the music players use it and can fall back to the original.
  Without ffmpeg the copies fail and the originals are played. Songs that have
  no copy yet are normalized when the server starts.

## Configuration

Settings come from CLI flags, environment variables (including `.env`) and an
optional TOML config file, in that order of precedence. The config file is read
from `--config` / `CONFIG`, or from `./config.toml` if it exists. See
[`config.example.toml`](config.example.toml) for all options.

Event settings only come from the config file:

- `[competition]`: name, location, dates, the determining day for ages,
  registration and music upload deadlines, fees and payment details. It is
  written to the `competition` table on every start.
- `[[categories]]`, `[[finals]]` and `[[timeplan]]`: only written into empty
  tables, so changes made in the admin UI are kept. A final reserves slots for
  the best riders of the heats ("1. Finalist (U11–U15)") before the results are
  known; a `[[timeplan]]` entry with `final = "…"` plans it. Custom entries
  can leave out the photos on the info board (`show_photos = false`) or be
  internal (`internal = true`, see [Info board](#info-board-moderation-screen-and-photos)).
- `[competition.jury]`: the clubs' jury obligation (minimum age of judges,
  Einsätze per two Küren, fee per missing Einsatz, optional judge deadline).
  The defaults are the Jury-Konzept Einrad Freestyle's rules.
- `[[jury_categories]]`: what judges register for and get assigned to (e.g.
  one heat block, 2 Einsätze, or a final, 1 Einsatz). Only written into an
  empty table, like `[[categories]]`.

Features that are still being refined are behind flags in `[features]`. They
are on in debug builds and off in release builds unless set:

- `jury_registration` (`JURY_REGISTRATION`): clubs register judges for the jury
  categories, admins see the jury obligation and plan the jury. Off, the judge
  endpoints answer 404 and the UI shows nothing about judges.

The database migrations only create the schema and don't insert any data.

## Setting up a new season

1. Copy `config.example.toml` to `config.toml` and set the competition, the
   categories, the finals and the timeplan.
2. Start with a fresh database (archive the old `db.sqlite`).
3. Start the server. It migrates the database and seeds it from the config.

The migrations of the 2024–2026 seasons were squashed into one baseline
migration; the originals are kept in `migrations-archive/` for reference. A
database created by those migrations can't be migrated with the baseline.

## Roles

The organizer team works with roles ([ADR 0003](docs/adr/0003-fixed-roles-for-the-organizer-side.md)).
A user can hold several; Admins grant them in the users table of the admin
overview (`/admin`). The first Admin comes from the `ADMIN` setting. Roles
never open another club's registration: that stays with the club contact.

| Role | Can |
|---|---|
| Admin | Everything, including granting roles |
| Juryleitung | Jury categories, judges, the jury plan, finals placements, the medals, the moderation screen |
| Kasse | Payments, with the clubs, their contacts and riders |
| Check-in | The check-in desk with birthdates and club balances, the medals |
| Musiksteuerung | The music control, players, award ceremonies, the moderation screen and checking riders in at the start |
| Moderation | The moderation screen |
| Musikprüfung | Marking songs as checked in the acts overview |

Give player logins (web player, `cup-player`) only the Musiksteuerung role.

## Metrics

With `METRICS_TOKEN` (or `[server] metrics_token`) set, `GET /metrics` serves
Prometheus metrics in the OpenMetrics format; without a token the endpoint
doesn't exist. The database counts (users, clubs, riders, acts, music,
payments, fees, the live timeplan and its delay) are read on every scrape;
HTTP requests, mails and normalization jobs are counted as they happen. All
metrics start with `cup_`.

```yaml
scrape_configs:
  - job_name: freestyle-cup
    scheme: https
    scrape_interval: 30s
    authorization:
      credentials: "<METRICS_TOKEN>"
    static_configs:
      - targets: ["anmeldung.example.org"]
```

## Remote music playback / event server

The music control (Musiksteuerung role) can play the current act's song itself, or drive a
player on another device: the web player at `/player` (any browser, e.g. a
laptop at the sound desk) or the server player, which plays on the server's own
sound card. Players and the music control talk over a websocket
(`/api/ws/player`); both need a login with the Musiksteuerung (or Admin) role.
"Weiter" in the timeplan cues the
next song on the active player but never starts it. The act timer starts with
the act's music, not with "Weiter" (acts without music: with "Weiter"), and the
waveform marks where the act's time runs out.

- **Web player:** open `/player` and click once to unlock audio (browsers
  block autoplay until then).
- **Server player:** build with `--features local-audio` (`make
  event-release`; needs the ALSA headers, `libasound2-dev` on Linux) and set
  `LOCAL_PLAYER=<name>` (`--local-player`, `[server] local_player`).
- **Command-line player:** `cup-player` plays on the sound card of the
  computer it runs on, without a browser: no audio to unlock, no tab to close
  by accident. Build it with `make cup-player` (needs the ALSA headers like
  the server player) and start it with a login with the Musiksteuerung role:

  ```sh
  CUP_SERVER=https://anmeldung.example.org CUP_EMAIL=musiksteuerung@example.org \
    CUP_PASSWORD=… target/release/cup-player
  ```

  The settings can also go into a `.env` file next to it. `--name` sets the
  name in the music control (default: the computer's name; a player with the
  same name takes over), `--device` the sound card (`--list-devices` lists
  them). It downloads the songs coming up in advance, and when the
  connection drops the music plays on while it connects again.

Event day setup:

1. Close registration and music changes before the event.
2. Copy the database and the songs with `make download`.
3. Run the server in the venue's local network. Over plain http set
   `INSECURE_COOKIES=true`, otherwise the login cookie is `Secure` and the
   browsers drop it.
4. After the event, upload the database with `make sync-up` and make the
   online server load it with `POST /api/command/reload_db?token=<RELOAD_DB_TOKEN>`.

## Info board, moderation screen and photos

- **Info board** (`/info-board`, public): the big screen in the hall. After an
  act it shows the newest photo, in breaks random recent ones, and a QR code
  to the photos if `[competition] photos_url` is set. During a custom
  timeplan entry it shows the entry's name and the clock, with photos unless
  the entry turns them off ("Fotos" in the admin timeplan).
- **Internal timeplan entries** ("intern" in the admin timeplan): custom
  entries only the organizer team sees, e.g. a jury meeting. Admin,
  Juryleitung, Moderation and Musiksteuerung see them, marked "intern";
  everyone else, and always the info board, the startlist and its CSV, get a
  „Pause“ of the same length instead.
- **Photos:** set `[photos] source` (see `config.example.toml`): a Nextcloud
  folder over WebDAV with a user and an app password, a Nextcloud share link,
  or a local folder. The server lists it every 15 s (the folder and its direct
  subfolders) and gets the images from Nextcloud scaled down to 1920 px where
  it can. The music control shows the newest photo below the act ("Nach der
  Kür auf dem Info-Board") and can hide it from the info board until the
  server restarts.
- **Moderation screen** (`/admin-moderation`, Moderation, Musiksteuerung,
  Juryleitung or Admin role): the clock, the delay
  as a big timer, the current act with its riders, description and progress
  (the act time from the music start, and the song's waveform while it plays),
  what comes next, and the timeplan. It has no controls.
- **Award ceremony** (`/admin-awards`, "Siegerehrung"): queues act songs
  found by name, rider, club, category or song, and plays them on demand on
  the player chosen in the music control, or else in its own browser. The
  queue is kept on the server until it restarts, so every open view sees the
  same.

## Demo data

`cargo run --bin seed_demo` (or `make demo`) **deletes the configured database**
and recreates it with the demo data from [`demo-data.toml`](demo-data.toml):
users in every state, clubs with full, partial, missing and excess payments,
riders in single and pair starts (incl. partners across clubs and
unregistered partners), groups (a small group across clubs with a replacement
rider, a large group and an incomplete group without category), acts with
names, songs and orders, a final, a timeplan that is partly run and a check-in in
progress (one club fully, one partly, one not checked in). The file lists the
logins at the top, including one account per role. The database and data path are resolved like for the
server; use `--demo` / `DEMO_DATA` for another file. Tests can load it with
`demo::DemoData`.

The demo songs are by [TheFatRat](https://www.thefatrat.com/downloads), used
under their [terms](https://www.thefatrat.com/terms-and-conditions). The terms
don't allow passing the files on, so they aren't in the repository: the first
run downloads them (about 80 MB, needs `curl`) into `demo-songs/` next to the
demo file (`--songs` / `DEMO_SONGS` for another directory) and later runs
reuse them. Songs that can't be downloaded, and all songs in the tests, are
short generated tones.
