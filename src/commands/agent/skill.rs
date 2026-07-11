use crate::app::AppContext;
use crate::cli::{InstallSkillArgs, SkillTarget};
use crate::core::CliError;
use crate::output::{self, OutputFormat};

pub async fn install_skill(args: InstallSkillArgs, ctx: &AppContext) -> Result<(), CliError> {
    const SKILL_BODY: &str = include_str!("../../../assets/SKILL.md");

    if args.print {
        print!("{SKILL_BODY}");
        return Ok(());
    }

    let home = crate::core::user_home_dir()
        .ok_or_else(|| CliError::Config("could not determine home directory".into()))?;

    let dest_path: std::path::PathBuf = if let Some(custom) = args.path {
        if let Some(stripped) = custom.strip_prefix("~/") {
            home.join(stripped)
        } else if custom == "~" {
            home.clone()
        } else {
            std::path::PathBuf::from(custom)
        }
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
    std::fs::write(&dest_path, SKILL_BODY)?;

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
