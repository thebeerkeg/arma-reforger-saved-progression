use crate::config::{Config, DbBackend};
use crate::error::BridgeResult;
use crate::models::{AggregateStats, BatchIncrementEntry, LeaderboardEntry, PlayerRecord, StatDelta};
use async_trait::async_trait;
use sqlx::{postgres::PgPoolOptions, sqlite::SqlitePoolOptions, PgPool, Row, SqlitePool};

#[async_trait]
pub trait Store: Send + Sync {
    async fn migrate(&self) -> BridgeResult<()>;
    async fn get_player(&self, uid: &str) -> BridgeResult<Option<PlayerRecord>>;
    async fn upsert_increment(
        &self,
        uid: &str,
        last_known_name: &str,
        delta: &StatDelta,
    ) -> BridgeResult<PlayerRecord>;
    async fn batch_upsert_increment(
        &self,
        entries: &[BatchIncrementEntry],
    ) -> BridgeResult<usize>;
    async fn leaderboard(&self, limit: i64) -> BridgeResult<Vec<LeaderboardEntry>>;
    async fn aggregate_stats(&self) -> BridgeResult<AggregateStats>;
    async fn leaderboard_paged(
        &self,
        limit: i64,
        offset: i64,
        search: Option<&str>,
    ) -> BridgeResult<(Vec<LeaderboardEntry>, i64)>;
}

// Builds a case-insensitive LIKE pattern with `\` as the escape character.
// Empty/whitespace input matches everything (returns "%"). User % and _ are
// escaped so they're treated as literal characters, not wildcards.
fn build_like_pattern(s: &str) -> String {
    let trimmed = s.trim();
    if trimmed.is_empty() {
        return "%".to_string();
    }
    let mut out = String::with_capacity(trimmed.len() + 2);
    out.push('%');
    for c in trimmed.chars() {
        if matches!(c, '\\' | '%' | '_') {
            out.push('\\');
        }
        for lc in c.to_lowercase() {
            out.push(lc);
        }
    }
    out.push('%');
    out
}

pub enum AnyStore {
    Sqlite(SqliteStore),
    Postgres(PostgresStore),
}

#[async_trait]
impl Store for AnyStore {
    async fn migrate(&self) -> BridgeResult<()> {
        match self {
            AnyStore::Sqlite(s) => s.migrate().await,
            AnyStore::Postgres(p) => p.migrate().await,
        }
    }
    async fn get_player(&self, uid: &str) -> BridgeResult<Option<PlayerRecord>> {
        match self {
            AnyStore::Sqlite(s) => s.get_player(uid).await,
            AnyStore::Postgres(p) => p.get_player(uid).await,
        }
    }
    async fn upsert_increment(
        &self,
        uid: &str,
        last_known_name: &str,
        delta: &StatDelta,
    ) -> BridgeResult<PlayerRecord> {
        match self {
            AnyStore::Sqlite(s) => s.upsert_increment(uid, last_known_name, delta).await,
            AnyStore::Postgres(p) => p.upsert_increment(uid, last_known_name, delta).await,
        }
    }
    async fn batch_upsert_increment(
        &self,
        entries: &[BatchIncrementEntry],
    ) -> BridgeResult<usize> {
        match self {
            AnyStore::Sqlite(s) => s.batch_upsert_increment(entries).await,
            AnyStore::Postgres(p) => p.batch_upsert_increment(entries).await,
        }
    }
    async fn leaderboard(&self, limit: i64) -> BridgeResult<Vec<LeaderboardEntry>> {
        match self {
            AnyStore::Sqlite(s) => s.leaderboard(limit).await,
            AnyStore::Postgres(p) => p.leaderboard(limit).await,
        }
    }
    async fn aggregate_stats(&self) -> BridgeResult<AggregateStats> {
        match self {
            AnyStore::Sqlite(s) => s.aggregate_stats().await,
            AnyStore::Postgres(p) => p.aggregate_stats().await,
        }
    }
    async fn leaderboard_paged(
        &self,
        limit: i64,
        offset: i64,
        search: Option<&str>,
    ) -> BridgeResult<(Vec<LeaderboardEntry>, i64)> {
        match self {
            AnyStore::Sqlite(s) => s.leaderboard_paged(limit, offset, search).await,
            AnyStore::Postgres(p) => p.leaderboard_paged(limit, offset, search).await,
        }
    }
}

