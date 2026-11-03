# PR previews built in the cloud and deployed through a jumphost

Every non-draft PR into `2027` from a branch of this repo (dependabot and forks are skipped; the `preview` label forces one) gets its own preview instance, and every push to `2027` updates the central 2027 instance; both run on the `einrad-server` VM in the home network behind the `caddy` VM. That VM can't be reached from GitHub's runners, has 2 cores and ~35 GB of disk, and used to build the binary itself. We decided that GitHub-hosted runners build the release binary and run Ansible themselves, reaching `einrad-server` and `caddy` with `ProxyJump` through a jumphost in the home network, as a dedicated `deploy` user. The jumphost won because it needs nothing new hosted at home, GitHub carries the concurrency and the compute of parallel PR builds, and it is how production is already deployed (a GitHub-hosted runner runs Ansible over SSH with a cloud-built binary).

## Considered options

- **Jumphost** (chosen): nothing new to run or patch at home; the deploy is the same Ansible a person runs by hand.
- **Self-hosted runner** on the VM: it would reach the VM directly, but builds would compete with the running instances for 2 cores and the disk, run one at a time, and a public repo's workflows would execute on a home machine.
- **Webhook receiver** at home that pulls and deploys: a new service to write, expose, authenticate and keep running, and the deploy logic would live outside the workflows.

The price of the jumphost is that a private key with passwordless sudo on `einrad-server` and `caddy` lives in GitHub secrets (environment `home`). It is limited by the `deploy` user on the jumphost: no shell, and its keys may only forward to port 22 of `einrad-server` and `caddy`. The jumphost's host name and port live only in those secrets and are never written to the repo, workflows or logs.

## Decisions made along the way

- **Release builds everywhere.** Previews were first planned as debug builds for speed; they switched to the same static musl release build as production, because the frontend is only embedded in release builds and a preview should run what production runs. `[profile.release]` keeps `debug = "line-tables-only"` so backtraces show file and line.
- **Cloud-only builds.** The build host (building on the VM) is removed; a reusable workflow builds `einrad-server` and `seed_demo` as a workflow artifact for the previews and the central 2027 instance. Production's `build-binary.yml` stays as it is until production switches.
- **`deploy` owns the apps on `einrad-server`.** The services run as `deploy` from its home directory, so the playbooks need no `become_user`.
- **One Caddy file per site.** The Caddyfile imports `/etc/caddy/sites/*.caddy`; a preview adds and removes only its own file, and existing sites stay untouched.
- **Previews are reseeded on every push** with `seed_demo`; they hold no data worth keeping, and a fresh database means a migration change never meets stale data.
- **Mailpit for previews only.** Previews send their mail to a Mailpit on `einrad-server` (UI behind Authelia), so demo data never mails anyone. The central 2027 instance sends real mail.
- **The central 2027 instance keeps its data**: a backup, then the migrations; a manual run with `reseed` recreates the demo data.

## Consequences

- The line tables make the binary larger than a plain release build; if that ever hurts (e.g. on the Uberspace production host), they're the first thing to drop.
- Production still deploys `latest-build` from `main`; switching it to `latest-2027` is a later, explicit step.
- In the Ansible inventory the group `preview` and `deploy/preview.yml` are the central 2027 instance, not a PR preview.
- A PR's preview deploys with that PR's own `deploy/`, so a PR can change how previews are deployed, but also break its own preview.
