# TBK Progression Bridge

A small REST service that lets the **TBK - Saved Progression** Arma Reforger addon persist player stats to either **SQLite** or **PostgreSQL**, and exposes a public web dashboard with live stats and a searchable leaderboard.

The Reforger game itself cannot speak SQL directly from Enforce Script, so this service sits between the game server and your database. The addon uses Reforger's `RestApi` to POST stat deltas to this bridge over HTTP.

## Install

### Prerequisites

- Rust toolchain — install via https://rustup.rs

The binary looks for `config.toml` in the current directory by default, so the simplest layout is to keep `config.toml` in the repo root and run the binary from there. Use `--config <path>` if you want it elsewhere.

### Windows

```powershell
git clone <repo-url>
cd TBKSavedProgressionBridge
cargo build --release
copy config.example.toml config.toml
# Edit config.toml and set api_key to a long random string.
.\target\release\tbk-progression-bridge.exe
```

### Linux

```bash
git clone <repo-url>
cd TBKSavedProgressionBridge
cargo build --release
cp config.example.toml config.toml
# Edit config.toml and set api_key to a long random string.
./target/release/tbk-progression-bridge
```

On first run the bridge creates the SQLite file (if using SQLite) and the schema in either backend.

Verify it's running:

```bash
curl http://127.0.0.1:8787/health
# -> ok
```

Open `http://127.0.0.1:8787/` in a browser to see the dashboard.

## Configuration

`config.toml` is TOML with three sections.

### `[server]`

| Key | Description |
|---|---|
| `bind_address` | Address and port to listen on. Default `"127.0.0.1:8787"`. Use `"0.0.0.0:8787"` only when reachable through a firewall or reverse proxy. |
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
postgres_url = "postgres://tbk:secret@localhost:5432/tbk_progression"
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

- Run this binary on the same machine as the dedicated server.
- Keep `bind_address = "127.0.0.1:8787"` so it isn't reachable from the public internet.
- Point the addon at `http://127.0.0.1:8787` in `TBK_ProgressionConfig.conf` inside the addon.
- The addon authenticates via the `?api_key=` query-param form (see [Authentication](#authentication)).
- If you want the dashboard publicly visible, put a reverse proxy (nginx, Caddy) in front of the bridge and only proxy `/`, `/api/*`, and `/health` — keep the gated endpoints on localhost.

## Smoke testing

`Tools/smoke-test.ps1` (Windows PowerShell) exercises every endpoint against either backend. On Linux you can hit the same routes with `curl` — see the script for the request shapes.

## Development

- `cargo check` — fast type-check.
- `cargo build --release` — optimized binary at `target/release/tbk-progression-bridge[.exe]`.
- `target/`, `Cargo.lock`, `*.db`, `config.toml`, and `.idea/` are gitignored.