pub async fn build_store(cfg: &Config) -> anyhow::Result<AnyStore> {
    match cfg.database.backend {
        DbBackend::Sqlite => {
            let url = format!("sqlite://{}?mode=rwc", cfg.database.sqlite_path);
            let pool = SqlitePoolOptions::new()
                .max_connections(cfg.database.max_connections)
                .connect(&url)
                .await?;
            Ok(AnyStore::Sqlite(SqliteStore { pool }))
        }
        DbBackend::Postgres => {
            let pool = PgPoolOptions::new()
                .max_connections(cfg.database.max_connections)
                .connect(&cfg.database.postgres_url)
                .await?;
            Ok(AnyStore::Postgres(PostgresStore { pool }))
        }
    }
}

// ---------- SQLite ----------

pub struct SqliteStore {
    pool: SqlitePool,
}

fn sqlite_row_to_record(row: &sqlx::sqlite::SqliteRow) -> sqlx::Result<PlayerRecord> {
    Ok(PlayerRecord {
        player_uid: row.try_get("player_uid")?,
        last_known_name: row.try_get("last_known_name")?,
        total_score: row.try_get("total_score")?,
        kills: row.try_get("kills")?,
        ai_kills: row.try_get("ai_kills")?,
        deaths: row.try_get("deaths")?,
        objectives: row.try_get("objectives")?,
        playtime_seconds: row.try_get("playtime_seconds")?,
        first_seen: row.try_get("first_seen")?,
        last_seen: row.try_get("last_seen")?,
    })
}

#[async_trait]
impl Store for SqliteStore {
    async fn migrate(&self) -> BridgeResult<()> {
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS players (
                player_uid       TEXT PRIMARY KEY,
                last_known_name  TEXT NOT NULL,
                total_score      INTEGER NOT NULL DEFAULT 0,
                kills            INTEGER NOT NULL DEFAULT 0,
                ai_kills         INTEGER NOT NULL DEFAULT 0,
                deaths           INTEGER NOT NULL DEFAULT 0,
                objectives       INTEGER NOT NULL DEFAULT 0,
                playtime_seconds INTEGER NOT NULL DEFAULT 0,
                first_seen       TIMESTAMP NOT NULL,
                last_seen        TIMESTAMP NOT NULL
            );
            "#,
        )
        .execute(&self.pool)
        .await?;
        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_players_total_score ON players(total_score DESC);",
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn get_player(&self, uid: &str) -> BridgeResult<Option<PlayerRecord>> {
        let row = sqlx::query(
            "SELECT player_uid, last_known_name, total_score, kills, ai_kills, deaths,
                    objectives, playtime_seconds, first_seen, last_seen
             FROM players WHERE player_uid = ?",
        )
        .bind(uid)
        .fetch_optional(&self.pool)
        .await?;
        match row {
            Some(r) => Ok(Some(sqlite_row_to_record(&r)?)),
            None => Ok(None),
        }
    }

    async fn upsert_increment(
        &self,
        uid: &str,
        last_known_name: &str,
        delta: &StatDelta,
    ) -> BridgeResult<PlayerRecord> {
        let now = chrono::Utc::now();
        let mut tx = self.pool.begin().await?;
        sqlx::query(
            r#"
            INSERT INTO players (player_uid, last_known_name, total_score, kills, ai_kills,
                                 deaths, objectives, playtime_seconds, first_seen, last_seen)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            ON CONFLICT(player_uid) DO UPDATE SET
                last_known_name  = excluded.last_known_name,
                total_score      = total_score      + excluded.total_score,
                kills            = kills            + excluded.kills,
                ai_kills         = ai_kills         + excluded.ai_kills,
                deaths           = deaths           + excluded.deaths,
                objectives       = objectives       + excluded.objectives,
                playtime_seconds = playtime_seconds + excluded.playtime_seconds,
                last_seen        = excluded.last_seen;
            "#,
        )
        .bind(uid)
        .bind(last_known_name)
        .bind(delta.total_score)
        .bind(delta.kills)
        .bind(delta.ai_kills)
        .bind(delta.deaths)
        .bind(delta.objectives)
        .bind(delta.playtime_seconds)
        .bind(now)
        .bind(now)
        .execute(&mut *tx)
        .await?;

        let row = sqlx::query(
            "SELECT player_uid, last_known_name, total_score, kills, ai_kills, deaths,
                    objectives, playtime_seconds, first_seen, last_seen
             FROM players WHERE player_uid = ?",
        )
        .bind(uid)
        .fetch_one(&mut *tx)
        .await?;
        let rec = sqlite_row_to_record(&row)?;
        tx.commit().await?;
        Ok(rec)
    }

