use crate::app::AppContext;
use crate::cli::{InstallSkillArgs, SkillTarget};
use crate::core::CliError;
use crate::output::{self, OutputFormat};
use std::io::Write;

pub async fn install_skill(args: InstallSkillArgs, ctx: &AppContext) -> Result<(), CliError> {
    const SKILL_BODY: &str = include_str!("../../../assets/SKILL.md");

    if args.print {
        print!("{SKILL_BODY}");
        return Ok(());
    }

    let home = crate::core::user_home_dir()
        .ok_or_else(|| CliError::Config("could not determine home directory".into()))?;

    let dest_path: std::path::PathBuf = if let Some(custom) = args.path {
        expand_home_path(&custom, &home)
    } else {
        match args.target {
            SkillTarget::Codex => home.join(".codex/skills/sunox/SKILL.md"),
            SkillTarget::Claude => home.join(".claude/skills/sunox/SKILL.md"),
            SkillTarget::Cursor => std::env::current_dir()?.join(".cursor/rules/sunox.mdc"),
        }
    };

    if dest_path.exists() && !args.force {
        return Err(CliError::Config(format!(
            "{} already exists — pass --force to overwrite",
            dest_path.display()
        )));
    }

    if let Some(parent) = dest_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let parent = dest_path
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."));
    let mut staged = tempfile::NamedTempFile::new_in(parent)?;
    staged.write_all(SKILL_BODY.as_bytes())?;
    staged.as_file().sync_all()?;
    staged
        .persist(&dest_path)
        .map_err(|error| CliError::Io(error.error))?;

    match ctx.fmt {
        OutputFormat::Json => output::json::success(serde_json::json!({
            "installed": true,
            "path": dest_path.display().to_string(),
            "target": match args.target {
                SkillTarget::Codex => "codex",
                SkillTarget::Claude => "claude",
                SkillTarget::Cursor => "cursor",
            },
        })),
        OutputFormat::Table => {
            eprintln!("Installed sunox skill to: {}", dest_path.display());
            match args.target {
                SkillTarget::Codex => {
                    eprintln!("Restart Codex / Trae CLI to pick up the new skill.");
                }
                SkillTarget::Claude => {
                    eprintln!("Restart Claude Code to pick up the new skill.");
                }
                SkillTarget::Cursor => {
                    eprintln!("Cursor will pick up the rule on next workspace reload.");
                }
            }
        }
    }
    Ok(())
}

fn expand_home_path(custom: &str, home: &std::path::Path) -> std::path::PathBuf {
    if custom == "~" {
        return home.to_path_buf();
    }
    custom
        .strip_prefix("~/")
        .or_else(|| custom.strip_prefix(r"~\"))
        .map(|stripped| home.join(stripped))
        .unwrap_or_else(|| std::path::PathBuf::from(custom))
}

#[cfg(test)]
mod tests {
    use super::expand_home_path;
    use std::path::Path;
    #[cfg(windows)]
    use std::path::PathBuf;

    #[test]
    fn custom_skill_path_expands_both_path_separator_styles() {
        let home = Path::new(r"C:\Users\alice");

        #[cfg(windows)]
        assert_eq!(
            expand_home_path(r"~\skills\sunox.md", home),
            PathBuf::from(r"C:\Users\alice\skills\sunox.md")
        );
        assert_eq!(
            expand_home_path("~/skills/sunox.md", home),
            home.join("skills/sunox.md")
        );
    }
}
