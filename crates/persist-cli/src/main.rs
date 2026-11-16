//! `persist` — the Persist command line interface.

mod cli;
mod commands;
mod output;
mod progress;

use clap::Parser;
use cli::{Cli, Command};
use commands::Ctx;
use persist_core::config::Paths;
use persist_core::error::Error;
use persist_core::session::Session;
use progress::CancelFlag;

fn main() {
    let cli = Cli::parse();

    // Install a Ctrl-C handler that flips the cancellation flag so long
    // operations stop cleanly instead of being killed mid-write.
    let cancel = CancelFlag::new();

    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("error: could not start async runtime: {e}");
            std::process::exit(1);
        }
    };

    let code = runtime.block_on(async move {
        spawn_interrupt_handler(cancel.clone());
        run(cli, cancel).await
    });
    std::process::exit(code);
}

async fn run(cli: Cli, cancel: CancelFlag) -> i32 {
    match dispatch(cli, cancel).await {
        Ok(()) => 0,
        Err(e) => {
            let json = std::env::args().any(|a| a == "--json");
            if json {
                let payload = serde_json::json!({
                    "ok": false,
                    "error": { "code": e.code(), "message": e.to_string() }
                });
                eprintln!("{}", serde_json::to_string_pretty(&payload).unwrap_or_default());
            } else {
                output::error(&e.to_string());
            }
            e.exit_code()
        }
    }
}

async fn dispatch(cli: Cli, cancel: CancelFlag) -> Result<(), Error> {
    // `completions` does not need a session.
    if let Command::Completions { shell } = cli.command {
        commands::completions(shell);
        return Ok(());
    }

    let paths = match &cli.home {
        Some(home) => Paths {
            config_dir: home.join("config"),
            data_dir: home.join("data"),
            cache_dir: home.join("cache"),
        },
        None => Paths::resolve()?,
    };
    let session = Session::open(paths)?;
    let mut ctx = Ctx {
        session,
        json: cli.json,
        yes: cli.yes,
        quiet: cli.quiet,
        cancel,
    };

    match cli.command {
        Command::Login { .. } => commands::login(&mut ctx).await,
        Command::Logout => commands::logout(&mut ctx),
        Command::Account => commands::account(&ctx).await,
        Command::Init {
            name,
            local,
            drive_folder,
        } => commands::init(&mut ctx, name, local, drive_folder).await,
        Command::Clone {
            repository_id,
            destination,
        } => commands::clone(&mut ctx, repository_id, destination).await,
        Command::Add {
            path,
            remote,
            name,
            exclude,
        } => commands::add(&ctx, path, remote, name, exclude),
        Command::Remove { path } => commands::remove(&ctx, path),
        Command::List => commands::list(&ctx),
        Command::Push {
            path,
            dry_run,
            message,
        } => commands::push(&mut ctx, path, dry_run, message).await,
        Command::Status => commands::status(&ctx).await,
        Command::Pull {
            snapshot,
            destination,
            root,
            paths,
            overwrite,
            skip,
            rename,
            dry_run,
        } => {
            let args = cli::RestoreArgs {
                destination,
                root,
                paths,
                overwrite,
                skip,
                rename,
                dry_run,
            };
            commands::pull(&mut ctx, args, snapshot).await
        }
        Command::Restore {
            snapshot_id,
            options,
        } => commands::pull(&mut ctx, options, Some(snapshot_id)).await,
        Command::Log { limit } => commands::log(&ctx, limit).await,
        Command::Diff {
            snapshot_a,
            snapshot_b,
            text,
        } => commands::diff(&ctx, snapshot_a, snapshot_b, text).await,
        Command::Storage => commands::storage(&ctx).await,
        Command::Verify { deep } => commands::verify(&ctx, deep).await,
        Command::Config { action } => commands::config(&mut ctx, action),
        Command::Completions { .. } => unreachable!("handled above"),
    }
}

/// Install a SIGINT handler. On non-unix platforms this is a no-op.
fn spawn_interrupt_handler(cancel: CancelFlag) {
    #[cfg(unix)]
    {
        tokio::spawn(async move {
            if tokio::signal::ctrl_c().await.is_ok() {
                eprintln!("\ninterrupt received, finishing current operation safely…");
                cancel.cancel();
            }
        });
    }
    #[cfg(not(unix))]
    {
        let _ = cancel;
    }
}
