//! `ferroshop` binary: HTTP server plus maintenance commands.

mod demo;

use std::net::SocketAddr;

use anyhow::Context as _;
use axum::Router;
use clap::{Parser, Subcommand};
use ferroshop_admin_api::AppState;
use ferroshop_core::media::MediaStore;
use ferroshop_core::write::{self, WriteMode};
use ferroshop_core::{Context, Db};
use serde_json::json;
use sqlx::postgres::PgPoolOptions;
use tower_http::cors::CorsLayer;
use tower_http::services::{ServeDir, ServeFile};
use tower_http::trace::TraceLayer;

#[derive(Parser)]
#[command(name = "ferroshop", version, about = "ferroshop – e-commerce in Rust")]
struct Cli {
    #[arg(long, env = "DATABASE_URL")]
    database_url: String,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Runs the HTTP server (Admin-API, Store-API and Administration UI).
    Serve {
        #[arg(long, env = "LISTEN", default_value = "0.0.0.0:8000")]
        listen: SocketAddr,
        #[arg(long, env = "APP_SECRET")]
        secret: String,
        /// Directory with the built Administration (`admin/dist`).
        #[arg(long, env = "ADMIN_DIR", default_value = "../admin/dist")]
        admin_dir: String,
        /// Apply pending migrations before starting.
        #[arg(long, env = "AUTO_MIGRATE")]
        migrate: bool,
        /// Where media files live: a local directory or `s3://bucket/prefix`.
        #[arg(long, env = "MEDIA_STORAGE", default_value = "./var/media")]
        media_storage: String,
    },
    /// Applies pending database migrations.
    Migrate,
    /// Creates an administrator.
    #[command(name = "user:create")]
    UserCreate {
        username: String,
        #[arg(long)]
        email: String,
        #[arg(long, env = "FERROSHOP_PASSWORD")]
        password: String,
        #[arg(long, default_value = "")]
        first_name: String,
        #[arg(long, default_value = "")]
        last_name: String,
    },
    /// Creates an API integration and prints its credentials once.
    #[command(name = "integration:create")]
    IntegrationCreate {
        label: String,
        /// Grant full access instead of no privileges (assign roles in the Administration).
        #[arg(long)]
        admin: bool,
    },
    /// Fills the shop with demo manufacturers, categories, products, customers and orders.
    #[command(name = "demo-data")]
    DemoData,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let _ = dotenv_file();
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .init();
    let cli = Cli::parse();
    let db: Db = PgPoolOptions::new()
        .max_connections(20)
        .connect(&cli.database_url)
        .await
        .context("connecting to the database")?;

    match cli.command {
        Command::Migrate => {
            ferroshop_core::MIGRATOR.run(&db).await?;
            println!("migrations applied");
        }
        Command::Serve { listen, secret, admin_dir, migrate, media_storage } => {
            if migrate {
                ferroshop_core::MIGRATOR.run(&db).await?;
            }
            anyhow::ensure!(secret.len() >= 32, "APP_SECRET must be at least 32 characters");
            let admin = ServeDir::new(&admin_dir).fallback(ServeFile::new(format!("{admin_dir}/index.html")));
            let media = MediaStore::from_config(&media_storage).map_err(|e| anyhow::anyhow!(e))?;
            let store = ferroshop_store_api::StoreState::new(db.clone());
            let state = AppState::new(db, &secret, media);
            let app = Router::new()
                .merge(ferroshop_admin_api::router(state.clone()))
                .merge(ferroshop_store_api::router(store))
                .merge(ferroshop_admin_api::media_router(state))
                .nest_service("/admin", admin)
                .layer(CorsLayer::permissive())
                .layer(TraceLayer::new_for_http());
            let listener = tokio::net::TcpListener::bind(listen).await?;
            tracing::info!("ferroshop listening on http://{listen} (admin: /admin, store-api: /store-api)");
            axum::serve(listener, app).await?;
        }
        Command::UserCreate { username, email, password, first_name, last_name } => {
            let payload = json!({
                "username": username, "email": email, "password": password,
                "firstName": first_name, "lastName": last_name, "admin": true,
                "aclRoles": [{ "id": "0191a6f0f4a07c7e9b1c2d3e4f5a6d01" }],
            });
            let id =
                write::write(&db, "user", &payload, WriteMode::Create, &Context::default()).await.map_err(describe)?;
            println!("created admin user {username} ({})", ferroshop_core::id::format(id));
        }
        Command::IntegrationCreate { label, admin } => {
            let access_key = format!("SWIA{}", random_token(18).to_uppercase());
            let secret = random_token(40);
            let payload = json!({ "label": label, "accessKey": access_key, "secretAccessKey": secret, "admin": admin });
            write::write(&db, "integration", &payload, WriteMode::Create, &Context::default())
                .await
                .map_err(describe)?;
            println!(
                "client_id:     {access_key}\nclient_secret: {secret}\n(the secret is not stored in plain text and cannot be shown again)"
            );
        }
        Command::DemoData => {
            let count = demo::run(&db).await.map_err(describe)?;
            println!("created {count} demo products");
        }
    }
    Ok(())
}

/// Random alphanumeric token from the OS RNG (via UUIDv4 bytes).
fn random_token(len: usize) -> String {
    const CHARS: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
    std::iter::repeat_with(|| uuid::Uuid::new_v4().into_bytes())
        .flatten()
        .filter(|b| (*b as usize) < CHARS.len() * 4)
        .map(|b| CHARS[b as usize % CHARS.len()] as char)
        .take(len)
        .collect()
}

fn describe(err: ferroshop_core::Error) -> anyhow::Error {
    match err {
        ferroshop_core::Error::Validation(list) => {
            let lines: Vec<String> = list.iter().map(|e| format!("{}: {}", e.pointer, e.detail)).collect();
            anyhow::anyhow!("validation failed:\n  {}", lines.join("\n  "))
        }
        other => other.into(),
    }
}

/// Loads `KEY=value` pairs from `.env` in the current or parent directory without overriding the environment.
fn dotenv_file() -> std::io::Result<()> {
    let content = std::fs::read_to_string(".env").or_else(|_| std::fs::read_to_string("../.env"))?;
    for line in content.lines().map(str::trim).filter(|l| !l.starts_with('#')) {
        if let Some((key, value)) = line.split_once('=')
            && std::env::var_os(key.trim()).is_none()
        {
            // SAFETY: called at startup before any other thread is spawned.
            unsafe { std::env::set_var(key.trim(), value.trim()) };
        }
    }
    Ok(())
}
