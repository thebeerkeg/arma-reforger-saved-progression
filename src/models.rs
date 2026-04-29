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
