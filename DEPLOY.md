# Deploying to Fly.io

The app is a single Rust binary that keeps its state — the SQLite database and
uploaded team logos — on disk. On Fly that disk is a **persistent volume**
mounted at `/data`. The included `Dockerfile` and `fly.toml` are already wired
for this: `DATABASE_URL=/data/lineup.db` and `UPLOADS_DIR=/data/uploads`.

## One-time setup

1. **Install flyctl**

   ```bash
   curl -L https://fly.io/install.sh | sh      # macOS/Linux
   # or: brew install flyctl
   ```

2. **Create / sign in to a Fly account** (a payment method is required even for
   small usage, but a single auto-stopping machine like this is very cheap):

   ```bash
   fly auth signup     # or: fly auth login
   ```

## Launch the app

From the repository root:

1. **Create the app** without deploying yet. This reads `fly.toml`, detects the
   `Dockerfile`, and lets you pick a unique app name and a nearby region:

   ```bash
   fly launch --no-deploy --copy-config
   ```

   When prompted, keep the existing settings. Note the **region** you choose
   (e.g. `iad`, `lax`, `ord`) — the volume must be created in the same one.

2. **Create the persistent volume** (1 GB is plenty; Fly includes some free
   volume storage). Use the same region as the app:

   ```bash
   fly volumes create lineup_data --region <your-region> --size 1
   ```

3. **Deploy:**

   ```bash
   fly deploy
   ```

4. **Keep it to a single machine** (SQLite + the volume live on one machine, so
   don't scale horizontally):

   ```bash
   fly scale count 1
   ```

5. **Open it:**

   ```bash
   fly open
   ```

Sign up inside the app, add your team and roster, and you're live. Data
persists across deploys and restarts because it lives on the `lineup_data`
volume.

## Cost / scale-to-zero

`fly.toml` sets `auto_stop_machines = "stop"` and `min_machines_running = 0`, so
the machine sleeps when no one is using it and cold-starts (a few seconds) on
the next request. That keeps a personal-use app at or near $0.

## Updating

Push code, then redeploy:

```bash
fly deploy
```

Schema changes are applied automatically on boot (the app runs its migrations,
including additive `ALTER TABLE`s, at startup).

## Backups

The volume is not automatically backed up. To grab a copy of the database:

```bash
fly ssh console -C "cat /data/lineup.db" > backup-$(date +%F).db
```

Fly also takes periodic volume snapshots you can restore from
(`fly volumes snapshots list <volume-id>`).

## Local run (no Docker)

```bash
cargo run
# open http://localhost:3000
```

Environment variables (all optional): `BIND_ADDR`, `DATABASE_URL` (a sqlite URI
or a plain file path), `UPLOADS_DIR`, `RUST_LOG`.
