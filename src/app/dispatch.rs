use clap::Parser;

use super::AppContext;
use crate::cli::{
    AuthArgs, Cli, ClipCommand, Commands, ConfigAction, ConfigArgs, PlaylistTracksArgs,
};
use crate::commands;
use crate::core::CliError;

pub async fn run() -> Result<(), CliError> {
    let cli = Cli::parse();
    let ctx = AppContext::new(cli.json, cli.quiet, cli.parallel, &cli.config_overrides)?;
    let Cli {
        prompt, command, ..
    } = cli;

    dispatch_command(command, prompt, &ctx).await
}

async fn dispatch_command(
    command: Option<Commands>,
    prompt: Option<String>,
    ctx: &AppContext,
) -> Result<(), CliError> {
    match command {
        Some(Commands::Create(args)) => commands::create::create(args, ctx).await,
        Some(Commands::Download(args)) => commands::media::download(args, ctx).await,
        Some(Commands::Add(args)) => {
            commands::playlist::run(
                crate::cli::PlaylistArgs {
                    command: crate::cli::PlaylistCommand::Add(PlaylistTracksArgs {
                        id: args.playlist_id,
                        clip_ids: args.clip_ids,
                    }),
                },
                ctx,
            )
            .await
        }
        None => {
            let Some(prompt) = prompt else {
                return Err(CliError::Config("provide a prompt or command".into()));
            };
            commands::create::create(
                crate::cli::CreateArgs {
                    prompt: Some(prompt),
                    title: None,
                    tags: None,
                    exclude: None,
                    lyrics: None,
                    lyrics_file: None,
                    model: None,
                    vocal: None,
                    weirdness: None,
                    style_influence: None,
                    enhance_tags: false,
                    instrumental: false,
                    token: None,
                    captcha: false,
                    no_captcha: false,
                    persona: None,
                },
                ctx,
            )
            .await
        }
        Some(Commands::Clip(args)) => run_clip(args.command, ctx).await,
        Some(Commands::Login) => commands::auth::run(auth_args_login(), ctx).await,
        Some(Commands::Logout) => commands::auth::run(auth_args_logout(), ctx).await,
        Some(Commands::Doctor(args)) if args.network => {
            commands::doctor::network(ctx, args.strict).await
        }
        Some(Commands::Doctor(_)) => {
            commands::config::run(
                ConfigArgs {
                    action: ConfigAction::Check,
                },
                ctx,
            )
            .await
        }
        Some(Commands::Auth(args)) => commands::auth::run(args, ctx).await,
        Some(Commands::Credits) => commands::account::credits(ctx).await,
        Some(Commands::Models) => commands::account::models(ctx).await,
        Some(Commands::Lyrics(args)) => commands::create::lyrics(args, ctx).await,
        Some(Commands::Persona(args)) => commands::persona::run(args, ctx).await,
        Some(Commands::Playlist(args)) => commands::playlist::run(args, ctx).await,
        Some(Commands::Config(args)) => commands::config::run(args, ctx).await,
        Some(Commands::AgentInfo) => commands::agent::agent_info(ctx).await,
        Some(Commands::InstallSkill(args)) => commands::agent::install_skill(args, ctx).await,
        Some(Commands::Update(args)) => commands::update::run(args, ctx).await,
    }
}

async fn run_clip(command: ClipCommand, ctx: &AppContext) -> Result<(), CliError> {
    match command {
        ClipCommand::List(args) => commands::library::list(args, ctx).await,
        ClipCommand::Search(args) => commands::library::search(args, ctx).await,
        ClipCommand::Info(args) => commands::library::info(args, ctx).await,
        ClipCommand::Status(args) => commands::library::status(args, ctx).await,
        ClipCommand::Wait(args) => commands::wait::run(args, ctx).await,
        ClipCommand::Download(args) => commands::media::download(args, ctx).await,
        ClipCommand::Upload(args) => commands::media::upload(args, ctx).await,
        ClipCommand::UploadStatus(args) => commands::media::upload_status(args, ctx).await,
        ClipCommand::Delete(args) => commands::library::delete(args, ctx).await,
        ClipCommand::Restore(args) => commands::library::restore(args, ctx).await,
        ClipCommand::Purge(args) => commands::library::purge(args, ctx).await,
        ClipCommand::EmptyTrash(args) => commands::library::empty_trash(args, ctx).await,
        ClipCommand::Like(args) => commands::library::like(args, ctx).await,
        ClipCommand::Dislike(args) => commands::library::dislike(args, ctx).await,
        ClipCommand::Set(args) => commands::library::set(args, ctx).await,
        ClipCommand::Publish(args) => commands::library::publish(args, ctx).await,
        ClipCommand::TimedLyrics(args) => commands::media::timed_lyrics(args, ctx).await,
        ClipCommand::Extend(args) => commands::create::extend(args, ctx).await,
        ClipCommand::Concat(args) => commands::create::concat(args, ctx).await,
        ClipCommand::Cover(args) => commands::create::cover(args, ctx).await,
        ClipCommand::Inspire(args) => commands::create::inspire(args, ctx).await,
        ClipCommand::Remaster(args) => commands::create::remaster(args, ctx).await,
        ClipCommand::Speed(args) => commands::create::speed(args, ctx).await,
        ClipCommand::Reverse(args) => commands::create::reverse(args, ctx).await,
        ClipCommand::Crop(args) => commands::create::crop(args, ctx).await,
        ClipCommand::Fade(args) => commands::create::fade(args, ctx).await,
        ClipCommand::Stems(args) => commands::create::stems(args, ctx).await,
    }
}

fn auth_args_login() -> AuthArgs {
    AuthArgs {
        login: true,
        refresh: false,
        jwt: None,
        jwt_stdin: false,
        cookie: None,
        cookie_stdin: false,
        device: None,
        logout: false,
    }
}

fn auth_args_logout() -> AuthArgs {
    AuthArgs {
        login: false,
        refresh: false,
        jwt: None,
        jwt_stdin: false,
        cookie: None,
        cookie_stdin: false,
        device: None,
        logout: true,
    }
}
