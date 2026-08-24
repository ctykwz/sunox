use crate::app::AppContext;
use crate::cli::{LyricsProjectCommand, LyricsProjectsArgs};
use crate::core::{CliError, ensure_destructive_confirmed};
use crate::output::{self, OutputFormat};

pub async fn run(args: LyricsProjectsArgs, ctx: &AppContext) -> Result<(), CliError> {
    match args.command {
        LyricsProjectCommand::List => {
            let projects = ctx.client().await?.lyrics_projects().await?;
            render_projects(projects, ctx.fmt);
        }
        LyricsProjectCommand::Info(args) => {
            let id = project_id(args.id)?;
            let project = ctx.client().await?.lyrics_project(&id).await?;
            render_project(project, ctx.fmt);
        }
        LyricsProjectCommand::Create(args) => {
            let (client, _mutation_guard) = ctx.mutation_client().await?;
            let project = client.create_lyrics_project(&args.title).await?;
            render_project(project, ctx.fmt);
        }
        LyricsProjectCommand::Rename(args) => {
            let id = project_id(args.id)?;
            let (client, _mutation_guard) = ctx.mutation_client().await?;
            let project = client.rename_lyrics_project(&id, &args.title).await?;
            render_project(project, ctx.fmt);
        }
        LyricsProjectCommand::Flush(args) => {
            let id = project_id(args.id)?;
            let lyrics = match (args.lyrics, args.lyrics_file) {
                (Some(lyrics), None) => lyrics,
                (None, Some(path)) => std::fs::read_to_string(path)?,
                (None, None) => {
                    return Err(CliError::Config(
                        "lyrics project flush requires --lyrics or --lyrics-file".into(),
                    ));
                }
                (Some(_), Some(_)) => {
                    return Err(CliError::Config(
                        "lyrics project flush accepts only one of --lyrics or --lyrics-file".into(),
                    ));
                }
            };
            let (client, _mutation_guard) = ctx.mutation_client().await?;
            let response = client.flush_lyrics_project(&id, &lyrics).await?;
            match ctx.fmt {
                OutputFormat::Json => output::json::success(response),
                OutputFormat::Table => {
                    println!("Saved lyrics project {id} at {}", response.updated_at)
                }
            }
        }
        LyricsProjectCommand::Delete(args) => {
            ensure_destructive_confirmed(args.yes, "sunox lyrics projects delete")?;
            let id = project_id(args.id)?;
            let (client, _mutation_guard) = ctx.mutation_client().await?;
            client.delete_lyrics_project(&id).await?;
            match ctx.fmt {
                OutputFormat::Json => output::json::success(serde_json::json!({
                    "project_id": id,
                    "deleted": true,
                })),
                OutputFormat::Table => println!("Deleted lyrics project {id}"),
            }
        }
    }
    Ok(())
}

fn project_id(id: String) -> Result<String, CliError> {
    let id = id.trim();
    if id.is_empty() {
        return Err(CliError::Config(
            "lyrics project ID must not be empty".into(),
        ));
    }
    Ok(id.to_string())
}

fn render_projects(projects: Vec<crate::api::types::LyricsProject>, format: OutputFormat) {
    match format {
        OutputFormat::Json => output::json::success(projects),
        OutputFormat::Table => {
            if projects.is_empty() {
                println!("No lyrics projects");
            } else {
                for project in projects {
                    println!("{}\t{}", project.id, project.title);
                }
            }
        }
    }
}

fn render_project(project: crate::api::types::LyricsProject, format: OutputFormat) {
    match format {
        OutputFormat::Json => output::json::success(project),
        OutputFormat::Table => {
            println!("{}\t{}", project.id, project.title);
            if !project.lyrics.is_empty() {
                println!("{}", project.lyrics);
            }
        }
    }
}
