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

System settings (server, database, SMTP, auth, admin, photos, feature
defaults) come from CLI flags, environment variables (including `.env`) and an
optional TOML config file, in that order of precedence. The config file is read
from `--config` / `CONFIG`, or from `./config.toml` if it exists. See
[`config.example.toml`](config.example.toml) for all options.

The competition comes from a separate competition file, read from
`--competition` / `COMPETITION`, or from `./competition.toml` if it exists. See
[`competition.example.toml`](competition.example.toml). A config file that
still contains competition sections is rejected.

- `[competition]`: name, location, dates, the determining day for ages,
  registration and music upload deadlines, fees and payment details.
- `[[categories]]`, `[[finals]]` and `[[timeplan]]`. A final reserves slots for
  the best riders of the heats ("1. Finalist (U11–U15)") before the results are
  known; a `[[timeplan]]` entry with `final = "…"` plans it. Custom entries
  can leave out the photos on the info board (`show_photos = false`) or be
  internal (`internal = true`, see [Info board](#info-board-moderation-screen-and-photos)).
- `[competition.jury]`: the clubs' jury obligation (minimum age of judges,
  Einsätze per two Küren, fee per missing Einsatz, optional judge deadline).
  The defaults are the Jury-Konzept Einrad Freestyle's rules.
- `[[jury_categories]]`: what judges register for and get assigned to (e.g.
  one heat block, 2 Einsätze, or a final, 1 Einsatz).

The database is the source of the competition ([ADR 0005](docs/adr/0005-the-database-is-the-source-of-the-competition.md)),
the competition file only an import:

- It is applied only when the database has no competition. A file that differs
  from the database is ignored with a warning, so editing it and restarting
  changes nothing.
- The competition's id never changes once created; a file with another id is
  ignored with a warning.
- `--replace-competition` / `REPLACE_COMPETITION` replaces the competition
  (except its id), categories, finals, timeplan and jury categories in one
  transaction; an invalid file changes nothing. Club data is kept: clubs,
  riders, acts with music, pair requests, judges, bookings, fee waivers, users
  and roles. Timeplan slots and progress (also the acts' started and ended
  times), finals placements and the jury plan with its publication are dropped,
  with a warning listing them. Judge registrations are matched to the new jury
  categories by name; the others are written to
  `data_path/remap/jury-registrations-<timestamp>.toml`, with an empty
  `jury_category` to fill in. A file that doesn't change anything replaces
  nothing, so leaving the flag set doesn't drop the planning on every start.
- "Wettkampf bearbeiten" in the admin UI ("Wettkampf", Admin only) changes the
  competition while the server runs, also while the registration is open:
  dates and deadlines, fees, payment details, jury rules, photos and
  features. It validates like the competition file and applies right away;
  the id never changes, the determining day not once categories exist.
- The admin UI imports a competition file too ("Wettkampf", Admin only):
  without a competition directly, over an existing one like
  `--replace-competition`, after a preview of what it replaces and drops and a
  confirmation. Dropped judge registrations are offered as the remap file to
  download. The new settings apply without a restart.
- The export ("Wettkampf", Admin only) downloads the competition as a
  competition file that the import and `--competition` take unchanged, e.g. to
  save a test instance's settings: the settings with the id, the features it
  sets, the photos with their password (share the file only in trusted
  channels), the categories with explicit birthday bounds, the finals, the
  timeplan and the jury categories. Nothing clubs entered, no bookings, fee
  waivers, users, act order, progress, placements or jury plan.
- A filled-in remap file is applied on the admin judges screen ("Jury",
  "Remap-Datei anwenden"): entries with an existing `jury_category` register
  their judge for it and the task again; entries whose judge or jury category
  isn't found are listed.
- **Setup mode:** without a competition in the database and without a
  competition file, the server starts anyway if there is an admin (`[admin]`
  / `ADMIN`, or one in the database). Admins see the import, everyone else a
  "noch nicht eröffnet" page. Without an admin it refuses to start, as nobody
  could import.

Features that are still being refined are competition settings
(`[competition.features]`): on, off or unset. An unset feature follows
`[features]` in the system config, then the build default: on in debug builds,
off in release builds. A change in the database applies on the next request,
without a restart.

- `jury_registration` (`JURY_REGISTRATION` for the system default): clubs
  register judges for the jury categories, admins see the jury obligation and
  plan the jury. Off, the judge endpoints answer 404 and the UI shows nothing
  about judges.

The database migrations only create the schema and don't insert any data.

## Setting up a new season

1. Copy `competition.example.toml` to `competition.toml` and set the
   competition, the categories, the finals and the timeplan.
2. Start with a fresh database (archive the old `db.sqlite`).
3. Start the server. It migrates the database and creates the competition from
   the competition file. Or start it without one (setup mode, with an admin)
   and import the file under "Wettkampf" in the admin UI.

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
metrics start with `einrad_server_`.

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
- **Photos:** set `[competition.photos] source` (see
  `competition.example.toml`): a Nextcloud folder over WebDAV with a user and
  an app password, a Nextcloud share link, or a local folder. The server lists
  it every 5 s (the folder and its direct subfolders) and gets the images from
  Nextcloud scaled down to 1920 px where it can. It reads the competition's
  photo settings just as often, so a changed source is picked up without a
  restart. The music control shows the newest photo below the act ("Nach der
  Kür auf dem Info-Board") and can hide it from the info board until the
  server restarts or the source changes.
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


## Deployment

There are three setups:

- **Dev:** `make run` on your machine.
- **Preview:** `einrad-server`, an Ubuntu VM, behind
  <https://einrad-server.home.hoeser.dev/> on the `caddy` VM.
- **Production:** Uberspace (`uberspace`).

The Ansible playbooks in [`deploy/`](deploy) deploy preview and production:

```sh
make deploy-preview      # ansible-playbook -i deploy/inventory.yml deploy/preview.yml
make deploy-production   # ansible-playbook -i deploy/inventory.yml deploy/production.yml
```

The hosts in [`deploy/inventory.yml`](deploy/inventory.yml) are SSH host
aliases: the user, host name and key come from your `~/.ssh/config`. The
preview logs in to `einrad-server` and `caddy` as `deploy` (see
[The deploy user](#the-deploy-user)) and needs `sudo` there.
`ANSIBLE_ARGS="--check --diff"` shows what would change.

Both run the same static musl binary (it embeds the frontend) from
`~/<cup_dir>/bin`, with `config.toml`, `competition.toml`, `.env`, the
database and `data/` in `~/<cup_dir>`:

- **Binary:** built by GitHub Actions only, nothing builds on the servers.
  Both download a GitHub release, by default `latest-build`, built from `main`
  by [`build-binary.yml`](.github/workflows/build-binary.yml);
  `-e cup_github_tag=<tag>` picks another release, `-e cup_binary=<file>`
  deploys a binary you already have (e.g. `make centos-release` or an artifact
  of [`build.yml`](.github/workflows/build.yml)).
- **Config:** `config.<environment>.toml` and `competition.<environment>.toml`
  from the repository root (ignored by git, `config.preview.toml` …) are
  uploaded as `config.toml` and `competition.toml`. Without such a file, the
  one on the server stays as it is. The service sets `HTTP_ADDRESS` (and on
  the preview `BASE_URL`), which take precedence over both.
- **Update:** a new binary only starts after the service was stopped and the
  database was copied to `backups/` (the newest 10 are kept); the old binary
  stays as `bin/einrad-server.previous`. A new config only
  restarts the service. Then the playbook waits until the server answers.
- **Preview:** runs as the systemd service `einrad-server` in
  `~deploy/einrad-server`, as `deploy`, with ffmpeg from apt. The playbook then sets the
  `einrad-server.home.hoeser.dev` entry in the Caddyfile on `caddy` (a
  managed block; a hand-written entry for the domain is removed) and reloads
  Caddy. `-e preview_seed_demo=true` recreates the database with the demo data:
  `seed_demo` is the file next to `cup_binary`, or the release's
  `seed_demo-linux` asset (`latest-build` doesn't have one).
- **Production:** runs as the supervisord service `einrad-server` in
  `~/einrad-server`, and the web backend of
  `anmeldung.freestyle-cup.nrw` points to its port. The playbook stops if
  another service runs from that directory, and warns if ffmpeg is missing.

The settings and their defaults are in [`deploy/group_vars/`](deploy/group_vars).

To roll back, move `bin/einrad-server.previous` back, copy the
database from `backups/` and restart the service (`sudo systemctl restart
einrad-server` on the preview, `supervisorctl restart einrad-server`
on Uberspace).

### The deploy user

GitHub Actions deploys the preview as the user `deploy`, which
[`deploy/bootstrap.yml`](deploy/bootstrap.yml) creates. Run it once with your
own account, which needs `sudo` on every host, and again to change the keys:

```sh
ssh-keygen -t ed25519 -N '' -C einrad-deploy -f ~/.ssh/einrad-deploy
ansible-playbook -i deploy/inventory.yml deploy/bootstrap.yml -K \
  -e deploy_public_key=~/.ssh/einrad-deploy.pub
```

`deploy_public_key` is a public key or a file with one key per line (e.g. also
your own, to deploy the preview by hand); they replace `deploy`'s
`authorized_keys`. The playbook is idempotent and changes:

- **Jumphost** (inventory group `jumphost`): the host `jump` is an alias you
  define in your `~/.ssh/config`; its real name stays out of the repository.
  `deploy` gets no shell, and its keys may only forward to `einrad-server:22`
  and `caddy:22` (`restrict,port-forwarding,permitopen=…`), which is all
  `ProxyJump` needs. The jumphost resolves these names, so clients must jump
  to exactly them. If sshd doesn't allow `deploy` TCP forwarding, a
  `Match User deploy` block at the end of `/etc/ssh/sshd_config` does; if
  `AllowUsers`, `AllowGroups`, `DenyUsers`, `DenyGroups` or
  `DisableForwarding` lock it out, the playbook stops and you fix that by
  hand.
- **`einrad-server` and `caddy`:** `deploy` with the keys and passwordless
  `sudo` (`/etc/sudoers.d/deploy`, checked with `visudo`).

On `einrad-server`, the services run as `deploy` from its home directory:
the playbooks use the login user, and the preview playbook stops if that
isn't `deploy` (log in with `-u deploy` or a `User deploy` entry in your
`~/.ssh/config`). An instance deployed as your own user before moves over
once, with its database:

```sh
sudo systemctl stop einrad-server
sudo mv ~/einrad-server ~deploy/
sudo chown -R deploy:deploy ~deploy/einrad-server
make deploy-preview ANSIBLE_ARGS="-u deploy"   # rewrites the unit for deploy
```

### From GitHub Actions

[`deploy-production.yml`](.github/workflows/deploy-production.yml) runs the
production playbook on demand (Actions → Deploy production → Run workflow).
It needs these secrets in the `production` environment:

- `UBERSPACE_HOST`, `UBERSPACE_USER`: the SSH host name and user.
- `UBERSPACE_SSH_KEY`: a private key whose public key is in the Uberspace's
  `~/.ssh/authorized_keys`.
- `UBERSPACE_KNOWN_HOSTS`: the host's line from `ssh-keyscan <host>`.

The preview isn't reachable from GitHub's runners; deploying it from Actions
would need a self-hosted runner in the home network.
