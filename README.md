# TBK Progression Bridge

A small REST service that lets the **TBK - Saved Progression** Arma Reforger addon persist player stats to either **SQLite** or **PostgreSQL**, and exposes a public web dashboard with live stats and a searchable leaderboard.

The Reforger game itself cannot speak SQL directly from Enforce Script, so this service sits between the game server and your database. The addon uses Reforger's `RestApi` to POST stat deltas to this bridge over HTTP.

## Install

Two ways to run the bridge:

- **[Docker Compose](#option-a--docker-compose-recommended)** — recommended. One command brings up the bridge; Postgres and HTTPS are opt-in via Compose profiles and combine freely. No Rust toolchain on the host.
- **[Manual build](#option-b--manual-build)** — build from source with `cargo`. Useful for development, or when you don't want Docker on the host.

In both cases the bridge reads `config.toml` from its working directory (override with `--config <path>`). On first run it creates the SQLite file (if using SQLite) and applies the schema in either backend.

Once it's up, verify:

```bash
curl http://127.0.0.1:8787/health
# -> ok
```

Open `http://127.0.0.1:8787/` in a browser to see the dashboard.

### Option A — Docker Compose (recommended)

Requires Docker Engine 20.10+ with the Compose v2 plugin. The default stack is the bridge alone with SQLite on a named volume; Postgres and HTTPS are each opt-in via a Compose profile and combine freely.

**SQLite (default):**

```bash
git clone <repo-url>
cd TBKSavedProgressionBridge
cp config.docker.example.toml config.toml
# Edit config.toml and set api_key to a long random string.
docker compose up -d --build
```

**Postgres:**

```bash
cp config.docker.example.toml config.toml
# In config.toml: switch the [database] block to postgres (the example file
# has the lines pre-filled and commented out) and set the password in the URL.
cp .env.example .env
# Edit .env and set POSTGRES_PASSWORD to match the URL in config.toml.
docker compose --profile postgres up -d --build
```

**HTTPS / external access:**

Use this when the Reforger server runs on a different machine, or when you want the dashboard reachable from the public internet. An nginx reverse proxy fronts the bridge with an automatically-issued Let's Encrypt cert.

Prerequisites:

- A domain with an A/AAAA record pointing at this host.
- Ports `80` and `443` open in the firewall (port 80 is needed for the ACME HTTP-01 challenge and renewals).

```bash
cp nginx/user_conf.d/bridge.conf.example nginx/user_conf.d/bridge.conf
# Replace every `bridge.example.com` in bridge.conf with your real domain.
cp .env.example .env   # if you haven't already
# Set CERTBOT_EMAIL in .env (Let's Encrypt uses it for expiry warnings).
docker compose --profile https up -d --build
```

Combine with `--profile postgres` to enable both. Once nginx has a certificate, the bridge is reachable at `https://your.domain/`. The container also stays bound to `127.0.0.1:8787` on the host, so a same-machine Reforger server can keep talking to `http://127.0.0.1:8787` with no extra TLS overhead — point the addon at the public URL only when the game server lives elsewhere. The api_key gates `/player/*` and `/leaderboard` either way; the supplied nginx config strips query strings from access logs so the key isn't recorded server-side.

Tail logs with `docker compose logs -f bridge` (add `nginx` for the proxy). Data lives in named volumes (`bridge-data` for SQLite, `postgres-data` for Postgres, `nginx-secrets` for issued certs) so `docker compose down` is non-destructive; use `docker compose down -v` to wipe.

### Option B — Manual build

Prerequisite: Rust toolchain — install via https://rustup.rs.

**Windows:**

```powershell
git clone <repo-url>
cd TBKSavedProgressionBridge
cargo build --release
copy config.example.toml config.toml
# Edit config.toml and set api_key to a long random string.
.\target\release\tbk-progression-bridge.exe
```

**Linux:**

```bash
git clone <repo-url>
cd TBKSavedProgressionBridge
cargo build --release
cp config.example.toml config.toml
# Edit config.toml and set api_key to a long random string.
./target/release/tbk-progression-bridge
```

## Configuration

`config.toml` is TOML with three sections.

### `[server]`

| Key | Description |
|---|---|
| `bind_address` | Address and port to listen on. Default `"127.0.0.1:8787"` for the manual build (keeps it off the network). For Docker, the bundled `config.docker.example.toml` uses `"0.0.0.0:8787"` so traffic from other containers (and the published host port) can reach it — host-side exposure is controlled by the `ports:` mapping in `docker-compose.yml`, not this setting. |
| `api_key` | Shared secret. Required for `/player/*` and `/leaderboard` endpoints. **Must be changed from the default value.** |

### `[database]`

| Key | Description |
|---|---|
| `backend` | Either `"sqlite"` or `"postgres"`. |
| `sqlite_path` | Path to the SQLite file. Created on first run. Required when `backend = "sqlite"`. |
| `postgres_url` | libpq-style connection string, e.g. `"postgres://tbk:secret@localhost:5432/tbk_progression"`. Required when `backend = "postgres"`. |
| `max_connections` | Maximum pooled DB connections. Default `10`. |

### `[dashboard]` (optional)

Customizes the public web dashboard at `/`. Omit the whole section, or any individual field, to keep the defaults.

| Key | Default |
|---|---|
| `title` | `"TBK Progression"` |
| `subtitle` | `"Live stats from the Saved Progression bridge"` |

## Switching to PostgreSQL

```toml
[database]
backend = "postgres"
# Manual build, Postgres on the same host:
postgres_url = "postgres://tbk:secret@localhost:5432/tbk_progression"
# Docker Compose with --profile postgres (host is the service name):
# postgres_url = "postgres://tbk:secret@postgres:5432/tbk_progression"
```

Restart the bridge. The schema is created automatically on first start.

## Authentication

`/`, `/api/*`, and `/health` are public (read-only). Everything under `/player/*` and `/leaderboard` requires the configured `api_key`. The bridge accepts the key two ways:

- **Header:** `X-Api-Key: <api_key>` — preferred for direct testing (curl, Postman).
- **Query parameter:** `?api_key=<api_key>` — used by the Arma Reforger addon. Reforger's `RestApi` strips custom request headers before they reach the network, so the header form does not work from inside the game.

## Endpoints

| Method | Path | Auth | Body | Returns |
|---|---|---|---|---|
| GET  | `/` | public | — | HTML dashboard |
| GET  | `/api/stats` | public | — | `{ "aggregate": { ... } }` |
| GET  | `/api/leaderboard?limit=25&offset=0&q=alice` | public | — | `{ "entries": [ ... ], "total": N, "limit": L, "offset": O }` |
| GET  | `/api/player/:uid` | public | — | `PlayerRecord` JSON or 404 |
| GET  | `/health` | public | — | `ok` |
| GET  | `/player/:uid` | api_key | — | `PlayerRecord` JSON or 404 |
| POST | `/player/:uid/increment` | api_key | `{ "last_known_name": "...", "kills": 1, ... }` | updated `PlayerRecord` |
| POST | `/player/batch-increment` | api_key | `{ "entries": [ { "player_uid": "...", "last_known_name": "...", ... } ] }` | `{ "applied": N }` |
| GET  | `/leaderboard?limit=100` | api_key | — | `{ "entries": [ ... ] }` |

### Stat fields

Increment bodies accept any of these fields. All are optional, default to `0`, and are applied as **deltas** (additive — values may also be negative):

| Field | Description |
|---|---|
| `total_score` | Score points earned |
| `kills` | Player kills |
| `ai_kills` | AI kills |
| `deaths` | Deaths |
| `objectives` | Objectives completed |
| `playtime_seconds` | Time played, in seconds |

`POST /player/:uid/increment` also requires `last_known_name` (non-empty string).

## Deploying alongside an Arma Reforger server

Two supported topologies:

**Same machine as the dedicated server (simplest).** Run the bridge on the Reforger host, keep it bound to localhost, and point the addon at `http://127.0.0.1:8787` in `TBK_ProgressionConfig.conf`. With Docker Compose this is the default profile — the published port is `127.0.0.1:8787:8787`, so nothing external reaches the bridge. If you also want a public dashboard, enable `--profile https` (see [HTTPS / external access](#option-a--docker-compose-recommended)); the local addon can keep using the loopback URL even with HTTPS active.

**Separate machine from the dedicated server.** Run the bridge on its own host with `--profile https` enabled and DNS pointed at it. Set the addon's URL to `https://your.domain` in `TBK_ProgressionConfig.conf`. The api_key gates `/player/*` and `/leaderboard`, so HTTPS + a strong key is the only thing standing between the internet and your write endpoints — pick a long random string.

In both cases the addon authenticates via the `?api_key=` query-param form (see [Authentication](#authentication)) — Reforger's `RestApi` strips custom headers before they leave the game.

## Smoke testing

`Tools/smoke-test.ps1` (Windows PowerShell) exercises every endpoint against either backend. On Linux you can hit the same routes with `curl` — see the script for the request shapes.

## Development

- `cargo check` — fast type-check.
- `cargo build --release` — optimized binary at `target/release/tbk-progression-bridge[.exe]`.
- See `.gitignore` for the full list of ignored paths. Notable: build output (`target/`, `Cargo.lock`), local databases (`*.db*`), and anything containing secrets (`config.toml`, `.env`, `nginx/user_conf.d/bridge.conf`).

## Screenshots

![Leaderboard view with aggregate stat tiles and a searchable player table](screenshots/leaderboard.png)

![Player detail modal showing per-player score, kills, AI kills, deaths, objectives, playtime, and first/last seen timestamps](screenshots/player-stats.png)
