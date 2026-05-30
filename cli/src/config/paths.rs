use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct SkillPaths {
    pub root: PathBuf,
    pub router: PathBuf,
    pub profiles: PathBuf,
    pub atomics: PathBuf,
    pub strategies_approved: PathBuf,
    pub strategies_generated: PathBuf,
}

impl SkillPaths {
    pub fn new(root: impl AsRef<Path>) -> Self {
        let root = root.as_ref().to_path_buf();
        Self {
            router: root.join("config/role-router.yaml"),
            profiles: root.join("config/profiles"),
            atomics: root.join("atomics"),
            strategies_approved: root.join("strategies/approved"),
            strategies_generated: root.join("strategies/generated"),
            root,
        }
    }
}
