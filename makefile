
all: frontend backend
	
migrate-db:
	sqlx migrate run

install-frontend:
	cd frontend && npm install
	mkdir -p static

openapi: migrate-db install-frontend
	cargo run --bin gen_openapi

# The builds generate the API client from openapi.json (see frontend/kubb.config.js).
frontend: openapi
	cd frontend && npm run build

frontend-release: openapi
	cd frontend && npm run build:release

run: all
	cargo run

backend: migrate-db
	cargo build

release: clean-frontend openapi frontend-release migrate-db
	cargo build --release

centos-release: clean-frontend openapi frontend-release migrate-db
	cargo build --release --target x86_64-unknown-linux-musl

# Event server in the venue: release build with the server player (needs the
# ALSA headers, libasound2-dev on Linux).
event-release: clean-frontend openapi frontend-release migrate-db
	cargo build --release --features local-audio

# The command-line player for the sound desk, target/release/cup-player
# (needs the ALSA headers, libasound2-dev on Linux).
cup-player: migrate-db
	cargo build --release --features cli-player --bin cup-player

demo:
	cargo run --bin seed_demo

run-release: release
	cargo run --release

# Integration tests start Mailpit via testcontainers. Fall back to the rootless
# Podman socket when Docker isn't available.
PODMAN_SOCKET := /run/user/$(shell id -u)/podman/podman.sock
ifeq ($(DOCKER_HOST)$(wildcard /var/run/docker.sock),)
ifneq ($(wildcard $(PODMAN_SOCKET)),)
export DOCKER_HOST := unix://$(PODMAN_SOCKET)
endif
endif

test: migrate-db
	cargo test --features player-client
	cd frontend && npm test

# After the frontend build: the type check needs the generated API client.
lint: frontend
	cargo clippy
	cd frontend && npm run check

clean-frontend:
	rm -rf static

clean: clean-frontend
	cargo clean

reset: clean-frontend
	rm -rf data
	rm -rf db.sqlite3

download:
	rsync -v uberspace:einrad-server/db.sqlite db.sqlite
	rsync -v -r uberspace:einrad-server/data/ data/

sync-up:
	rsync -v db.sqlite uberspace:einrad-server/db.sqlite

# Deploy with Ansible (see deploy/) a GitHub release or a binary you have, e.g.
# `make deploy-preview ANSIBLE_ARGS="-e cup_github_tag=<tag>"` or
# `ANSIBLE_ARGS="-e cup_binary=<file>"`; `--check --diff` shows what would change.
deploy-preview:
	ansible-playbook -i deploy/inventory.yml deploy/preview.yml $(ANSIBLE_ARGS)

deploy-production:
	ansible-playbook -i deploy/inventory.yml deploy/production.yml $(ANSIBLE_ARGS)

# Mailpit for the PR previews on einrad-server, its UI behind Authelia on caddy.
deploy-mailpit:
	ansible-playbook -i deploy/inventory.yml deploy/mailpit.yml $(ANSIBLE_ARGS)

# The preview of one PR, e.g.
# `make deploy-preview-pr PR=12 ANSIBLE_ARGS="-e cup_binary=<dir>/einrad-server"`.
deploy-preview-pr:
	ansible-playbook -i deploy/inventory.yml deploy/preview-pr.yml -e pr=$(PR) $(ANSIBLE_ARGS)

# Remove the preview of one PR, `make prune-previews PR=12`, or every preview
# but some, `make prune-previews KEEP=[12,14]` (`KEEP=[]` removes all).
prune-previews:
	ansible-playbook -i deploy/inventory.yml deploy/preview-prune.yml $(if $(PR),-e pr=$(PR)) $(if $(KEEP),-e 'keep=$(KEEP)') $(ANSIBLE_ARGS)
