use axum::extract::{Multipart, Path, Query, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use rand::RngCore;
use serde::Deserialize;

use crate::auth::AuthUser;
use crate::error::{AppError, AppResult};
use crate::models::*;
use crate::pdf;
use crate::AppState;

// ---- ownership helpers ----

/// Map a stored logo URL ("/uploads/<file>") to its path on disk under the
/// configured uploads directory.
fn logo_fs_path(uploads_dir: &str, logo_url: &str) -> String {
    let file = logo_url.rsplit('/').next().unwrap_or(logo_url);
    format!("{uploads_dir}/{file}")
}

async fn owned_team(state: &AppState, user_id: i64, team_id: i64) -> AppResult<Team> {
    sqlx::query_as::<_, Team>("SELECT * FROM teams WHERE id = ? AND user_id = ?")
        .bind(team_id)
        .bind(user_id)
        .fetch_optional(&state.pool)
        .await?
        .ok_or_else(|| AppError::not_found("team not found"))
}

async fn owned_lineup(state: &AppState, user_id: i64, lineup_id: i64) -> AppResult<Lineup> {
    sqlx::query_as::<_, Lineup>(
        "SELECT lineups.* FROM lineups \
         JOIN teams ON teams.id = lineups.team_id \
         WHERE lineups.id = ? AND teams.user_id = ?",
    )
    .bind(lineup_id)
    .bind(user_id)
    .fetch_optional(&state.pool)
    .await?
    .ok_or_else(|| AppError::not_found("lineup not found"))
}

async fn owned_player(state: &AppState, user_id: i64, player_id: i64) -> AppResult<Player> {
    sqlx::query_as::<_, Player>(
        "SELECT players.* FROM players \
         JOIN teams ON teams.id = players.team_id \
         WHERE players.id = ? AND teams.user_id = ?",
    )
    .bind(player_id)
    .bind(user_id)
    .fetch_optional(&state.pool)
    .await?
    .ok_or_else(|| AppError::not_found("player not found"))
}

// ---- teams ----

pub async fn list_teams(
    user: AuthUser,
    State(state): State<AppState>,
) -> AppResult<Json<Vec<Team>>> {
    let teams = sqlx::query_as::<_, Team>(
        "SELECT * FROM teams WHERE user_id = ? ORDER BY created_at DESC, id DESC",
    )
    .bind(user.id)
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(teams))
}

