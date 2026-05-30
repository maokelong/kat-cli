use crate::config::{parse_strategy_file, SkillRoot};
use anyhow::{bail, Context, Result};
use clap::{Args, Subcommand};
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Args)]
pub struct StrategyCommand {
    #[command(subcommand)]
    pub action: StrategyAction,
}

#[derive(Debug, Subcommand)]
pub enum StrategyAction {
    List {
        #[arg(long)]
        skill_root: PathBuf,
        #[arg(long)]
        domain: Option<String>,
    },
    Render {
        #[arg(long)]
        skill_root: PathBuf,
        id: String,
    },
    Lint {
        #[arg(long)]
        skill_root: PathBuf,
        path: PathBuf,
    },
}

pub fn run(cmd: StrategyCommand) -> Result<()> {
    match cmd.action {
        StrategyAction::List { skill_root, domain } => {
            let skill = SkillRoot::load(skill_root)?;
            for strategy in skill.strategies() {
                if domain
                    .as_deref()
                    .map_or(true, |domain| domain == strategy.metadata.domain)
                {
                    println!(
                        "{}\t{}\t{}",
                        strategy.metadata.id, strategy.metadata.domain, strategy.metadata.status
                    );
                }
            }
        }
        StrategyAction::Render { skill_root, id } => {
            let skill = SkillRoot::load(skill_root)?;
            let strategy = skill
                .strategy(&id)
                .with_context(|| format!("未找到策略: {id}"))?;
            println!("---");
            println!("{}", serde_norway::to_string(&strategy.metadata)?.trim());
            println!("---");
            print!("{}", strategy.body);
        }
        StrategyAction::Lint { skill_root, path } => {
            let _ = SkillRoot::load(skill_root)?;
            let text =
                fs::read_to_string(&path).with_context(|| format!("读取 {}", path.display()))?;
            if !text.trim_start().starts_with("---") {
                bail!("缺少 YAML frontmatter");
            }
            let strategy = parse_strategy_file(&path)?;
            println!("ok\t{}\t{}", strategy.metadata.id, path.display());
        }
    }
    Ok(())
}
