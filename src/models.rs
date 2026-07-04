use serde::{Deserialize, Serialize};
use sqlx::FromRow;

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct Team {
    pub id: i64,
    pub user_id: i64,
    pub name: String,
    pub head_coach: String,
    pub assistant_coaches: String,
    pub logo_path: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct Player {
    pub id: i64,
    pub team_id: i64,
    pub number: String,
    pub name: String,
    pub default_position: String,
    pub sort_order: i64,
}

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct Lineup {
    pub id: i64,
    pub team_id: i64,
    pub name: String,
    pub opponent: String,
    pub game_date: String,
    pub location: String,
    pub home_away: String,
    pub use_dh: i64,
    pub use_eh: i64,
    pub dh_mode: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct LineupSpot {
    pub id: i64,
    pub lineup_id: i64,
    pub batting_order: i64,
    pub slot_kind: String,
    pub player_id: Option<i64>,
    pub position: String,
    pub is_dh: i64,
}

// ---- Request payloads ----

#[derive(Debug, Deserialize)]
pub struct AuthPayload {
    pub email: String,
    pub password: String,
}

#[derive(Debug, Deserialize)]
pub struct TeamPayload {
    pub name: String,
    #[serde(default)]
    pub head_coach: String,
    #[serde(default)]
    pub assistant_coaches: String,
}

#[derive(Debug, Deserialize)]
pub struct PlayerPayload {
    #[serde(default)]
    pub number: String,
    pub name: String,
    #[serde(default)]
    pub default_position: String,
}

#[derive(Debug, Deserialize)]
pub struct SpotPayload {
    pub batting_order: i64,
    #[serde(default = "default_slot_kind")]
    pub slot_kind: String,
    pub player_id: Option<i64>,
    #[serde(default)]
    pub position: String,
    #[serde(default)]
    pub is_dh: bool,
}

fn default_slot_kind() -> String {
    "BAT".to_string()
}

#[derive(Debug, Deserialize)]
pub struct LineupPayload {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub opponent: String,
    #[serde(default)]
    pub game_date: String,
    #[serde(default)]
    pub location: String,
    #[serde(default)]
    pub home_away: String,
    #[serde(default)]
    pub use_dh: bool,
    #[serde(default)]
    pub use_eh: bool,
    #[serde(default = "default_dh_mode")]
    pub dh_mode: String,
    #[serde(default)]
    pub spots: Vec<SpotPayload>,
}

fn default_dh_mode() -> String {
    "straight9".to_string()
}

// ---- Composite responses ----

#[derive(Debug, Serialize)]
pub struct LineupDetail {
    #[serde(flatten)]
    pub lineup: Lineup,
    pub spots: Vec<LineupSpot>,
}
