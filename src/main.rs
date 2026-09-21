mod cli;
mod commands;
mod describe;
mod rows;

use clap::Parser;
use rbx::{db, output};

use cli::{Cli, Commands};
use commands::{history, mytags, playlists, query, tracks};

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    if matches!(cli.command, Commands::Describe { .. }) {
        let (out, code) = match cli.command {
            Commands::Describe { resource, action } => {
                (describe::handle_describe(resource, action), output::EXIT_OK)
            }
            _ => unreachable!(),
        };
        output::print(&out);
        if code != output::EXIT_OK {
            std::process::exit(code);
        }
        return;
    }

    let db_path = match &cli.db {
        Some(p) => p.clone(),
        None => {
            output::print(&output::error(
                "config",
                output::EXIT_CONFIG,
                "Missing --db path",
                Some("Set --db or RBX_DB_PATH environment variable"),
            ));
            std::process::exit(output::EXIT_CONFIG);
        }
    };

    let read_only = !cli::needs_write(&cli.command);
    let pool = match db::open(&db_path, read_only).await {
        Ok(p) => p,
        Err(e) => {
            output::print(&output::error(
                "config",
                output::EXIT_CONFIG,
                &format!("Failed to open database: {}", e),
                Some("Check that --db points to a valid rekordbox master.db"),
            ));
            std::process::exit(output::EXIT_CONFIG);
        }
    };

    let (out, code) = match cli.command {
        Commands::Tracks { action } => tracks::handle_tracks(&pool, action).await,
        Commands::Playlists { action } => {
            playlists::handle_playlists(&pool, &db_path, action).await
        }
        Commands::Mytags { action } => mytags::handle_mytags(&pool, action).await,
        Commands::History { action } => history::handle_history(&pool, action).await,
        Commands::Query { sql, unsafe_write } => {
            query::handle_query(&pool, &sql, unsafe_write).await
        }
        Commands::Describe { .. } => unreachable!(),
    };

    output::print(&out);
    if code != output::EXIT_OK {
        std::process::exit(code);
    }
}