pub async fn get_team(
    user: AuthUser,
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> AppResult<Json<Team>> {
    Ok(Json(owned_team(&state, user.id, id).await?))
}

pub async fn create_team(
    user: AuthUser,
    State(state): State<AppState>,
    Json(payload): Json<TeamPayload>,
) -> AppResult<Response> {
    if payload.name.trim().is_empty() {
        return Err(AppError::bad_request("team name is required"));
    }
    let rec: (i64,) = sqlx::query_as(
        "INSERT INTO teams (user_id, name, head_coach, assistant_coaches) \
         VALUES (?, ?, ?, ?) RETURNING id",
    )
    .bind(user.id)
    .bind(payload.name.trim())
    .bind(&payload.head_coach)
    .bind(&payload.assistant_coaches)
    .fetch_one(&state.pool)
    .await?;
    let team = owned_team(&state, user.id, rec.0).await?;
    Ok((StatusCode::CREATED, Json(team)).into_response())
}

pub async fn update_team(
    user: AuthUser,
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(payload): Json<TeamPayload>,
) -> AppResult<Json<Team>> {
    owned_team(&state, user.id, id).await?;
    if payload.name.trim().is_empty() {
        return Err(AppError::bad_request("team name is required"));
    }
    sqlx::query(
        "UPDATE teams SET name = ?, head_coach = ?, assistant_coaches = ? WHERE id = ?",
    )
    .bind(payload.name.trim())
    .bind(&payload.head_coach)
    .bind(&payload.assistant_coaches)
    .bind(id)
    .execute(&state.pool)
    .await?;
    Ok(Json(owned_team(&state, user.id, id).await?))
}

pub async fn delete_team(
    user: AuthUser,
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> AppResult<Json<serde_json::Value>> {
    let team = owned_team(&state, user.id, id).await?;
    if let Some(path) = &team.logo_path {
        std::fs::remove_file(logo_fs_path(&state.uploads_dir, path)).ok();
    }
    sqlx::query("DELETE FROM teams WHERE id = ?")
        .bind(id)
        .execute(&state.pool)
        .await?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

pub async fn upload_logo(
    user: AuthUser,
    State(state): State<AppState>,
    Path(id): Path<i64>,
    mut multipart: Multipart,
) -> AppResult<Json<Team>> {
    let team = owned_team(&state, user.id, id).await?;

    let field = multipart
        .next_field()
        .await
        .map_err(|e| AppError::bad_request(e.to_string()))?
        .ok_or_else(|| AppError::bad_request("no file provided"))?;

    let content_type = field.content_type().unwrap_or("").to_string();
    let ext = match content_type.as_str() {
        "image/png" => "png",
        "image/jpeg" | "image/jpg" => "jpg",
        _ => return Err(AppError::bad_request("logo must be a PNG or JPEG image")),
    };

    let data = field
        .bytes()
        .await
        .map_err(|e| AppError::bad_request(e.to_string()))?;
    if data.len() > 5 * 1024 * 1024 {
        return Err(AppError::bad_request("logo must be under 5 MB"));
    }

    let mut r = [0u8; 8];
    rand::rngs::OsRng.fill_bytes(&mut r);
    let suffix: String = r.iter().map(|b| format!("{b:02x}")).collect();
    let filename = format!("team_{id}_{suffix}.{ext}");
    std::fs::create_dir_all(&state.uploads_dir).ok();
    std::fs::write(format!("{}/{filename}", state.uploads_dir), &data)
        .map_err(|e| AppError::new(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    // Remove previous logo file.
    if let Some(old) = &team.logo_path {
        std::fs::remove_file(logo_fs_path(&state.uploads_dir, old)).ok();
    }

    let logo_path = format!("/uploads/{filename}");
    sqlx::query("UPDATE teams SET logo_path = ? WHERE id = ?")
        .bind(&logo_path)
        .bind(id)
        .execute(&state.pool)
        .await?;

    Ok(Json(owned_team(&state, user.id, id).await?))
}

// ---- players ----

pub async fn list_players(
    user: AuthUser,
    State(state): State<AppState>,
    Path(team_id): Path<i64>,
) -> AppResult<Json<Vec<Player>>> {
    owned_team(&state, user.id, team_id).await?;
    let players = sqlx::query_as::<_, Player>(
        "SELECT * FROM players WHERE team_id = ? ORDER BY sort_order, id",
    )
    .bind(team_id)
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(players))
}

pub async fn create_player(
    user: AuthUser,
    State(state): State<AppState>,
    Path(team_id): Path<i64>,
    Json(payload): Json<PlayerPayload>,
) -> AppResult<Response> {
    owned_team(&state, user.id, team_id).await?;
    if payload.name.trim().is_empty() {
        return Err(AppError::bad_request("player name is required"));
    }
    let next: (i64,) =
        sqlx::query_as("SELECT COALESCE(MAX(sort_order), 0) + 1 FROM players WHERE team_id = ?")
            .bind(team_id)
            .fetch_one(&state.pool)
            .await?;
    let rec: (i64,) = sqlx::query_as(
        "INSERT INTO players (team_id, number, name, default_position, sort_order) \
         VALUES (?, ?, ?, ?, ?) RETURNING id",
    )
    .bind(team_id)
    .bind(payload.number.trim())
    .bind(payload.name.trim())
    .bind(payload.default_position.trim())
    .bind(next.0)
    .fetch_one(&state.pool)
    .await?;
    let player = owned_player(&state, user.id, rec.0).await?;
    Ok((StatusCode::CREATED, Json(player)).into_response())
}

pub async fn update_player(
    user: AuthUser,
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(payload): Json<PlayerPayload>,
) -> AppResult<Json<Player>> {
    owned_player(&state, user.id, id).await?;
    if payload.name.trim().is_empty() {
        return Err(AppError::bad_request("player name is required"));
    }
    sqlx::query("UPDATE players SET number = ?, name = ?, default_position = ? WHERE id = ?")
        .bind(payload.number.trim())
        .bind(payload.name.trim())
        .bind(payload.default_position.trim())
        .bind(id)
        .execute(&state.pool)
        .await?;
    Ok(Json(owned_player(&state, user.id, id).await?))
}

pub async fn delete_player(
    user: AuthUser,
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> AppResult<Json<serde_json::Value>> {
    owned_player(&state, user.id, id).await?;
    sqlx::query("DELETE FROM players WHERE id = ?")
        .bind(id)
        .execute(&state.pool)
        .await?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

// ---- lineups ----

async fn load_spots(state: &AppState, lineup_id: i64) -> AppResult<Vec<LineupSpot>> {
    let spots = sqlx::query_as::<_, LineupSpot>(
        "SELECT * FROM lineup_spots WHERE lineup_id = ? ORDER BY batting_order, id",
    )
    .bind(lineup_id)
    .fetch_all(&state.pool)
    .await?;
    Ok(spots)
}

async fn replace_spots(
    state: &AppState,
    lineup_id: i64,
    spots: &[SpotPayload],
) -> AppResult<()> {
    sqlx::query("DELETE FROM lineup_spots WHERE lineup_id = ?")
        .bind(lineup_id)
        .execute(&state.pool)
        .await?;
    for spot in spots {
        sqlx::query(
            "INSERT INTO lineup_spots (lineup_id, batting_order, slot_kind, player_id, position, is_dh) \
             VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind(lineup_id)
        .bind(spot.batting_order)
        .bind(&spot.slot_kind)
        .bind(spot.player_id)
        .bind(&spot.position)
        .bind(spot.is_dh as i64)
        .execute(&state.pool)
        .await?;
    }
    Ok(())
}

pub async fn list_lineups(
    user: AuthUser,
    State(state): State<AppState>,
    Path(team_id): Path<i64>,
) -> AppResult<Json<Vec<Lineup>>> {
    owned_team(&state, user.id, team_id).await?;
    let lineups = sqlx::query_as::<_, Lineup>(
        "SELECT * FROM lineups WHERE team_id = ? ORDER BY created_at DESC, id DESC",
    )
    .bind(team_id)
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(lineups))
}

pub async fn create_lineup(
    user: AuthUser,
    State(state): State<AppState>,
    Path(team_id): Path<i64>,
    Json(payload): Json<LineupPayload>,
) -> AppResult<Response> {
    owned_team(&state, user.id, team_id).await?;
    let rec: (i64,) = sqlx::query_as(
        "INSERT INTO lineups (team_id, name, opponent, game_date, location, home_away, use_dh, use_eh, dh_mode) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?) RETURNING id",
    )
    .bind(team_id)
    .bind(&payload.name)
    .bind(&payload.opponent)
    .bind(&payload.game_date)
    .bind(&payload.location)
    .bind(&payload.home_away)
    .bind(payload.use_dh as i64)
    .bind(payload.use_eh as i64)
    .bind(&payload.dh_mode)
    .fetch_one(&state.pool)
    .await?;
    replace_spots(&state, rec.0, &payload.spots).await?;
    let detail = lineup_detail(&state, rec.0).await?;
    Ok((StatusCode::CREATED, Json(detail)).into_response())
}

async fn lineup_detail(state: &AppState, lineup_id: i64) -> AppResult<LineupDetail> {
    let lineup = sqlx::query_as::<_, Lineup>("SELECT * FROM lineups WHERE id = ?")
        .bind(lineup_id)
        .fetch_one(&state.pool)
        .await?;
    let spots = load_spots(state, lineup_id).await?;
    Ok(LineupDetail { lineup, spots })
}

pub async fn get_lineup(
    user: AuthUser,
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> AppResult<Json<LineupDetail>> {
    owned_lineup(&state, user.id, id).await?;
    Ok(Json(lineup_detail(&state, id).await?))
}

pub async fn update_lineup(
    user: AuthUser,
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(payload): Json<LineupPayload>,
) -> AppResult<Json<LineupDetail>> {
    owned_lineup(&state, user.id, id).await?;
    sqlx::query(
        "UPDATE lineups SET name = ?, opponent = ?, game_date = ?, location = ?, \
         home_away = ?, use_dh = ?, use_eh = ?, dh_mode = ? WHERE id = ?",
    )
    .bind(&payload.name)
    .bind(&payload.opponent)
    .bind(&payload.game_date)
    .bind(&payload.location)
    .bind(&payload.home_away)
    .bind(payload.use_dh as i64)
    .bind(payload.use_eh as i64)
    .bind(&payload.dh_mode)
    .bind(id)
    .execute(&state.pool)
    .await?;
    replace_spots(&state, id, &payload.spots).await?;
    Ok(Json(lineup_detail(&state, id).await?))
}

pub async fn delete_lineup(
    user: AuthUser,
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> AppResult<Json<serde_json::Value>> {
    owned_lineup(&state, user.id, id).await?;
    sqlx::query("DELETE FROM lineups WHERE id = ?")
        .bind(id)
        .execute(&state.pool)
        .await?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

// ---- PDF ----

#[derive(Debug, Deserialize)]
pub struct PdfQuery {
    #[serde(default = "one")]
    pub coach: u32,
    #[serde(default = "one")]
    pub scorekeeper: u32,
    #[serde(default = "one", rename = "self")]
    pub self_copy: u32,
    #[serde(default = "one")]
    pub umpire: u32,
}

fn one() -> u32 {
    1
}

pub async fn lineup_pdf(
    user: AuthUser,
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Query(q): Query<PdfQuery>,
) -> AppResult<Response> {
    let lineup = owned_lineup(&state, user.id, id).await?;
    let team = owned_team(&state, user.id, lineup.team_id).await?;
    let spots = load_spots(&state, id).await?;
    let players = sqlx::query_as::<_, Player>("SELECT * FROM players WHERE team_id = ?")
        .bind(team.id)
        .fetch_all(&state.pool)
        .await?;

    let request = pdf::PdfRequest {
        coach: q.coach.min(20),
        scorekeeper: q.scorekeeper.min(20),
        self_copy: q.self_copy.min(20),
        umpire: q.umpire.min(20),
    };

    let bytes = pdf::render_sheet(&team, &lineup, &spots, &players, &request, &state.uploads_dir)?;

    let filename = format!(
        "lineup-{}.pdf",
        if lineup.opponent.is_empty() {
            "card".to_string()
        } else {
            lineup.opponent.replace(' ', "_")
        }
    );

    Ok((
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, "application/pdf".to_string()),
            (
                header::CONTENT_DISPOSITION,
                format!("inline; filename=\"{filename}\""),
            ),
        ],
        bytes,
    )
        .into_response())
}