    async fn batch_upsert_increment(
        &self,
        entries: &[BatchIncrementEntry],
    ) -> BridgeResult<usize> {
        if entries.is_empty() {
            return Ok(0);
        }
        let now = chrono::Utc::now();
        let mut tx = self.pool.begin().await?;
        for e in entries {
            sqlx::query(
                r#"
                INSERT INTO players (player_uid, last_known_name, total_score, kills, ai_kills,
                                     deaths, objectives, playtime_seconds, first_seen, last_seen)
                VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
                ON CONFLICT(player_uid) DO UPDATE SET
                    last_known_name  = excluded.last_known_name,
                    total_score      = total_score      + excluded.total_score,
                    kills            = kills            + excluded.kills,
                    ai_kills         = ai_kills         + excluded.ai_kills,
                    deaths           = deaths           + excluded.deaths,
                    objectives       = objectives       + excluded.objectives,
                    playtime_seconds = playtime_seconds + excluded.playtime_seconds,
                    last_seen        = excluded.last_seen;
                "#,
            )
            .bind(&e.player_uid)
            .bind(&e.last_known_name)
            .bind(e.delta.total_score)
            .bind(e.delta.kills)
            .bind(e.delta.ai_kills)
            .bind(e.delta.deaths)
            .bind(e.delta.objectives)
            .bind(e.delta.playtime_seconds)
            .bind(now)
            .bind(now)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok(entries.len())
    }

    async fn leaderboard(&self, limit: i64) -> BridgeResult<Vec<LeaderboardEntry>> {
        let rows = sqlx::query(
            "SELECT player_uid, last_known_name, total_score, kills, deaths
             FROM players ORDER BY total_score DESC LIMIT ?",
        )
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;
        let mut out = Vec::with_capacity(rows.len());
        for (i, r) in rows.iter().enumerate() {
            out.push(LeaderboardEntry {
                rank: (i as i64) + 1,
                player_uid: r.try_get("player_uid")?,
                last_known_name: r.try_get("last_known_name")?,
                total_score: r.try_get("total_score")?,
                kills: r.try_get("kills")?,
                deaths: r.try_get("deaths")?,
            });
        }
        Ok(out)
    }

    async fn aggregate_stats(&self) -> BridgeResult<AggregateStats> {
        let row = sqlx::query(
            "SELECT
                COUNT(*)                                AS total_players,
                COALESCE(SUM(total_score), 0)           AS total_score,
                COALESCE(SUM(kills), 0)                 AS total_kills,
                COALESCE(SUM(ai_kills), 0)              AS total_ai_kills,
                COALESCE(SUM(deaths), 0)                AS total_deaths,
                COALESCE(SUM(objectives), 0)            AS total_objectives,
                COALESCE(SUM(playtime_seconds), 0)      AS total_playtime_seconds
             FROM players",
        )
        .fetch_one(&self.pool)
        .await?;
        Ok(AggregateStats {
            total_players: row.try_get("total_players")?,
            total_score: row.try_get("total_score")?,
            total_kills: row.try_get("total_kills")?,
            total_ai_kills: row.try_get("total_ai_kills")?,
            total_deaths: row.try_get("total_deaths")?,
            total_objectives: row.try_get("total_objectives")?,
            total_playtime_seconds: row.try_get("total_playtime_seconds")?,
        })
    }

    async fn leaderboard_paged(
        &self,
        limit: i64,
        offset: i64,
        search: Option<&str>,
    ) -> BridgeResult<(Vec<LeaderboardEntry>, i64)> {
        // ROW_NUMBER lives in a subquery so ranks are absolute (computed across the
        // full table) rather than relative to the filtered/paged window.
        let pattern = build_like_pattern(search.unwrap_or(""));
        let rows = sqlx::query(
            r#"
            SELECT player_uid, last_known_name, total_score, kills, deaths, rank
            FROM (
                SELECT player_uid, last_known_name, total_score, kills, deaths,
                       ROW_NUMBER() OVER (ORDER BY total_score DESC, player_uid) AS rank
                FROM players
            )
            WHERE LOWER(last_known_name) LIKE ? ESCAPE '\'
            ORDER BY rank
            LIMIT ? OFFSET ?
            "#,
        )
        .bind(&pattern)
        .bind(limit)
        .bind(offset)
        .fetch_all(&self.pool)
        .await?;

        let total: i64 = sqlx::query_scalar(
            r#"SELECT COUNT(*) FROM players WHERE LOWER(last_known_name) LIKE ? ESCAPE '\'"#,
        )
        .bind(&pattern)
        .fetch_one(&self.pool)
        .await?;

        let mut entries = Vec::with_capacity(rows.len());
        for r in &rows {
            entries.push(LeaderboardEntry {
                rank: r.try_get("rank")?,
                player_uid: r.try_get("player_uid")?,
                last_known_name: r.try_get("last_known_name")?,
                total_score: r.try_get("total_score")?,
                kills: r.try_get("kills")?,
                deaths: r.try_get("deaths")?,
            });
        }
        Ok((entries, total))
    }
}

// ---------- Postgres ----------

pub struct PostgresStore {
    pool: PgPool,
}

fn pg_row_to_record(row: &sqlx::postgres::PgRow) -> sqlx::Result<PlayerRecord> {
    Ok(PlayerRecord {
        player_uid: row.try_get("player_uid")?,
        last_known_name: row.try_get("last_known_name")?,
        total_score: row.try_get("total_score")?,
        kills: row.try_get("kills")?,
        ai_kills: row.try_get("ai_kills")?,
        deaths: row.try_get("deaths")?,
        objectives: row.try_get("objectives")?,
        playtime_seconds: row.try_get("playtime_seconds")?,
        first_seen: row.try_get("first_seen")?,
        last_seen: row.try_get("last_seen")?,
    })
}

#[async_trait]
impl Store for PostgresStore {
    async fn migrate(&self) -> BridgeResult<()> {
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS players (
                player_uid       TEXT PRIMARY KEY,
                last_known_name  TEXT NOT NULL,
                total_score      BIGINT NOT NULL DEFAULT 0,
                kills            BIGINT NOT NULL DEFAULT 0,
                ai_kills         BIGINT NOT NULL DEFAULT 0,
                deaths           BIGINT NOT NULL DEFAULT 0,
                objectives       BIGINT NOT NULL DEFAULT 0,
                playtime_seconds BIGINT NOT NULL DEFAULT 0,
                first_seen       TIMESTAMPTZ NOT NULL,
                last_seen        TIMESTAMPTZ NOT NULL
            );
            "#,
        )
        .execute(&self.pool)
        .await?;
        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_players_total_score ON players(total_score DESC);",
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn get_player(&self, uid: &str) -> BridgeResult<Option<PlayerRecord>> {
        let row = sqlx::query(
            "SELECT player_uid, last_known_name, total_score, kills, ai_kills, deaths,
                    objectives, playtime_seconds, first_seen, last_seen
             FROM players WHERE player_uid = $1",
        )
        .bind(uid)
        .fetch_optional(&self.pool)
        .await?;
        match row {
            Some(r) => Ok(Some(pg_row_to_record(&r)?)),
            None => Ok(None),
        }
    }

