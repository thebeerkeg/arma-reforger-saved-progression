use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct PlayerRecord {
    pub player_uid: String,
    pub last_known_name: String,
    pub total_score: i64,
    pub kills: i64,
    pub ai_kills: i64,
    pub deaths: i64,
    pub objectives: i64,
    pub playtime_seconds: i64,
    pub first_seen: DateTime<Utc>,
    pub last_seen: DateTime<Utc>,
}

/// A delta update to a player's stats. All fields are increments and may be negative.
#[derive(Debug, Deserialize, Clone, Default)]
pub struct StatDelta {
    #[serde(default)]
    pub total_score: i64,
    #[serde(default)]
    pub kills: i64,
    #[serde(default)]
    pub ai_kills: i64,
    #[serde(default)]
    pub deaths: i64,
    #[serde(default)]
    pub objectives: i64,
    #[serde(default)]
    pub playtime_seconds: i64,
}

#[derive(Debug, Deserialize)]
pub struct IncrementRequest {
    pub last_known_name: String,
    /// When present (with `faction`), the same delta is also applied to the
    /// per-(match, player, faction) row in match_players.
    #[serde(default)]
    pub match_id: Option<String>,
    #[serde(default)]
    pub faction: Option<String>,
    #[serde(flatten)]
    pub delta: StatDelta,
}

#[derive(Debug, Deserialize)]
pub struct BatchIncrementEntry {
    pub player_uid: String,
    pub last_known_name: String,
    /// Same semantics as IncrementRequest.match_id — both fields must be set
    /// for the per-match upsert to fire. Missing/empty is fine; the lifetime
    /// row is always updated.
    #[serde(default)]
    pub match_id: Option<String>,
    #[serde(default)]
    pub faction: Option<String>,
    #[serde(flatten)]
    pub delta: StatDelta,
}

#[derive(Debug, Deserialize)]
pub struct BatchIncrementRequest {
    pub entries: Vec<BatchIncrementEntry>,
}

#[derive(Debug, Serialize)]
pub struct LeaderboardEntry {
    pub rank: i64,
    pub player_uid: String,
    pub last_known_name: String,
    pub total_score: i64,
    pub kills: i64,
    pub deaths: i64,
}

#[derive(Debug, Serialize)]
pub struct AggregateStats {
    pub total_players: i64,
    pub total_score: i64,
    pub total_kills: i64,
    pub total_ai_kills: i64,
    pub total_deaths: i64,
    pub total_objectives: i64,
    pub total_playtime_seconds: i64,
}

// -----------------------------------------------------------------------------
// Match tracking
// -----------------------------------------------------------------------------

/// One round/session as registered by the addon at game-mode start. The
/// addon picks the id (so it can reference it from increments before the
/// register POST has even completed).
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Match {
    pub id: String,
    pub scenario: String,
    pub start_time: DateTime<Utc>,
    pub end_time: Option<DateTime<Utc>>,
    pub winning_faction: Option<String>,
    pub end_reason: Option<String>,
}

/// Per-(match, player, faction) row. The composite key on faction means a
/// player who switched sides mid-match shows up as two rows, each carrying the
/// stats earned under that faction.
#[derive(Debug, Serialize, Clone)]
pub struct MatchPlayer {
    pub match_id: String,
    pub player_uid: String,
    pub last_known_name: String,
    pub faction: String,
    pub total_score: i64,
    pub kills: i64,
    pub ai_kills: i64,
    pub deaths: i64,
    pub objectives: i64,
    pub playtime_seconds: i64,
}

#[derive(Debug, Serialize, Clone)]
pub struct MatchFactionScore {
    pub faction: String,
    pub total_score: i64,
    pub kills: i64,
    pub deaths: i64,
    pub player_count: i64,
}

#[derive(Debug, Serialize)]
pub struct MatchSummary {
    #[serde(flatten)]
    pub match_meta: Match,
    pub factions: Vec<MatchFactionScore>,
    pub players: Vec<MatchPlayer>,
}

#[derive(Debug, Serialize, Clone)]
pub struct MatchListEntry {
    pub id: String,
    pub scenario: String,
    pub start_time: DateTime<Utc>,
    pub end_time: Option<DateTime<Utc>>,
    pub winning_faction: Option<String>,
    pub end_reason: Option<String>,
    pub player_count: i64,
    pub total_score: i64,
}

#[derive(Debug, Deserialize)]
pub struct RegisterMatchRequest {
    pub id: String,
    pub scenario: String,
    /// Optional — defaults to server's current UTC time. Useful when the
    /// addon wants to attribute a match to the wall-clock moment it started
    /// rather than the moment the bridge received the POST.
    #[serde(default)]
    pub start_time: Option<DateTime<Utc>>,
}

#[derive(Debug, Deserialize)]
pub struct EndMatchRequest {
    /// Free-form faction key (e.g. "US", "USSR", "FIA") or null on draw / Campaign.
    #[serde(default)]
    pub winning_faction: Option<String>,
    /// Free-form reason: "victory", "draw", "abandoned", "timeout", etc.
    #[serde(default)]
    pub end_reason: Option<String>,
    /// Optional — defaults to server's current UTC time.
    #[serde(default)]
    pub end_time: Option<DateTime<Utc>>,
}
