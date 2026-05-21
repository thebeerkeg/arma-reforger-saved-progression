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
    #[serde(flatten)]
    pub delta: StatDelta,
}

#[derive(Debug, Deserialize)]
pub struct BatchIncrementEntry {
    pub player_uid: String,
    pub last_known_name: String,
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
    pub last_seen: DateTime<Utc>,
    pub total_score: i64,
    pub kills: i64,
    pub ai_kills: i64,
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
//
// Matches are written to the bridge ONLY at clean game-mode end. The addon
// accumulates per-(player, faction) totals locally during the match and posts
// the whole match in one atomic /match/finalize call. Server crashes / abrupt
// shutdowns therefore leave nothing in the matches/match_players tables —
// abandoned matches are intentionally invisible.

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

/// One row in the roster posted alongside a match's finalize.
#[derive(Debug, Deserialize, Clone)]
pub struct FinalizeMatchPlayer {
    pub player_uid: String,
    pub last_known_name: String,
    pub faction: String,
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

/// Atomic match save — POST /match/finalize. Inserts the matches row plus
/// every roster entry in one transaction; failure leaves both tables
/// untouched.
#[derive(Debug, Deserialize)]
pub struct FinalizeMatchRequest {
    pub id: String,
    pub scenario: String,
    /// Falls back to (now() - duration_unknown) on the bridge when absent.
    /// Addon should send the wall-clock match start.
    #[serde(default)]
    pub start_time: Option<DateTime<Utc>>,
    /// Falls back to server's now() when absent.
    #[serde(default)]
    pub end_time: Option<DateTime<Utc>>,
    #[serde(default)]
    pub winning_faction: Option<String>,
    #[serde(default)]
    pub end_reason: Option<String>,
    #[serde(default)]
    pub players: Vec<FinalizeMatchPlayer>,
}
