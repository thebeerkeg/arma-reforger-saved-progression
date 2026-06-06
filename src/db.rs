use crate::config::{Config, DbBackend};
use crate::error::BridgeResult;
use crate::models::{
    AggregateStats, BatchIncrementEntry, FinalizeMatchRequest, LeaderboardEntry, Match,
    MatchFactionScore, MatchListEntry, MatchPlayer, MatchSummary, PlayerRecord, StatDelta,
};
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

    // Match tracking — only finished matches are persisted. The addon
    // accumulates per-match data locally during play and posts the whole match
    // in one atomic finalize call.
    async fn finalize_match(&self, req: &FinalizeMatchRequest) -> BridgeResult<Match>;
    async fn list_matches(
        &self,
        limit: i64,
        offset: i64,
    ) -> BridgeResult<(Vec<MatchListEntry>, i64)>;
    async fn get_match_summary(&self, id: &str) -> BridgeResult<Option<MatchSummary>>;
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
    async fn finalize_match(&self, req: &FinalizeMatchRequest) -> BridgeResult<Match> {
        match self {
            AnyStore::Sqlite(s) => s.finalize_match(req).await,
            AnyStore::Postgres(p) => p.finalize_match(req).await,
        }
    }
    async fn list_matches(
        &self,
        limit: i64,
        offset: i64,
    ) -> BridgeResult<(Vec<MatchListEntry>, i64)> {
        match self {
            AnyStore::Sqlite(s) => s.list_matches(limit, offset).await,
            AnyStore::Postgres(p) => p.list_matches(limit, offset).await,
        }
    }
    async fn get_match_summary(&self, id: &str) -> BridgeResult<Option<MatchSummary>> {
        match self {
            AnyStore::Sqlite(s) => s.get_match_summary(id).await,
            AnyStore::Postgres(p) => p.get_match_summary(id).await,
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

fn sqlite_row_to_match(row: &sqlx::sqlite::SqliteRow) -> sqlx::Result<Match> {
    Ok(Match {
        id: row.try_get("id")?,
        scenario: row.try_get("scenario")?,
        start_time: row.try_get("start_time")?,
        end_time: row.try_get("end_time")?,
        winning_faction: row.try_get("winning_faction")?,
        end_reason: row.try_get("end_reason")?,
    })
}

fn sqlite_row_to_match_player(row: &sqlx::sqlite::SqliteRow) -> sqlx::Result<MatchPlayer> {
    Ok(MatchPlayer {
        match_id: row.try_get("match_id")?,
        player_uid: row.try_get("player_uid")?,
        last_known_name: row.try_get("last_known_name")?,
        faction: row.try_get("faction")?,
        total_score: row.try_get("total_score")?,
        kills: row.try_get("kills")?,
        ai_kills: row.try_get("ai_kills")?,
        deaths: row.try_get("deaths")?,
        objectives: row.try_get("objectives")?,
        playtime_seconds: row.try_get("playtime_seconds")?,
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

        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS matches (
                id              TEXT PRIMARY KEY,
                scenario        TEXT NOT NULL,
                start_time      TIMESTAMP NOT NULL,
                end_time        TIMESTAMP,
                winning_faction TEXT,
                end_reason      TEXT
            );
            "#,
        )
        .execute(&self.pool)
        .await?;
        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_matches_start_time ON matches(start_time DESC);",
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS match_players (
                match_id         TEXT NOT NULL,
                player_uid       TEXT NOT NULL,
                faction          TEXT NOT NULL,
                last_known_name  TEXT NOT NULL,
                total_score      INTEGER NOT NULL DEFAULT 0,
                kills            INTEGER NOT NULL DEFAULT 0,
                ai_kills         INTEGER NOT NULL DEFAULT 0,
                deaths           INTEGER NOT NULL DEFAULT 0,
                objectives       INTEGER NOT NULL DEFAULT 0,
                playtime_seconds INTEGER NOT NULL DEFAULT 0,
                PRIMARY KEY (match_id, player_uid, faction),
                FOREIGN KEY (match_id) REFERENCES matches(id) ON DELETE CASCADE
            );
            "#,
        )
        .execute(&self.pool)
        .await?;
        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_match_players_match ON match_players(match_id);",
        )
        .execute(&self.pool)
        .await?;
        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_match_players_uid ON match_players(player_uid);",
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
        let row = sqlx::query(
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
                last_seen        = excluded.last_seen
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
        Ok(sqlite_row_to_record(&row)?)
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
            "SELECT player_uid, last_known_name, last_seen, total_score, kills, ai_kills, deaths
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
                last_seen: r.try_get("last_seen")?,
                total_score: r.try_get("total_score")?,
                kills: r.try_get("kills")?,
                ai_kills: r.try_get("ai_kills")?,
                deaths: r.try_get("deaths")?,
            });
        }
        Ok(out)
    }

    async fn aggregate_stats(&self) -> BridgeResult<AggregateStats> {
        let now = chrono::Utc::now();
        let online_since = now - chrono::Duration::minutes(2);
        let online_until = now + chrono::Duration::seconds(30);
        let row = sqlx::query(
            "SELECT
                COUNT(*)                                AS total_players,
                COUNT(CASE WHEN last_seen >= ? AND last_seen <= ? THEN 1 END)
                                                        AS total_online_players,
                COALESCE(SUM(total_score), 0)           AS total_score,
                COALESCE(SUM(kills), 0)                 AS total_kills,
                COALESCE(SUM(ai_kills), 0)              AS total_ai_kills,
                COALESCE(SUM(deaths), 0)                AS total_deaths,
                COALESCE(SUM(objectives), 0)            AS total_objectives,
                COALESCE(SUM(playtime_seconds), 0)      AS total_playtime_seconds
             FROM players",
        )
        .bind(online_since)
        .bind(online_until)
        .fetch_one(&self.pool)
        .await?;
        Ok(AggregateStats {
            total_players: row.try_get("total_players")?,
            total_online_players: row.try_get("total_online_players")?,
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
            SELECT player_uid, last_known_name, last_seen, total_score, kills, ai_kills, deaths, rank
            FROM (
                SELECT player_uid, last_known_name, last_seen, total_score, kills, ai_kills, deaths,
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
                last_seen: r.try_get("last_seen")?,
                total_score: r.try_get("total_score")?,
                kills: r.try_get("kills")?,
                ai_kills: r.try_get("ai_kills")?,
                deaths: r.try_get("deaths")?,
            });
        }
        Ok((entries, total))
    }

    async fn finalize_match(&self, req: &FinalizeMatchRequest) -> BridgeResult<Match> {
        let now = chrono::Utc::now();
        let start_time = req.start_time.unwrap_or(now);
        let end_time = req.end_time.unwrap_or(now);

        let mut tx = self.pool.begin().await?;

        // The matches row is full-overwritten on conflict — finalize is the
        // authoritative write for a match (no live data can have created a
        // partial row anymore).
        sqlx::query(
            r#"
            INSERT INTO matches (id, scenario, start_time, end_time, winning_faction, end_reason)
            VALUES (?, ?, ?, ?, ?, ?)
            ON CONFLICT(id) DO UPDATE SET
                scenario        = excluded.scenario,
                start_time      = excluded.start_time,
                end_time        = excluded.end_time,
                winning_faction = excluded.winning_faction,
                end_reason      = excluded.end_reason;
            "#,
        )
        .bind(&req.id)
        .bind(&req.scenario)
        .bind(start_time)
        .bind(end_time)
        .bind(&req.winning_faction)
        .bind(&req.end_reason)
        .execute(&mut *tx)
        .await?;

        for p in &req.players {
            // Faction can theoretically be empty (player who never picked a
            // side but somehow earned stats). Skip those — composite key
            // requires a non-empty faction and we'd otherwise UPSERT them all
            // into the same '' row.
            if p.faction.is_empty() {
                continue;
            }
            sqlx::query(
                r#"
                INSERT INTO match_players (match_id, player_uid, faction, last_known_name,
                                           total_score, kills, ai_kills, deaths, objectives, playtime_seconds)
                VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
                ON CONFLICT(match_id, player_uid, faction) DO UPDATE SET
                    last_known_name  = excluded.last_known_name,
                    total_score      = excluded.total_score,
                    kills            = excluded.kills,
                    ai_kills         = excluded.ai_kills,
                    deaths           = excluded.deaths,
                    objectives       = excluded.objectives,
                    playtime_seconds = excluded.playtime_seconds;
                "#,
            )
            .bind(&req.id)
            .bind(&p.player_uid)
            .bind(&p.faction)
            .bind(&p.last_known_name)
            .bind(p.total_score)
            .bind(p.kills)
            .bind(p.ai_kills)
            .bind(p.deaths)
            .bind(p.objectives)
            .bind(p.playtime_seconds)
            .execute(&mut *tx)
            .await?;
        }

        let row = sqlx::query(
            "SELECT id, scenario, start_time, end_time, winning_faction, end_reason
             FROM matches WHERE id = ?",
        )
        .bind(&req.id)
        .fetch_one(&mut *tx)
        .await?;
        let m = sqlite_row_to_match(&row)?;
        tx.commit().await?;
        Ok(m)
    }

    async fn list_matches(
        &self,
        limit: i64,
        offset: i64,
    ) -> BridgeResult<(Vec<MatchListEntry>, i64)> {
        let rows = sqlx::query(
            r#"
            SELECT m.id, m.scenario, m.start_time, m.end_time, m.winning_faction, m.end_reason,
                   COALESCE(p.player_count, 0) AS player_count,
                   COALESCE(p.total_score, 0)  AS total_score
            FROM matches m
            LEFT JOIN (
                SELECT match_id,
                       COUNT(DISTINCT player_uid) AS player_count,
                       SUM(total_score)           AS total_score
                FROM match_players
                GROUP BY match_id
            ) p ON p.match_id = m.id
            ORDER BY COALESCE(m.end_time, m.start_time) DESC
            LIMIT ? OFFSET ?
            "#,
        )
        .bind(limit)
        .bind(offset)
        .fetch_all(&self.pool)
        .await?;

        let total: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM matches")
            .fetch_one(&self.pool)
            .await?;

        let mut out = Vec::with_capacity(rows.len());
        for r in &rows {
            out.push(MatchListEntry {
                id: r.try_get("id")?,
                scenario: r.try_get("scenario")?,
                start_time: r.try_get("start_time")?,
                end_time: r.try_get("end_time")?,
                winning_faction: r.try_get("winning_faction")?,
                end_reason: r.try_get("end_reason")?,
                player_count: r.try_get("player_count")?,
                total_score: r.try_get("total_score")?,
            });
        }
        Ok((out, total))
    }

    async fn get_match_summary(&self, id: &str) -> BridgeResult<Option<MatchSummary>> {
        let match_row = sqlx::query(
            "SELECT id, scenario, start_time, end_time, winning_faction, end_reason
             FROM matches WHERE id = ?",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;
        let Some(match_row) = match_row else {
            return Ok(None);
        };
        let m = sqlite_row_to_match(&match_row)?;

        let player_rows = sqlx::query(
            "SELECT match_id, player_uid, last_known_name, faction, total_score, kills,
                    ai_kills, deaths, objectives, playtime_seconds
             FROM match_players
             WHERE match_id = ?
             ORDER BY total_score DESC, last_known_name",
        )
        .bind(id)
        .fetch_all(&self.pool)
        .await?;

        let faction_rows = sqlx::query(
            r#"
            SELECT faction,
                   COALESCE(SUM(total_score), 0)            AS total_score,
                   COALESCE(SUM(kills), 0)                  AS kills,
                   COALESCE(SUM(deaths), 0)                 AS deaths,
                   COUNT(DISTINCT player_uid)               AS player_count
            FROM match_players
            WHERE match_id = ?
            GROUP BY faction
            ORDER BY total_score DESC
            "#,
        )
        .bind(id)
        .fetch_all(&self.pool)
        .await?;

        let mut players = Vec::with_capacity(player_rows.len());
        for r in &player_rows {
            players.push(sqlite_row_to_match_player(r)?);
        }
        let mut factions = Vec::with_capacity(faction_rows.len());
        for r in &faction_rows {
            factions.push(MatchFactionScore {
                faction: r.try_get("faction")?,
                total_score: r.try_get("total_score")?,
                kills: r.try_get("kills")?,
                deaths: r.try_get("deaths")?,
                player_count: r.try_get("player_count")?,
            });
        }

        Ok(Some(MatchSummary {
            match_meta: m,
            factions,
            players,
        }))
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

fn pg_row_to_match(row: &sqlx::postgres::PgRow) -> sqlx::Result<Match> {
    Ok(Match {
        id: row.try_get("id")?,
        scenario: row.try_get("scenario")?,
        start_time: row.try_get("start_time")?,
        end_time: row.try_get("end_time")?,
        winning_faction: row.try_get("winning_faction")?,
        end_reason: row.try_get("end_reason")?,
    })
}

fn pg_row_to_match_player(row: &sqlx::postgres::PgRow) -> sqlx::Result<MatchPlayer> {
    Ok(MatchPlayer {
        match_id: row.try_get("match_id")?,
        player_uid: row.try_get("player_uid")?,
        last_known_name: row.try_get("last_known_name")?,
        faction: row.try_get("faction")?,
        total_score: row.try_get("total_score")?,
        kills: row.try_get("kills")?,
        ai_kills: row.try_get("ai_kills")?,
        deaths: row.try_get("deaths")?,
        objectives: row.try_get("objectives")?,
        playtime_seconds: row.try_get("playtime_seconds")?,
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

        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS matches (
                id              TEXT PRIMARY KEY,
                scenario        TEXT NOT NULL,
                start_time      TIMESTAMPTZ NOT NULL,
                end_time        TIMESTAMPTZ,
                winning_faction TEXT,
                end_reason      TEXT
            );
            "#,
        )
        .execute(&self.pool)
        .await?;
        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_matches_start_time ON matches(start_time DESC);",
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS match_players (
                match_id         TEXT NOT NULL,
                player_uid       TEXT NOT NULL,
                faction          TEXT NOT NULL,
                last_known_name  TEXT NOT NULL,
                total_score      BIGINT NOT NULL DEFAULT 0,
                kills            BIGINT NOT NULL DEFAULT 0,
                ai_kills         BIGINT NOT NULL DEFAULT 0,
                deaths           BIGINT NOT NULL DEFAULT 0,
                objectives       BIGINT NOT NULL DEFAULT 0,
                playtime_seconds BIGINT NOT NULL DEFAULT 0,
                PRIMARY KEY (match_id, player_uid, faction),
                FOREIGN KEY (match_id) REFERENCES matches(id) ON DELETE CASCADE
            );
            "#,
        )
        .execute(&self.pool)
        .await?;
        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_match_players_match ON match_players(match_id);",
        )
        .execute(&self.pool)
        .await?;
        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_match_players_uid ON match_players(player_uid);",
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
            "SELECT player_uid, last_known_name, last_seen, total_score, kills, ai_kills, deaths
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
                last_seen: r.try_get("last_seen")?,
                total_score: r.try_get("total_score")?,
                kills: r.try_get("kills")?,
                ai_kills: r.try_get("ai_kills")?,
                deaths: r.try_get("deaths")?,
            });
        }
        Ok(out)
    }

    async fn aggregate_stats(&self) -> BridgeResult<AggregateStats> {
        // Postgres SUM(BIGINT) returns NUMERIC; cast back to BIGINT so we can read it as i64.
        let now = chrono::Utc::now();
        let online_since = now - chrono::Duration::minutes(2);
        let online_until = now + chrono::Duration::seconds(30);
        let row = sqlx::query(
            "SELECT
                COUNT(*)                                          AS total_players,
                COUNT(*) FILTER (WHERE last_seen >= $1 AND last_seen <= $2)
                                                                  AS total_online_players,
                COALESCE(SUM(total_score), 0)::BIGINT             AS total_score,
                COALESCE(SUM(kills), 0)::BIGINT                   AS total_kills,
                COALESCE(SUM(ai_kills), 0)::BIGINT                AS total_ai_kills,
                COALESCE(SUM(deaths), 0)::BIGINT                  AS total_deaths,
                COALESCE(SUM(objectives), 0)::BIGINT              AS total_objectives,
                COALESCE(SUM(playtime_seconds), 0)::BIGINT        AS total_playtime_seconds
             FROM players",
        )
        .bind(online_since)
        .bind(online_until)
        .fetch_one(&self.pool)
        .await?;
        Ok(AggregateStats {
            total_players: row.try_get("total_players")?,
            total_online_players: row.try_get("total_online_players")?,
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
            SELECT player_uid, last_known_name, last_seen, total_score, kills, ai_kills, deaths, rank
            FROM (
                SELECT player_uid, last_known_name, last_seen, total_score, kills, ai_kills, deaths,
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
                last_seen: r.try_get("last_seen")?,
                total_score: r.try_get("total_score")?,
                kills: r.try_get("kills")?,
                ai_kills: r.try_get("ai_kills")?,
                deaths: r.try_get("deaths")?,
            });
        }
        Ok((entries, total))
    }

    async fn finalize_match(&self, req: &FinalizeMatchRequest) -> BridgeResult<Match> {
        let now = chrono::Utc::now();
        let start_time = req.start_time.unwrap_or(now);
        let end_time = req.end_time.unwrap_or(now);

        let mut tx = self.pool.begin().await?;

        sqlx::query(
            r#"
            INSERT INTO matches (id, scenario, start_time, end_time, winning_faction, end_reason)
            VALUES ($1, $2, $3, $4, $5, $6)
            ON CONFLICT (id) DO UPDATE SET
                scenario        = EXCLUDED.scenario,
                start_time      = EXCLUDED.start_time,
                end_time        = EXCLUDED.end_time,
                winning_faction = EXCLUDED.winning_faction,
                end_reason      = EXCLUDED.end_reason
            "#,
        )
        .bind(&req.id)
        .bind(&req.scenario)
        .bind(start_time)
        .bind(end_time)
        .bind(&req.winning_faction)
        .bind(&req.end_reason)
        .execute(&mut *tx)
        .await?;

        for p in &req.players {
            if p.faction.is_empty() {
                continue;
            }
            sqlx::query(
                r#"
                INSERT INTO match_players (match_id, player_uid, faction, last_known_name,
                                           total_score, kills, ai_kills, deaths, objectives, playtime_seconds)
                VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
                ON CONFLICT (match_id, player_uid, faction) DO UPDATE SET
                    last_known_name  = EXCLUDED.last_known_name,
                    total_score      = EXCLUDED.total_score,
                    kills            = EXCLUDED.kills,
                    ai_kills         = EXCLUDED.ai_kills,
                    deaths           = EXCLUDED.deaths,
                    objectives       = EXCLUDED.objectives,
                    playtime_seconds = EXCLUDED.playtime_seconds;
                "#,
            )
            .bind(&req.id)
            .bind(&p.player_uid)
            .bind(&p.faction)
            .bind(&p.last_known_name)
            .bind(p.total_score)
            .bind(p.kills)
            .bind(p.ai_kills)
            .bind(p.deaths)
            .bind(p.objectives)
            .bind(p.playtime_seconds)
            .execute(&mut *tx)
            .await?;
        }

        let row = sqlx::query(
            "SELECT id, scenario, start_time, end_time, winning_faction, end_reason
             FROM matches WHERE id = $1",
        )
        .bind(&req.id)
        .fetch_one(&mut *tx)
        .await?;
        let m = pg_row_to_match(&row)?;
        tx.commit().await?;
        Ok(m)
    }

    async fn list_matches(
        &self,
        limit: i64,
        offset: i64,
    ) -> BridgeResult<(Vec<MatchListEntry>, i64)> {
        let rows = sqlx::query(
            r#"
            SELECT m.id, m.scenario, m.start_time, m.end_time, m.winning_faction, m.end_reason,
                   COALESCE(p.player_count, 0)::BIGINT AS player_count,
                   COALESCE(p.total_score, 0)::BIGINT  AS total_score
            FROM matches m
            LEFT JOIN (
                SELECT match_id,
                       COUNT(DISTINCT player_uid) AS player_count,
                       SUM(total_score)           AS total_score
                FROM match_players
                GROUP BY match_id
            ) p ON p.match_id = m.id
            ORDER BY COALESCE(m.end_time, m.start_time) DESC
            LIMIT $1 OFFSET $2
            "#,
        )
        .bind(limit)
        .bind(offset)
        .fetch_all(&self.pool)
        .await?;

        let total: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM matches")
            .fetch_one(&self.pool)
            .await?;

        let mut out = Vec::with_capacity(rows.len());
        for r in &rows {
            out.push(MatchListEntry {
                id: r.try_get("id")?,
                scenario: r.try_get("scenario")?,
                start_time: r.try_get("start_time")?,
                end_time: r.try_get("end_time")?,
                winning_faction: r.try_get("winning_faction")?,
                end_reason: r.try_get("end_reason")?,
                player_count: r.try_get("player_count")?,
                total_score: r.try_get("total_score")?,
            });
        }
        Ok((out, total))
    }

    async fn get_match_summary(&self, id: &str) -> BridgeResult<Option<MatchSummary>> {
        let match_row = sqlx::query(
            "SELECT id, scenario, start_time, end_time, winning_faction, end_reason
             FROM matches WHERE id = $1",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;
        let Some(match_row) = match_row else {
            return Ok(None);
        };
        let m = pg_row_to_match(&match_row)?;

        let player_rows = sqlx::query(
            "SELECT match_id, player_uid, last_known_name, faction, total_score, kills,
                    ai_kills, deaths, objectives, playtime_seconds
             FROM match_players
             WHERE match_id = $1
             ORDER BY total_score DESC, last_known_name",
        )
        .bind(id)
        .fetch_all(&self.pool)
        .await?;

        let faction_rows = sqlx::query(
            r#"
            SELECT faction,
                   COALESCE(SUM(total_score), 0)::BIGINT  AS total_score,
                   COALESCE(SUM(kills), 0)::BIGINT        AS kills,
                   COALESCE(SUM(deaths), 0)::BIGINT       AS deaths,
                   COUNT(DISTINCT player_uid)::BIGINT     AS player_count
            FROM match_players
            WHERE match_id = $1
            GROUP BY faction
            ORDER BY total_score DESC
            "#,
        )
        .bind(id)
        .fetch_all(&self.pool)
        .await?;

        let mut players = Vec::with_capacity(player_rows.len());
        for r in &player_rows {
            players.push(pg_row_to_match_player(r)?);
        }
        let mut factions = Vec::with_capacity(faction_rows.len());
        for r in &faction_rows {
            factions.push(MatchFactionScore {
                faction: r.try_get("faction")?,
                total_score: r.try_get("total_score")?,
                kills: r.try_get("kills")?,
                deaths: r.try_get("deaths")?,
                player_count: r.try_get("player_count")?,
            });
        }

        Ok(Some(MatchSummary {
            match_meta: m,
            factions,
            players,
        }))
    }
}
