# ⚾ Lineup Card Maker

A web app for building, saving, and printing **professional baseball lineup
cards**. Manage your roster, drag players into the batting order, assign
fielding positions, then export a print-ready PDF with cards sized for the
opposing coach, scorekeeper, yourself, and the umpire — all on one sheet with
cut lines.

## Features

- **Accounts** — email + password sign-in; your teams and lineups are saved to
  your account.
- **Teams** — name, head coach, assistant coaches, and a team logo (PNG/JPEG).
  Multiple teams per account.
- **Roster** — add players with number, name, and default fielding position.
- **Lineup builder** — 9 batting spots plus optional **DH** and **EH**. Drag a
  player from the pool into a spot and pick a position; placed players are
  struck through in the pool. Live card preview.
- **Save / load** — store multiple lineups per team and reopen them later.
- **Print / PDF** — server-generated PDF on US-Letter sheets with cut borders:
  - **Large cards** (3.75 × 5.5 in) for the home coach, scorekeeper, and you —
    more room to write.
  - **Umpire card** (3.5 × 5 in) sized to fit a standard umpire lineup-card
    holder.
  - Choose how many of each recipient to print; cards flow across pages.

## Tech stack

- **Backend:** Rust + [Axum](https://github.com/tokio-rs/axum)
- **Database:** SQLite via [`sqlx`](https://github.com/launchbadge/sqlx)
  (schema in `migrations/0001_init.sql`, applied on startup)
- **Auth:** Argon2 password hashing + HTTP-only cookie sessions
- **PDF:** server-side with [`printpdf`](https://github.com/fschutt/printpdf)
- **Frontend:** static HTML/CSS + vanilla JS with native HTML5 drag-and-drop
  (no build step) — served directly by Axum from `static/`

## Running locally

```bash
cargo run
# then open http://localhost:3000
```

Environment variables (all optional):

| Variable       | Default          | Purpose                                   |
| -------------- | ---------------- | ----------------------------------------- |
| `BIND_ADDR`    | `0.0.0.0:3000`   | Address/port to bind                      |
| `DATABASE_URL` | `lineup.db`      | SQLite URI or plain file path             |
| `UPLOADS_DIR`  | `uploads`        | Directory for uploaded team logos         |
| `STATIC_DIR`   | `static`         | Directory for the static front end        |
| `RUST_LOG`     | `info`           | Log filter                                |

Uploaded logos are stored under `UPLOADS_DIR` and served at `/uploads/...`.

## Deploying

See [DEPLOY.md](DEPLOY.md) for a step-by-step Fly.io deployment (Docker image +
persistent volume for the database and logos). A `Dockerfile` and `fly.toml`
are included.

## Tests

```bash
cargo test
```

Covers PDF byte-output sanity (valid `%PDF` header) and the card page-packing
layout.

## Project layout

```
src/
  main.rs       app bootstrap, router, static serving, DB init
  auth.rs       signup/login/logout, sessions, AuthUser extractor
  handlers.rs   teams, players, lineups CRUD + logo upload + PDF endpoint
  pdf.rs        server-side lineup-card PDF rendering
  models.rs     data + request/response types
  error.rs      unified API error type
migrations/     SQLite schema
static/         index.html, app.js, styles.css
```
