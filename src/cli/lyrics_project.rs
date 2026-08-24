use std::path::PathBuf;

use clap::Subcommand;

#[derive(Subcommand)]
pub enum LyricsCommand {
    /// Rewrite or enhance one selected lyrics span through Lyrics 2.0
    Rewrite(super::LyricsRewriteArgs),

    /// Combine two lyrics sources and wait for the generated mashup
    Mashup(super::LyricsMashupArgs),

    /// Read or resume polling a previously submitted lyrics mashup
    MashupStatus(super::LyricsMashupStatusArgs),

    /// Manage autosaved lyrics projects
    Projects(LyricsProjectsArgs),
}

#[derive(clap::Args)]
pub struct LyricsProjectsArgs {
    #[command(subcommand)]
    pub command: LyricsProjectCommand,
}

#[derive(Subcommand)]
pub enum LyricsProjectCommand {
    /// List every lyrics project, following Web pagination
    List,

    /// Create a lyrics project
    Create(LyricsProjectCreateArgs),

    /// Show one lyrics project
    Info(LyricsProjectInfoArgs),

    /// Rename a lyrics project
    Rename(LyricsProjectRenameArgs),

    /// Delete a lyrics project
    Delete(LyricsProjectDeleteArgs),

    /// Save lyrics into a project immediately
    Flush(LyricsProjectFlushArgs),
}

#[derive(clap::Args)]
pub struct LyricsProjectCreateArgs {
    /// Initial project title (the Web protocol stores at most 200 Unicode characters)
    #[arg(long, default_value = "")]
    pub title: String,
}

#[derive(clap::Args)]
pub struct LyricsProjectInfoArgs {
    /// Lyrics project ID
    pub id: String,
}

#[derive(clap::Args)]
pub struct LyricsProjectRenameArgs {
    /// Lyrics project ID
    pub id: String,

    /// New title
    #[arg(long)]
    pub title: String,
}

#[derive(clap::Args)]
pub struct LyricsProjectDeleteArgs {
    /// Lyrics project ID
    pub id: String,

    /// Confirm this destructive action
    #[arg(short = 'y', long)]
    pub yes: bool,
}

#[derive(clap::Args)]
pub struct LyricsProjectFlushArgs {
    /// Lyrics project ID
    pub id: String,

    /// Lyrics text to save
    #[arg(long, conflicts_with = "lyrics_file")]
    pub lyrics: Option<String>,

    /// Read lyrics from a UTF-8 text file
    #[arg(long, conflicts_with = "lyrics")]
    pub lyrics_file: Option<PathBuf>,
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use super::{LyricsCommand, LyricsProjectCommand};
    use crate::cli::{Cli, Commands};

    #[test]
    fn lyrics_project_flush_accepts_a_file_without_breaking_the_legacy_lyrics_command() {
        let cli = Cli::try_parse_from([
            "sunox",
            "lyrics",
            "projects",
            "flush",
            "project-1",
            "--lyrics-file",
            "draft.md",
        ])
        .expect("valid lyrics project flush command");
        let Some(Commands::Lyrics(args)) = cli.command else {
            panic!("expected lyrics command");
        };
        let Some(LyricsCommand::Projects(projects)) = args.command else {
            panic!("expected lyrics projects command");
        };
        let LyricsProjectCommand::Flush(args) = projects.command else {
            panic!("expected flush command");
        };
        assert_eq!(args.id, "project-1");
        assert_eq!(
            args.lyrics_file.as_deref(),
            Some(std::path::Path::new("draft.md"))
        );

        Cli::try_parse_from(["sunox", "lyrics", "--prompt", "write a chorus"])
            .expect("legacy Cowrite command remains valid");
    }

    #[test]
    fn lyrics_project_delete_carries_explicit_confirmation() {
        let cli = Cli::try_parse_from(["sunox", "lyrics", "projects", "delete", "project-1", "-y"])
            .expect("valid lyrics project delete");
        let Some(Commands::Lyrics(args)) = cli.command else {
            panic!("expected lyrics command");
        };
        let Some(LyricsCommand::Projects(projects)) = args.command else {
            panic!("expected lyrics projects command");
        };
        let LyricsProjectCommand::Delete(args) = projects.command else {
            panic!("expected delete command");
        };
        assert!(args.yes);
    }

    #[test]
    fn lyrics_rewrite_parses_explicit_selection_parts_and_file_inputs() {
        let cli = Cli::try_parse_from([
            "sunox",
            "lyrics",
            "rewrite",
            "--prompt",
            "make it brighter",
            "--prefix-file",
            "prefix.txt",
            "--edit",
            "old chorus",
            "--suffix",
            "outro",
            "--title",
            "Draft",
        ])
        .expect("valid lyrics rewrite command");
        let Some(Commands::Lyrics(args)) = cli.command else {
            panic!("expected lyrics command");
        };
        let Some(LyricsCommand::Rewrite(args)) = args.command else {
            panic!("expected rewrite command");
        };
        assert_eq!(args.prompt, "make it brighter");
        assert_eq!(
            args.prefix_file.as_deref(),
            Some(std::path::Path::new("prefix.txt"))
        );
        assert_eq!(args.edit.as_deref(), Some("old chorus"));
        assert_eq!(args.suffix.as_deref(), Some("outro"));
        assert_eq!(args.title, "Draft");
    }

    #[test]
    fn lyrics_mashup_supports_two_text_or_file_sources_and_bounded_wait() {
        let cli = Cli::try_parse_from([
            "sunox",
            "lyrics",
            "mashup",
            "--lyrics-a-file",
            "first.txt",
            "--lyrics-b",
            "second lyrics",
            "--timeout",
            "120",
        ])
        .expect("valid lyrics mashup command");
        let Some(Commands::Lyrics(args)) = cli.command else {
            panic!("expected lyrics command");
        };
        let Some(LyricsCommand::Mashup(args)) = args.command else {
            panic!("expected mashup command");
        };
        assert_eq!(
            args.lyrics_a_file.as_deref(),
            Some(std::path::Path::new("first.txt"))
        );
        assert_eq!(args.lyrics_b.as_deref(), Some("second lyrics"));
        assert_eq!(args.timeout, 120);
        assert!(!args.no_wait);
    }

    #[test]
    fn lyrics_mashup_status_is_a_read_only_recovery_command() {
        let cli = Cli::try_parse_from([
            "sunox",
            "lyrics",
            "mashup-status",
            "mashup-1",
            "--wait",
            "--timeout",
            "90",
        ])
        .expect("valid mashup status command");
        let Some(Commands::Lyrics(args)) = cli.command else {
            panic!("expected lyrics command");
        };
        let Some(LyricsCommand::MashupStatus(args)) = args.command else {
            panic!("expected mashup status command");
        };
        assert_eq!(args.id, "mashup-1");
        assert!(args.wait);
        assert_eq!(args.timeout, Some(90));
    }

    #[test]
    fn lyrics_mashup_status_timeout_requires_wait() {
        assert!(
            Cli::try_parse_from([
                "sunox",
                "lyrics",
                "mashup-status",
                "mashup-1",
                "--timeout",
                "90",
            ])
            .is_err()
        );

        Cli::try_parse_from(["sunox", "lyrics", "mashup-status", "mashup-1"])
            .expect("a one-shot read must not require --wait");
    }
}
