# TBK Progression Bridge

A small REST service that lets the **TBK - Saved Progression** Arma Reforger addon persist player stats to either **SQLite** or **PostgreSQL**.

The Reforger game itself cannot speak SQL directly from Enforce Script, so this service sits between the game server and your database. The addon uses Reforger's `RestApi` to POST stat deltas to this bridge over HTTP.

## Quick start (SQLite, single binary)

1. Install Rust (https://rustup.rs) and run `cargo build --release`.
2. Copy `config.example.toml` next to the resulting `target/release/tbk-progression-bridge.exe` and rename it `config.toml`.
3. Edit `config.toml`:
   - Set `api_key` to a long random string.
   - Leave `backend = "sqlite"` and pick a `sqlite_path`.
4. Run the binary. It will create the SQLite file and the schema on first start.

```
.\tbk-progression-bridge.exe --config config.toml
```

5. Test it (from another shell):

```
curl http://127.0.0.1:8787/health
# -> ok
```

## Switching to PostgreSQL

In `config.toml`:

```toml
[database]
backend = "postgres"
postgres_url = "postgres://tbk:secret@localhost:5432/tbk_progression"
```

Restart the bridge. The schema is created automatically on first start.

## Endpoints

`/`, `/api/*`, and `/health` are public. All other endpoints require the header `X-Api-Key: <your api_key>`.

| Method | Path | Body | Returns |
|---|---|---|---|
| GET  | `/` | — | HTML dashboard (stats + leaderboard) |
| GET  | `/api/stats` | — | `{ "aggregate": { ... }, "leaderboard": [ ... ] }` |
| GET  | `/api/player/:uid` | — | `PlayerRecord` JSON or 404 |
| GET  | `/health` | — | `ok` |
| GET  | `/player/:uid` | — | `PlayerRecord` JSON or 404 |
| POST | `/player/:uid/increment` | `{ "last_known_name": "...", "kills": 1, "total_score": 10, ... }` | updated `PlayerRecord` |
| POST | `/player/batch-increment` | `{ "entries": [ { "player_uid": "...", "last_known_name": "...", "kills": 1, ... } ] }` | `{ "applied": N }` |
| GET  | `/leaderboard?limit=100` | — | `{ "entries": [ ... ] }` |

All stat fields in increment bodies are optional and default to zero. They are **deltas**, not absolute values.

## Deploying alongside an Arma Reforger server

- Run this binary on the same machine as the dedicated server.
- Keep `bind_address = "127.0.0.1:8787"` so it isn't reachable from the public internet.
- Point the addon at `http://127.0.0.1:8787` (configured in `TBK_ProgressionConfig.conf` inside the addon).

## Smoke testing

Once the bridge is running, you can run `Tools\smoke-test.ps1` to verify all endpoints work end-to-end against either DB backend.
