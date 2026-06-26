use std::{
    fs::{self, OpenOptions},
    io::{Read, Seek, SeekFrom, Write},
    path::{Component, Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use serde_json::Value;

const CHECKLIST_RENDERED_MARKER: &str = "由 plan/state/evidence 渲染";

pub struct AnalysisRunStore {
    run_dir: PathBuf,
}

impl AnalysisRunStore {
    pub fn create(root: impl AsRef<Path>, run_id: &str) -> Result<Self> {
        validate_run_id(run_id)?;
        let run_dir = root.as_ref().join(run_id);
        fs::create_dir_all(&run_dir)
            .with_context(|| format!("failed to create run dir {}", run_dir.display()))?;
        Ok(Self { run_dir })
    }

    pub fn write_plan(&self, plan: &Value) -> Result<()> {
        self.write_json("plan.json", plan)
    }

    pub fn write_state(&self, state: &Value) -> Result<()> {
        self.write_json("state.json", state)
    }

    pub fn append_evidence(&self, evidence: &Value) -> Result<()> {
        let path = self.run_dir.join("evidence.jsonl");
        let mut file = OpenOptions::new()
            .create(true)
            .read(true)
            .append(true)
            .open(&path)
            .with_context(|| format!("failed to open {}", path.display()))?;
        if file.metadata()?.len() > 0 {
            file.seek(SeekFrom::End(-1))
                .with_context(|| format!("failed to inspect {}", path.display()))?;
            let mut last = [0];
            file.read_exact(&mut last)
                .with_context(|| format!("failed to inspect {}", path.display()))?;
            if last[0] != b'\n' {
                writeln!(file).with_context(|| format!("failed to separate {}", path.display()))?;
            }
        }
        serde_json::to_writer(&mut file, evidence)
            .with_context(|| format!("failed to append {}", path.display()))?;
        writeln!(file).with_context(|| format!("failed to terminate {}", path.display()))?;
        Ok(())
    }

    pub fn render_checklist(&self) -> Result<()> {
        let text = format!(
            "# Analysis Checklist\n\n本文件{CHECKLIST_RENDERED_MARKER}，不是机器执行 source of truth。\n\n- [ ] 查看 plan.json\n- [ ] 查看 state.json\n- [ ] 查看 evidence.jsonl\n"
        );
        fs::write(self.run_dir.join("checklist.md"), text).with_context(|| {
            format!(
                "failed to write {}",
                self.run_dir.join("checklist.md").display()
            )
        })
    }

    pub fn write_report(&self, report: &str) -> Result<()> {
        fs::write(self.run_dir.join("report.md"), report).with_context(|| {
            format!(
                "failed to write {}",
                self.run_dir.join("report.md").display()
            )
        })
    }

    fn write_json(&self, name: &str, value: &Value) -> Result<()> {
        let path = self.run_dir.join(name);
        let raw = serde_json::to_vec_pretty(value)?;
        fs::write(&path, raw).with_context(|| format!("failed to write {}", path.display()))
    }
}

fn validate_run_id(run_id: &str) -> Result<()> {
    if run_id.is_empty()
        || run_id.contains('/')
        || run_id.contains('\\')
        || !run_id
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.'))
    {
        bail!("invalid analysis run id {run_id:?}");
    }

    let mut components = Path::new(run_id).components();
    match (components.next(), components.next()) {
        (Some(Component::Normal(component)), None) if component == run_id => Ok(()),
        _ => bail!("invalid analysis run id {run_id:?}"),
    }
}