    async fn upsert_increment(
        &self,
        uid: &str,
        last_known_name: &str,
        delta: &StatDelta,
    ) -> BridgeResult<PlayerRecord> {
        let now = chrono::Utc::now();
        let row = sqlx::query(
            r#"
            INSERT INTO players (player_uid, last_known_name, total_score, kills, ai_kills,
                                 deaths, objectives, playtime_seconds, first_seen, last_seen)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
            ON CONFLICT (player_uid) DO UPDATE SET
                last_known_name  = EXCLUDED.last_known_name,
                total_score      = players.total_score      + EXCLUDED.total_score,
                kills            = players.kills            + EXCLUDED.kills,
                ai_kills         = players.ai_kills         + EXCLUDED.ai_kills,
                deaths           = players.deaths           + EXCLUDED.deaths,
                objectives       = players.objectives       + EXCLUDED.objectives,
                playtime_seconds = players.playtime_seconds + EXCLUDED.playtime_seconds,
                last_seen        = EXCLUDED.last_seen
            RETURNING player_uid, last_known_name, total_score, kills, ai_kills, deaths,
                      objectives, playtime_seconds, first_seen, last_seen;
            "#,
        )
        .bind(uid)
        .bind(last_known_name)
        .bind(delta.total_score)
        .bind(delta.kills)
        .bind(delta.ai_kills)
        .bind(delta.deaths)
        .bind(delta.objectives)
        .bind(delta.playtime_seconds)
        .bind(now)
        .bind(now)
        .fetch_one(&self.pool)
        .await?;
        Ok(pg_row_to_record(&row)?)
    }

