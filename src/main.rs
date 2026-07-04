mod auth;
mod error;
mod handlers;
mod models;
mod pdf;

use axum::{
    routing::{get, post, put},
    Router,
};
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::SqlitePool;
use std::str::FromStr;
use tower_http::services::ServeDir;
use tower_http::trace::TraceLayer;

#[derive(Clone)]
pub struct AppState {
    pub pool: SqlitePool,
    /// Filesystem directory where uploaded team logos are stored. On a hosted
    /// deployment this points at a persistent volume (e.g. /data/uploads).
    pub uploads_dir: String,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,tower_http=info".into()),
        )
        .init();

    let uploads_dir = std::env::var("UPLOADS_DIR").unwrap_or_else(|_| "uploads".to_string());
    std::fs::create_dir_all(&uploads_dir).ok();

    let static_dir = std::env::var("STATIC_DIR").unwrap_or_else(|_| "static".to_string());

    let db_url = std::env::var("DATABASE_URL").unwrap_or_else(|_| "lineup.db".to_string());
    // Accept either a sqlite:// URI or a plain filesystem path.
    let opts = if db_url.contains("://") {
        SqliteConnectOptions::from_str(&db_url)?
    } else {
        SqliteConnectOptions::new().filename(&db_url)
    }
    .create_if_missing(true)
    .foreign_keys(true);
    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .connect_with(opts)
        .await?;

    // Apply schema.
    let schema = include_str!("../migrations/0001_init.sql");
    sqlx::raw_sql(schema).execute(&pool).await?;

    // Additive migrations for databases created before a column existed.
    // (CREATE TABLE IF NOT EXISTS won't add new columns to an existing table.)
    let has_is_dh: Option<(i64,)> =
        sqlx::query_as("SELECT 1 FROM pragma_table_info('lineup_spots') WHERE name = 'is_dh'")
            .fetch_optional(&pool)
            .await?;
    if has_is_dh.is_none() {
        sqlx::query("ALTER TABLE lineup_spots ADD COLUMN is_dh INTEGER NOT NULL DEFAULT 0")
            .execute(&pool)
            .await?;
    }
    let has_dh_mode: Option<(i64,)> =
        sqlx::query_as("SELECT 1 FROM pragma_table_info('lineups') WHERE name = 'dh_mode'")
            .fetch_optional(&pool)
            .await?;
    if has_dh_mode.is_none() {
        sqlx::query("ALTER TABLE lineups ADD COLUMN dh_mode TEXT NOT NULL DEFAULT 'straight9'")
            .execute(&pool)
            .await?;
    }

    let state = AppState {
        pool,
        uploads_dir: uploads_dir.clone(),
    };

    let api = Router::new()
        .route("/signup", post(auth::signup))
        .route("/login", post(auth::login))
        .route("/logout", post(auth::logout))
        .route("/me", get(auth::me))
        .route("/teams", get(handlers::list_teams).post(handlers::create_team))
        .route(
            "/teams/{id}",
            get(handlers::get_team)
                .put(handlers::update_team)
                .delete(handlers::delete_team),
        )
        .route("/teams/{id}/logo", post(handlers::upload_logo))
        .route(
            "/teams/{id}/players",
            get(handlers::list_players).post(handlers::create_player),
        )
        .route(
            "/players/{id}",
            put(handlers::update_player).delete(handlers::delete_player),
        )
        .route(
            "/teams/{id}/lineups",
            get(handlers::list_lineups).post(handlers::create_lineup),
        )
        .route(
            "/lineups/{id}",
            get(handlers::get_lineup)
                .put(handlers::update_lineup)
                .delete(handlers::delete_lineup),
        )
        .route("/lineups/{id}/pdf", get(handlers::lineup_pdf));

    let app = Router::new()
        .nest("/api", api)
        .nest_service("/uploads", ServeDir::new(&uploads_dir))
        .fallback_service(ServeDir::new(&static_dir).append_index_html_on_directories(true))
        .layer(TraceLayer::new_for_http())
        .with_state(state);

    let addr = std::env::var("BIND_ADDR").unwrap_or_else(|_| "0.0.0.0:3000".to_string());
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    tracing::info!("listening on http://{addr}");
    axum::serve(listener, app).await?;
    Ok(())
}
