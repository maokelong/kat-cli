use crate::config::SkillRoot;
use anyhow::Result;
use clap::{Args, Subcommand};
use std::path::PathBuf;

#[derive(Debug, Args)]
pub struct ProfileCommand {
    #[command(subcommand)]
    pub action: ProfileAction,
}

#[derive(Debug, Subcommand)]
pub enum ProfileAction {
    List {
        #[arg(long)]
        skill_root: PathBuf,
    },
    Route {
        #[arg(long)]
        skill_root: PathBuf,
        #[arg(long)]
        question: String,
    },
}

pub fn run(cmd: ProfileCommand) -> Result<()> {
    match cmd.action {
        ProfileAction::List { skill_root } => {
            let skill = SkillRoot::load(skill_root)?;
            for profile in skill.profiles() {
                println!("{}\t{}", profile.id, profile.display_name);
            }
        }
        ProfileAction::Route {
            skill_root,
            question,
        } => {
            let skill = SkillRoot::load(skill_root)?;
            println!("{}", skill.route_question(&question));
        }
    }
    Ok(())
}
