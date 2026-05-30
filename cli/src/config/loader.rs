use crate::config::models::{Atomic, Profile, RoleRouter, Strategy, StrategyMetadata};
use crate::config::paths::SkillPaths;
use anyhow::{bail, Context, Result};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct SkillRoot {
    router: Option<RoleRouter>,
    profiles: BTreeMap<String, Profile>,
    atomics: BTreeMap<String, Atomic>,
    strategies: BTreeMap<String, Strategy>,
}

impl SkillRoot {
    pub fn load(root: impl AsRef<Path>) -> Result<Self> {
        let paths = SkillPaths::new(root);
        let router = load_router(&paths.router)?;
        let profiles = load_profiles(&paths.profiles)?;
        let atomics = load_atomics(&paths.atomics)?;
        let strategies = load_strategies(&paths.strategies_approved)?;
        Ok(Self {
            router,
            profiles,
            atomics,
            strategies,
        })
    }

    pub fn profiles(&self) -> impl Iterator<Item = &Profile> {
        self.profiles.values()
    }

    pub fn strategies(&self) -> impl Iterator<Item = &Strategy> {
        self.strategies.values()
    }

    pub fn atomics(&self) -> impl Iterator<Item = &Atomic> {
        self.atomics.values()
    }

    pub fn profile(&self, id: &str) -> Option<&Profile> {
        self.profiles.get(id)
    }

    pub fn atomic(&self, id: &str) -> Option<&Atomic> {
        self.atomics.get(id)
    }

    pub fn strategy(&self, id: &str) -> Option<&Strategy> {
        self.strategies.get(id)
    }

    pub fn route_question(&self, question: &str) -> String {
        let Some(router) = &self.router else {
            return self
                .profiles
                .keys()
                .next()
                .cloned()
                .unwrap_or_else(|| "unknown".to_string());
        };
        let question = question.to_lowercase();
        for domain in &router.domains {
            if domain
                .aliases
                .iter()
                .any(|alias| question.contains(&alias.to_lowercase()))
            {
                return domain.id.clone();
            }
        }
        router.default_domain.clone()
    }
}

fn load_router(path: &Path) -> Result<Option<RoleRouter>> {
    if !path.exists() {
        return Ok(None);
    }
    let text = fs::read_to_string(path).with_context(|| format!("读取 {}", path.display()))?;
    let router: RoleRouter =
        serde_norway::from_str(&text).with_context(|| format!("解析 {}", path.display()))?;
    Ok(Some(router))
}

fn load_profiles(dir: &Path) -> Result<BTreeMap<String, Profile>> {
    let mut out = BTreeMap::new();
    for path in yaml_files(dir)? {
        let text = fs::read_to_string(&path).with_context(|| format!("读取 {}", path.display()))?;
        let profile: Profile =
            serde_norway::from_str(&text).with_context(|| format!("解析 {}", path.display()))?;
        out.insert(profile.id.clone(), profile);
    }
    Ok(out)
}

fn load_atomics(root: &Path) -> Result<BTreeMap<String, Atomic>> {
    let mut out = BTreeMap::new();
    if !root.exists() {
        return Ok(out);
    }
    for domain in fs::read_dir(root).with_context(|| format!("读取 {}", root.display()))? {
        let domain = domain?;
        if !domain.file_type()?.is_dir() {
            continue;
        }
        for path in yaml_files(&domain.path())? {
            let text =
                fs::read_to_string(&path).with_context(|| format!("读取 {}", path.display()))?;
            let atomic: Atomic = serde_norway::from_str(&text)
                .with_context(|| format!("解析 {}", path.display()))?;
            out.insert(atomic.id.clone(), atomic);
        }
    }
    Ok(out)
}

fn load_strategies(dir: &Path) -> Result<BTreeMap<String, Strategy>> {
    let mut out = BTreeMap::new();
    if !dir.exists() {
        return Ok(out);
    }
    for path in markdown_files(dir)? {
        let strategy = parse_strategy_file(&path)?;
        out.insert(strategy.metadata.id.clone(), strategy);
    }
    Ok(out)
}

pub fn parse_strategy_file(path: &Path) -> Result<Strategy> {
    let text = fs::read_to_string(path).with_context(|| format!("读取 {}", path.display()))?;
    let normalized = text.replace("\r\n", "\n");
    let stripped = normalized
        .strip_prefix("---\n")
        .context("策略 frontmatter 必须以 --- 开始")?;
    let (frontmatter, body) = stripped
        .split_once("\n---\n")
        .context("策略 frontmatter 必须以 --- 结束")?;
    let metadata: StrategyMetadata = serde_norway::from_str(frontmatter)
        .with_context(|| format!("解析 frontmatter {}", path.display()))?;
    Ok(Strategy {
        metadata,
        body: body.to_string(),
        path: PathBuf::from(path),
    })
}

fn yaml_files(dir: &Path) -> Result<Vec<PathBuf>> {
    if !dir.exists() {
        bail!("缺少目录 {}", dir.display());
    }
    let mut paths = Vec::new();
    for entry in fs::read_dir(dir).with_context(|| format!("读取 {}", dir.display()))? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|value| value.to_str()) == Some("yaml") {
            paths.push(path);
        }
    }
    paths.sort();
    Ok(paths)
}

fn markdown_files(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut paths = Vec::new();
    for entry in fs::read_dir(dir).with_context(|| format!("读取 {}", dir.display()))? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|value| value.to_str()) == Some("md") {
            paths.push(path);
        }
    }
    paths.sort();
    Ok(paths)
}
