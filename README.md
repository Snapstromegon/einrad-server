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

The SMTP settings and `jwt_secret` (`--jwt-secret`, `JWT_SECRET`, `[auth]
jwt_secret`) are required: the server refuses to start without them and names
what is missing. There are no built-in secrets. `reload_db_token`
(`RELOAD_DB_TOKEN`) and `metrics_token` (`METRICS_TOKEN`) are optional: without them the token
path of `reload_db` and `GET /metrics` are off (a logged-in admin can still
reload).

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
- Jury units: judges register for, and the jury plan assigns them to, the
  Vorlauf of every heat and every final (ADR 0016), so there is nothing extra
  to define. A heat counts 2 Einsätze, a final 1; `jury_deployments`,
  `jury_needed_technical`, `jury_needed_performance`, `jury_needed_dismounts`
  and `jury_registrable` on a `[[heats]]` or `[[finals]]` entry override that.
  `[[jury_categories]]` is gone: a file with it is rejected with an example.

The database is the source of the competition ([ADR 0005](docs/adr/0005-the-database-is-the-source-of-the-competition.md)),
the competition file only an import:

- It is applied only when the database has no competition. A file that differs
  from the database is ignored with a warning, so editing it and restarting
  changes nothing.
- The competition's id never changes once created; a file with another id is
  ignored with a warning.
- `--replace-competition` / `REPLACE_COMPETITION` replaces the competition
  (except its id), categories, heats, finals and timeplan in one
  transaction; an invalid file changes nothing. Club data is kept: clubs,
  riders, acts with music, pair requests, judges, bookings, fee waivers, users
  and roles. Timeplan slots and progress (also the acts' started and ended
  times), finals placements and the jury plan with its publication are dropped,
  with a warning listing them. Judge registrations are matched to the new heats
  and finals by name; the others are written to
  `data_path/remap/jury-registrations-<timestamp>.toml`, with an empty
  `jury_unit` (`heat: <name>` or `final: <name>`) to fill in. A file that doesn't change anything replaces
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
  channels), the categories with their ages (so the file carries over to the
  next season), the heats and finals (with their jury settings that differ
  from the defaults) and the timeplan. Nothing clubs entered, no bookings, fee
  waivers, users, act order, progress, placements or jury plan.
- A filled-in remap file is applied on the admin judges screen ("Jury",
  "Remap-Datei anwenden"): entries with an existing `jury_unit` (`heat: <name>`
  or `final: <name>`) register their judge for it and the task again; entries
  whose judge or jury unit isn't found are listed.
- **Setup mode:** without a competition in the database and without a
  competition file, the server starts anyway if there is an admin (`[admin]`
  / `ADMIN`, or one in the database). Admins see the import, everyone else a
  "noch nicht eröffnet" page. Without an admin it refuses to start, as nobody
  could import.

Features that are still being refined are competition settings
(`[competition.features]`, or "Funktionen" in "Wettkampf bearbeiten"): on, off
or unset. An unset feature follows the build default. The system config has
no features: a `config.toml` that still has `[features]` is rejected with
where they went. A change in the database applies on the next request, without
a restart.

- `jury_registration` (unset: on in debug builds, off in release builds): clubs
  register judges for the heats' Vorlaufs and the finals, admins see the jury obligation and
  plan the jury. Off, the judge endpoints answer 404 and the UI shows nothing
  about judges.