    async fn batch_upsert_increment(
        &self,
        entries: &[BatchIncrementEntry],
    ) -> BridgeResult<usize> {
        if entries.is_empty() {
            return Ok(0);
        }
        let now = chrono::Utc::now();
        let mut tx = self.pool.begin().await?;
        for e in entries {
            sqlx::query(
                r#"
                INSERT INTO players (player_uid, last_known_name, total_score, kills, ai_kills,
                                     deaths, objectives, playtime_seconds, first_seen, last_seen)
                VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
                ON CONFLICT (player_uid) DO UPDATE SET
                    last_known_name  = EXCLUDED.last_known_name,
                    total_score      = players.total_score      + EXCLUDED.total_score,
                    kills            = players.kills            + EXCLUDED.kills,
                    ai_kills         = players.ai_kills         + EXCLUDED.ai_kills,
                    deaths           = players.deaths           + EXCLUDED.deaths,
                    objectives       = players.objectives       + EXCLUDED.objectives,
                    playtime_seconds = players.playtime_seconds + EXCLUDED.playtime_seconds,
                    last_seen        = EXCLUDED.last_seen;
                "#,
            )
            .bind(&e.player_uid)
            .bind(&e.last_known_name)
            .bind(e.delta.total_score)
            .bind(e.delta.kills)
            .bind(e.delta.ai_kills)
            .bind(e.delta.deaths)
            .bind(e.delta.objectives)
            .bind(e.delta.playtime_seconds)
            .bind(now)
            .bind(now)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok(entries.len())
    }

    async fn leaderboard(&self, limit: i64) -> BridgeResult<Vec<LeaderboardEntry>> {
        let rows = sqlx::query(
            "SELECT player_uid, last_known_name, total_score, kills, deaths
             FROM players ORDER BY total_score DESC LIMIT $1",
        )
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;
        let mut out = Vec::with_capacity(rows.len());
        for (i, r) in rows.iter().enumerate() {
            out.push(LeaderboardEntry {
                rank: (i as i64) + 1,
                player_uid: r.try_get("player_uid")?,
                last_known_name: r.try_get("last_known_name")?,
                total_score: r.try_get("total_score")?,
                kills: r.try_get("kills")?,
                deaths: r.try_get("deaths")?,
            });
        }
        Ok(out)
    }

    async fn aggregate_stats(&self) -> BridgeResult<AggregateStats> {
        // Postgres SUM(BIGINT) returns NUMERIC; cast back to BIGINT so we can read it as i64.
        let row = sqlx::query(
            "SELECT
                COUNT(*)                                          AS total_players,
                COALESCE(SUM(total_score), 0)::BIGINT             AS total_score,
                COALESCE(SUM(kills), 0)::BIGINT                   AS total_kills,
                COALESCE(SUM(ai_kills), 0)::BIGINT                AS total_ai_kills,
                COALESCE(SUM(deaths), 0)::BIGINT                  AS total_deaths,
                COALESCE(SUM(objectives), 0)::BIGINT              AS total_objectives,
                COALESCE(SUM(playtime_seconds), 0)::BIGINT        AS total_playtime_seconds
             FROM players",
        )
        .fetch_one(&self.pool)
        .await?;
        Ok(AggregateStats {
            total_players: row.try_get("total_players")?,
            total_score: row.try_get("total_score")?,
            total_kills: row.try_get("total_kills")?,
            total_ai_kills: row.try_get("total_ai_kills")?,
            total_deaths: row.try_get("total_deaths")?,
            total_objectives: row.try_get("total_objectives")?,
            total_playtime_seconds: row.try_get("total_playtime_seconds")?,
        })
    }

    async fn leaderboard_paged(
        &self,
        limit: i64,
        offset: i64,
        search: Option<&str>,
    ) -> BridgeResult<(Vec<LeaderboardEntry>, i64)> {
        let pattern = build_like_pattern(search.unwrap_or(""));
        let rows = sqlx::query(
            r#"
            SELECT player_uid, last_known_name, total_score, kills, deaths, rank
            FROM (
                SELECT player_uid, last_known_name, total_score, kills, deaths,
                       ROW_NUMBER() OVER (ORDER BY total_score DESC, player_uid) AS rank
                FROM players
            ) p
            WHERE LOWER(last_known_name) LIKE $1 ESCAPE '\'
            ORDER BY rank
            LIMIT $2 OFFSET $3
            "#,
        )
        .bind(&pattern)
        .bind(limit)
        .bind(offset)
        .fetch_all(&self.pool)
        .await?;

        let total: i64 = sqlx::query_scalar(
            r#"SELECT COUNT(*) FROM players WHERE LOWER(last_known_name) LIKE $1 ESCAPE '\'"#,
        )
        .bind(&pattern)
        .fetch_one(&self.pool)
        .await?;

        let mut entries = Vec::with_capacity(rows.len());
        for r in &rows {
            entries.push(LeaderboardEntry {
                rank: r.try_get("rank")?,
                player_uid: r.try_get("player_uid")?,
                last_known_name: r.try_get("last_known_name")?,
                total_score: r.try_get("total_score")?,
                kills: r.try_get("kills")?,
                deaths: r.try_get("deaths")?,
            });
        }
        Ok((entries, total))
    }
}