- `moderation_input` (unset: on): clubs can add an extra text for moderation
  when they register acts.

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
| Juryleitung | Jury units, judges, the jury plan, finals placements, the medals, the moderation screen |
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
   online server load it: log in as an admin and call `POST /api/command/reload_db`,
   or, if `RELOAD_DB_TOKEN` is set, as any logged-in user with
   `POST /api/command/reload_db?token=<RELOAD_DB_TOKEN>`.

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
- **Preview:** the central 2027 instance on `einrad-server`, an Ubuntu VM,
  behind <https://einrad-server.home.hoeser.dev/> on the `caddy` VM, deployed
  from every push to `main` (see [The central 2027 instance](#the-central-2027-instance)).
  The inventory group and the playbook are still called `preview`.
- **Production:** Uberspace (`uberspace`).

The Ansible playbooks in [`deploy/`](deploy) deploy preview and production,
the preview of a single PR and its removal (see
[PR previews](#pr-previews)) and Mailpit for the PR previews (see
[Mailpit](#mailpit)). They need the collections in
[`deploy/requirements.yml`](deploy/requirements.yml) beyond `ansible-core`:

```sh
ansible-galaxy collection install -r deploy/requirements.yml
make deploy-preview      # ansible-playbook -i deploy/inventory.yml deploy/preview.yml
make deploy-production   # ansible-playbook -i deploy/inventory.yml deploy/production.yml
make deploy-mailpit      # ansible-playbook -i deploy/inventory.yml deploy/mailpit.yml
make deploy-preview-pr PR=12 ANSIBLE_ARGS="-e cup_binary=<dir>/einrad-server"
make prune-previews PR=12   # or KEEP=[12,14]: removes PR previews
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
  of [`build.yml`](.github/workflows/build.yml)). The VMs may have CPUs
  without AVX, so [`.cargo/config.toml`](.cargo/config.toml) builds the C code
  in the binary (SQLite, ring) for baseline x86-64 like the Rust code;
  otherwise it gets the C compiler's default, which can crash with SIGILL
  there. `build.yml` checks this with
  [`check-cpu-baseline.sh`](.github/scripts/check-cpu-baseline.sh) on an
  emulated CPU without AVX.
- **Config:** by default nothing is uploaded. An instance without a
  `config.toml` goes through first-run setup (ADR 0011): the server logs a
  setup token and writes it to `einrad-server-setup-token.txt` next to the
  config. The deploy prints only that file's path (deploy logs may land in
  CI), never the token; open or `cat` it on the server. If the instance had a
  `config.toml` before the deploy and still ends up in first-run setup, the
  deploy shows a red, ignored `WARNING` task (it doesn't fail). A local file from the
  repository root (ignored by git) is uploaded only on request, as
  `-e cup_config_src=config.production.toml` and/or
  `-e cup_competition_src=competition.production.toml`, and then replaces the
  server's `config.toml` / `competition.toml` (0600). The service sets
  `HTTP_ADDRESS`, `DATABASE_URL` (`sqlite://./db.sqlite?mode=rwc`, so the
  server creates the database on the first start) and `BASE_URL` (preview and
  production, so the token file holds a clickable URL), which take precedence
  over both. The preview additionally loads `secrets.env` (`JWT_SECRET`,
  `RELOAD_DB_TOKEN`), generated once on the VM and never overwritten.
- **Update:** a new binary only starts after the service was stopped and the
  database was copied to `backups/` (the newest 10 are kept); the old binary
  stays as `bin/einrad-server.previous`. A new config only
  restarts the service. Then the playbook waits until the server answers.
- **Preview:** runs as the systemd service `einrad-server` in
  `~deploy/einrad-server`, as `deploy`, with ffmpeg from apt. The playbook then
  points `einrad-server.home.hoeser.dev` at it with the site file
  `/etc/caddy/sites/einrad-server.caddy` on `caddy` (see
  [Caddy site files](#caddy-site-files); an entry for the domain in the
  Caddyfile itself is removed). It keeps the database, `config.toml` and
  `secrets.env` on the VM (see
  [The central 2027 instance](#the-central-2027-instance)); without a
  database, the server creates an empty one.
  `-e preview_seed_demo=true` backs up the database and recreates it with the demo data:
  `seed_demo` is the file next to `cup_binary`, or the release's
  `seed_demo-linux` asset (`latest-build` doesn't have one).
- **Production:** runs as the supervisord service `einrad-server` in
  `~/einrad-server`, and the web backend of
  `anmeldung.lmfreestyle27.de` points to its port. The playbook stops before
  changing anything if another service runs from that directory, and warns if
  ffmpeg is missing. Without a `config.toml` the server starts in first-run
  setup, and without a database it creates an empty one.

The settings and their defaults are in [`deploy/group_vars/`](deploy/group_vars).

To roll back, move `bin/einrad-server.previous` back, copy the
database from `backups/` and restart the service (`sudo systemctl restart
einrad-server` on the preview, `supervisorctl restart einrad-server`
on Uberspace).

### The central 2027 instance

[`deploy-2027.yml`](.github/workflows/deploy-2027.yml) deploys every push to
`main` to <https://einrad-server.home.hoeser.dev/>:

1. **Build and test** in parallel: [`build.yml`](.github/workflows/build.yml)
   (the static musl release with `seed_demo`) and
   [`pr-test.yml`](.github/workflows/pr-test.yml) (tests and lint). If either
   fails, nothing is released or deployed.
2. **Release:** the prerelease `latest-2027` moves to the commit, with the
   assets `einrad-server-linux` and `seed_demo-linux`. It is never deleted:
   the first run creates it, later runs move its tag and then replace the
   assets in place, so they are missing only while they upload. A failed
   upload is tried again a few times; if all fail, the run fails before
   deploying, and the release stays at the commit with old or missing
   assets until the next push. Production can deploy it with
   `-e cup_github_tag=latest-2027` (or the `tag` input of Deploy
   production); its default stays `latest-build`.
3. **Deploy:** [`deploy/preview.yml`](deploy/preview.yml) with the build, as
   `deploy` through the jumphost (the `home` environment, see
   [From GitHub Actions](#from-github-actions)). The run shows up as a GitHub
   deployment in the environment `2027`, with the instance's URL; a failed
   deploy marks it failed.

Deploys run one after another (concurrency `deploy-2027`) and are never
cancelled halfway. GitHub keeps only one waiting deploy, so a newer run
replaces a waiting one, also a manual one with `reseed` (start it again). A
run whose commit is no longer the newest of `main` skips release and deploy,
as the newer commit's run does both.

The instance keeps its database, `config.toml` (with real SMTP: it sends
**real mail**, unlike the [PR previews](#pr-previews)) and `secrets.env` in
`~deploy/einrad-server` on the VM; nothing in the repository holds them. The
playbook uploads no config (a bare instance goes through first-run setup, and
the deploy prints the setup token file's path). A config that still has
competition sections fails the server's start and the health check. Without a
database, the server creates an empty one on start (setup mode, see above). A
new binary backs up the database and then migrates it on start.

**Reseed:** Actions → Deploy 2027 → Run workflow, branch `main`, with
`reseed` checked, backs up the database and recreates it with the demo data
(`-e preview_seed_demo=true`). Select `main` when starting the workflow;
started on any other branch, it fails before building.

**Moving the instance to `deploy`, once.** The instance used to run as your
own user from an old checkout (the unit `einrad-server` on port 3000, with
`.env`, `config.toml`, `db.sqlite` and maybe `data/` in the checkout). The first deploy as
`deploy` stops at the check above, with the old instance still running, but
it has published `latest-2027`. To move the instance, on `einrad-server`:

```sh
old=~/nrw-freestyle-cup-registration   # the old checkout
sudo install -d -o deploy -g deploy -m 0750 ~deploy/einrad-server
# Split the old config: the competition sections go into competition.toml,
# [photos] as [competition.photos].
cd "$(mktemp -d)"
awk '
  BEGIN { out = "config.toml" }
  /^[[:space:]]*\[/ {
    h = $0; sub(/^[[:space:]]*\[+[[:space:]]*/, "", h); sub(/[[:space:]]*[].].*/, "", h)
    out = h ~ /^(competition|categories|finals|timeplan|jury_categories|photos)$/ ? "competition.toml" : "config.toml"
    if (h == "photos") sub(/\[photos\]/, "[competition.photos]")
  }
  { print > out }' "$old/config.toml"
sudo install -o deploy -g deploy -m 0600 config.toml competition.toml "$old/.env" ~deploy/einrad-server/
# Try the new binary on a copy of the database, on another port and with
# an SMTP server that doesn't exist, so it can't send mail. Fix config.toml
# and competition.toml until it serves http://127.0.0.1:3999/, then stop it
# with Ctrl-C.
sudo install -o deploy -g deploy -m 0600 "$old/db.sqlite" ~deploy/einrad-server/try.sqlite
sudo -u deploy sh -c 'cd ~/einrad-server &&
  curl -fsSLo try-server https://github.com/Snapstromegon/einrad-server/releases/download/latest-2027/einrad-server-linux &&
  chmod +x try-server &&
  DATABASE_URL="sqlite://./try.sqlite?mode=rwc" HTTP_ADDRESS=127.0.0.1:3999 SMTP_SERVER=smtp.invalid ./try-server'
sudo sh -c 'rm -f ~deploy/einrad-server/try-server ~deploy/einrad-server/try.sqlite*'
# Move the database (and data/, if there is one) with the old instance
# stopped; a clean stop leaves no db.sqlite-wal behind. Skip this to start
# with an empty database instead.
sudo systemctl stop einrad-server
ls "$old"/db.sqlite*
sudo install -o deploy -g deploy -m 0600 "$old/db.sqlite" ~deploy/einrad-server/
[ ! -d "$old/data" ] || sudo cp -a "$old/data" ~deploy/einrad-server/
sudo chown -R deploy:deploy ~deploy/einrad-server
```

Then re-run the failed Deploy 2027 run (or push to `main`): it replaces the
old unit (same name and port) and starts the instance as `deploy`. The old
checkout with its database and binary stays as it is; once the new instance
runs, delete it.

### Caddy site files

Each site deployed to `caddy` has its own file in `/etc/caddy/sites/`, so
parallel deploys never edit the same file. The task files in
[`deploy/tasks/`](deploy/tasks) do the work:

- [`caddy_setup.yml`](deploy/tasks/caddy_setup.yml) creates `sites/` and
  appends a managed block to `/etc/caddy/Caddyfile`: the snippet
  `(authelia)` (Authelia's `forward_auth` on `127.0.0.1:9091`, as
  `monitoring.home.hoeser.dev` uses it) and then
  `import /etc/caddy/sites/*.caddy`. The hand-written site blocks stay as
  they are. It is idempotent; the preview playbook runs it every time.
- [`caddy_site.yml`](deploy/tasks/caddy_site.yml) writes
  `sites/<caddy_site>.caddy` with `caddy_site_content`, checks the whole
  config with `caddy validate` and reloads Caddy. If Caddy rejects it, the
  old file comes back and the task fails. The file is written to a hidden
  temp file and renamed, and the import skips hidden files, so a parallel
  run never sees half a file. As another run's broken file only stays until
  that run puts its old one back, the check tries 3 more times, 5 seconds
  apart, unless Caddy names this site file in the error.
- [`caddy_site_absent.yml`](deploy/tasks/caddy_site_absent.yml) removes
  `sites/<caddy_site>.caddy` and reloads Caddy.

```yaml
- name: Add the site
  ansible.builtin.include_tasks: tasks/caddy_site.yml
  vars:
    caddy_site: mail
    caddy_site_content: |
      mail.einrad-server.home.hoeser.dev {
      	import authelia
      	reverse_proxy "http://einrad-server:8025"
      }
```

`caddy_dir` and `caddy_reload_command` in
[`group_vars/proxy.yml`](deploy/group_vars/proxy.yml) say where the Caddyfile
is and how Caddy reloads.

### Mailpit

The PR previews send their mails to one shared
[Mailpit](https://mailpit.axllent.org/) on `einrad-server`; nobody receives
them, and you read them at <https://mail.einrad-server.home.hoeser.dev/>
behind Authelia. The instance of the preview playbook
(`einrad-server.home.hoeser.dev`) sends real mail and doesn't use it.
[`deploy/mailpit.yml`](deploy/mailpit.yml) (`make deploy-mailpit`) is
idempotent and

- installs `docker.io` and `python3-docker` from apt and runs the container
  `mailpit` with `restart: unless-stopped`: SMTP on `127.0.0.1:1025` only
  (no TLS, no login), the UI and API on port `8025` of the VM, for `caddy`.
  The mails are in the Docker volume `mailpit`; beyond 5000 the oldest go.
- writes the site file `/etc/caddy/sites/mail.caddy` on `caddy` (see
  [Caddy site files](#caddy-site-files)), which puts `import authelia` in
  front of `einrad-server:8025`.

The image is the `FROM` line of
[`deploy/mailpit/Dockerfile`](deploy/mailpit/Dockerfile), which only exists
so Dependabot keeps the tag current; a new tag replaces the container and
keeps the mails. Port, domain and limit are in
[`group_vars/preview.yml`](deploy/group_vars/preview.yml).

### PR previews

Every non-draft PR into `main` from a branch of this repository gets its own
preview at
`https://pr-<n>.einrad-server.home.hoeser.dev/`, with fresh demo data on
every push. [`preview.yml`](.github/workflows/preview.yml) builds the PR with
[`build.yml`](.github/workflows/build.yml) and runs
[`deploy/preview-pr.yml`](deploy/preview-pr.yml) from the PR's own checkout,
through the jumphost (see [From GitHub Actions](#from-github-actions)),
without waiting for the tests:

- **When:** on opening, reopening, a push, and when a draft becomes ready.
  Drafts get no preview unless they carry the label `preview`; adding that
  label also redeploys any PR. Other labels change nothing, and a PR turned
  back into a draft keeps the preview it has until the next nightly
  reconcile (see [Removing previews](#removing-previews)).
- **Skipped:** PRs from forks and events dependabot triggers, as neither gets
  the secrets. Labelling a dependabot PR `preview` deploys it.
- **Updates:** a new push cancels the running build or deploy of the PR and
  starts over.
- **In the PR:** the GitHub deployment `preview-pr-<n>` shows the preview as
  in progress, then links to it ("View deployment"), or shows it failed (also
  when the build failed or a newer push cancelled it), with the run as its
  log. There are no comments. Closing the PR removes the preview and marks
  the deployment inactive.

[`deploy/preview-pr.yml`](deploy/preview-pr.yml) installs or updates the
preview of one PR on `einrad-server` from a build you have, with `seed_demo`
next to the server binary; to deploy one by hand:

```sh
ansible-playbook -i deploy/inventory.yml deploy/preview-pr.yml \
  -e pr=12 -e cup_binary=<dir>/einrad-server
```

- **Instance:** `einrad-server@pr-12` of the systemd template unit
  `/etc/systemd/system/einrad-server@.service`, as `deploy`, from
  `~/previews/pr-12` (in `deploy`'s home), on port `20000 + 12`, with
  `MemoryMax=512M`, `CPUWeight=50` and `RUST_LOG=debug` (which the server
  doesn't read yet). The binary goes in like on the preview, with the shared
  tasks of [`tasks/release.yml`](deploy/tasks/release.yml), but without
  database backups.
- **Config:** `~/previews/pr-12/env`, rendered from
  [`templates/preview-pr.env.j2`](deploy/templates/preview-pr.env.j2)
  without secrets: `BASE_URL`, `HTTP_ADDRESS` and SMTP to Mailpit
  (`smtp://127.0.0.1:1025`). `secrets.env` next to it holds the required `JWT_SECRET` and
  the optional `RELOAD_DB_TOKEN`, generated with `openssl rand` on the first deploy and
  kept, so logins survive a deploy. No `config.toml` or `competition.toml`.
- **Demo data:** every run stops the instance and recreates its database
  with `seed_demo` and `demo-data.toml`, which bring the competition. The
  songs are downloaded once into `/var/cache/einrad-server/demo-songs`
  (`DEMO_SONGS`), which all previews share. If `seed_demo` fails, the
  instance starts again and the play fails.
- **Caddy:** the site file `/etc/caddy/sites/pr-12.caddy` on `caddy` (see
  [Caddy site files](#caddy-site-files)) serves
  `https://pr-12.einrad-server.home.hoeser.dev/` (own certificate, HTTP-01)
  with `X-Robots-Tag: noindex, nofollow`. It is written before the instance
  starts.
- **Failure:** if the server doesn't answer, the play fails and the instance
  keeps running, for `journalctl -u einrad-server@pr-12` and a look at the
  site.

It imports [`mailpit.yml`](deploy/mailpit.yml) first, so a fresh VM gets
Mailpit. Previews of different PRs can deploy at the same time: each has its
own directory, port and site file. They share the unit template (every run
writes it, the same unless a PR changes it), the Mailpit container (the
same unless a PR changes its image) and the song cache; the seeds of all
previews run one after another, so no song is downloaded twice at once.
Port base, directory and cache are in
[`group_vars/preview.yml`](deploy/group_vars/preview.yml).

[`deploy/preview-prune.yml`](deploy/preview-prune.yml) removes previews,
either the one of a PR or every preview but those of a list (e.g. the open
PRs):

```sh
make prune-previews PR=12           # -e pr=12: removes pr-12
make prune-previews KEEP=[12,14]    # -e 'keep=[12,14]': removes all others
make prune-previews KEEP=[]         # removes all previews
```

Set exactly one of `pr` (a positive integer) and `keep` (a list of them,
also as JSON: `-e '{"keep": [12, 14]}'`); anything else stops the playbook
before it connects. Removing a preview stops and disables
`einrad-server@pr-12`, deletes `~/previews/pr-12` on `einrad-server` and
the site file `/etc/caddy/sites/pr-12.caddy` on `caddy` (with
[`caddy_site_absent.yml`](deploy/tasks/caddy_site_absent.yml), which reloads
Caddy). The previews are found by their directories, their units (loaded or
enabled) and their site files, so whatever a half-failed deploy left behind
goes too. The central instance (`einrad-server.service`, `~/einrad-server`,
`sites/einrad-server.caddy`), Mailpit and `sites/mail.caddy`, the unit
template `einrad-server@.service` and the song cache stay. A preview that
doesn't exist is no error, so a second run changes nothing.

#### Removing previews

[`preview-teardown.yml`](.github/workflows/preview-teardown.yml) runs
[`deploy/preview-prune.yml`](deploy/preview-prune.yml) through the jumphost,
always from a checkout of `main` (a closed PR's branch may be gone), and
marks the deployments `preview-pr-<n>` of the removed previews inactive with
[`.github/scripts/preview-deployments.sh`](.github/scripts/preview-deployments.sh):

- **Teardown:** closing a PR into `main` (merged or not, draft or not)
  removes its preview (`-e '{"pr": 12}'`). It shares the concurrency group
  `preview-pr-<n>` with [`preview.yml`](.github/workflows/preview.yml), so it
  cancels a running deploy of the PR; reopening the PR cancels the teardown
  and deploys again. Forks and closes by dependabot are skipped (no
  secrets); the reconcile removes what they leave.
- **Reconcile:** every night at 03:17 UTC and by hand (Actions → PR preview
  teardown → Run workflow), it removes every preview but those of the open
  PRs into `main` that should have one: from a branch of this repository,
  not a draft or labelled `preview` (dependabot's included, as the list only
  keeps previews). Leftovers of a skipped or failed teardown and previews of
  PRs turned back into drafts go. It also marks every deployment of a
  removed preview inactive that isn't yet, e.g. when a cancelled deploy
  marked it failed after the teardown, but not those of PRs that got a
  preview while the playbook ran (it lists the open PRs again). A running
  reconcile is never cancelled; GitHub keeps one waiting run, so a newer one
  replaces it.
- **Race:** the open PRs are listed right before the playbook, after the
  SSH and Ansible setup. A PR that only gets its preview after the listing
  would lose it if its deploy finished in the seconds until the playbook has
  looked for the previews; a deploy builds for minutes first, and the next
  push or `preview` label deploys it again. A teardown and a reconcile can
  remove the same preview at once; if one of them fails over it, the next
  run cleans up.
- **Environments:** the workflow token can't delete environments (that
  needs the administration permission), so `preview-pr-<n>` stays listed
  under Settings → Environments with an inactive deployment. Delete old ones
  by hand if they get in the way:
  `gh api -X DELETE repos/Snapstromegon/einrad-server/environments/preview-pr-12`.
- **Default branch:** GitHub runs scheduled workflows and offers Run
  workflow only for workflow files on the default branch (`main`), so the
  reconcile needs `preview-teardown.yml` on `main`. It checks out `main`
  itself, so it prunes with `main`'s `deploy/` from either branch.

### The deploy user

GitHub Actions deploys the preview as the user `deploy`, which
[`deploy/bootstrap.yml`](deploy/bootstrap.yml) creates. The
[setup wizard](#the-setup-wizard) does this with a new key; by hand, run it
once with your own account, which needs `sudo` on every host, and again to
change the keys:

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

### The setup wizard

[`deploy/setup-wizard.sh`](deploy/setup-wizard.sh) walks you through the
manual setup for deploying the preview from GitHub Actions, step by step;
run it from the home network with `gh` logged in:

```sh
deploy/setup-wizard.sh
```

1. **Deploy key:** a new ed25519 key in a temp directory outside the
   repository.
2. **Bootstrap:** runs `bootstrap.yml -K` with the key, optionally with your
   own public key too.
3. **Jumphost and host keys:** asks for the jumphost's host name and port and
   scans the host keys of the jumphost (named `jump`, the `HostKeyAlias`) and
   of `einrad-server` and `caddy` (at the `HostName` your `~/.ssh/config`
   gives them), for you to compare.
4. **GitHub:** creates the environment `home` (no reviewers) and sets its
   secrets, asking before it replaces one.
5. **Check:** logs in like CI, as `deploy` through `jump` to `einrad-server`
   and `caddy`, without your `~/.ssh/config`.
6. **Authelia:** how to check that `access_control` covers
   `mail.einrad-server.home.hoeser.dev`, and the rule to add if not.
7. **Cleanup:** deletes the new private key; CI has it in `HOME_SSH_KEY`.

Each step can be skipped when it's done already; nothing is remembered
between runs. The jumphost's name only goes from your input into the secret
`HOME_JUMPHOST`, never into a file in the repository.

### From GitHub Actions

[`deploy-production.yml`](.github/workflows/deploy-production.yml) runs the
production playbook on demand (Actions → Deploy production → Run workflow).
It needs these secrets in the `production` environment:

- `UBERSPACE_HOST`, `UBERSPACE_USER`: the SSH host name and user.
- `UBERSPACE_SSH_KEY`: a private key whose public key is in the Uberspace's
  `~/.ssh/authorized_keys`.
- `UBERSPACE_KNOWN_HOSTS`: the host's line from `ssh-keyscan <host>`.

GitHub's runners reach the preview's hosts through the jumphost
(`ProxyJump jump`, to exactly `einrad-server` and `caddy`), with these
secrets in the `home` environment, which the
[setup wizard](#the-setup-wizard) sets:

- `HOME_SSH_KEY`: the private key of `deploy`.
- `HOME_KNOWN_HOSTS`: host keys of `jump` (the jumphost, looked up with
  `HostKeyAlias jump`), `einrad-server` and `caddy`.
- `HOME_JUMPHOST`, `HOME_JUMPHOST_PORT`: the jumphost's host name and SSH
  port, as reachable from the internet.

[`deploy-2027.yml`](.github/workflows/deploy-2027.yml) deploys the central
2027 instance with them (see
[The central 2027 instance](#the-central-2027-instance)).

Workflows set up SSH with the
[`home-ssh`](.github/actions/home-ssh/action.yml) action, after which `ssh
einrad-server`, `ssh caddy` and Ansible with `deploy/inventory.yml` work.
[`check-home-ssh.yml`](.github/workflows/check-home-ssh.yml) (Actions → Check
home SSH → Run workflow) checks the secrets: it connects to both hosts as
`deploy` and runs `sudo`. [`preview.yml`](.github/workflows/preview.yml)
deploys the [PR previews](#pr-previews) this way and
[`preview-teardown.yml`](.github/workflows/preview-teardown.yml) removes them
(see [Removing previews](#removing-previews)); both only use the environment
`home` for its secrets (`deployment: false`), so the PR shows only the
deployment `preview-pr-<n>`.
