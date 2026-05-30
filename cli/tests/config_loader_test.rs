use htrace::config::SkillRoot;
use std::fs;
use tempfile::tempdir;

#[test]
fn loads_profile_atomic_and_strategy_from_skill_root() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    fs::create_dir_all(root.join("config/profiles")).unwrap();
    fs::create_dir_all(root.join("atomics/scheduler-kernel")).unwrap();
    fs::create_dir_all(root.join("strategies/approved")).unwrap();

    fs::write(
        root.join("config/role-router.yaml"),
        r#"
domains:
  - id: scheduler-kernel
    aliases: ["调度", "冷启动"]
default_domain: scheduler-kernel
"#,
    )
    .unwrap();

    fs::write(
        root.join("config/profiles/scheduler-kernel.yaml"),
        r#"
id: scheduler-kernel
display_name: 调度/内核
knowledge:
  - knowledge/scheduler-kernel/cold-start-topdown.md
overview_atomics:
  - trace_sanity_check
approved_strategies:
  - cold-start-scheduler-topdown
allowed_atomics:
  - trace_sanity_check
"#,
    )
    .unwrap();

    fs::write(
        root.join("atomics/scheduler-kernel/trace_sanity_check.yaml"),
        r#"
id: trace_sanity_check
domain: scheduler-kernel
engine: perfetto-sql
description: 检查 trace 基础表是否可用。
inputs: {}
resources:
  timeout_ms: 1000
  max_rows: 100
  max_result_bytes: 4096
  priority: p0
sql: "SELECT 1 AS ok;"
outputs:
  columns:
    - name: ok
      type: int64
"#,
    )
    .unwrap();

    fs::write(
        root.join("strategies/approved/cold-start-scheduler-topdown.md"),
        r#"---
id: cold-start-scheduler-topdown
domain: scheduler-kernel
status: approved
allowed_atomics:
  - trace_sanity_check
review_required: false
---

# 冷启动调度分析
"#,
    )
    .unwrap();

    let skill = SkillRoot::load(root).unwrap();
    assert_eq!(
        skill.profile("scheduler-kernel").unwrap().id,
        "scheduler-kernel"
    );
    assert_eq!(
        skill.atomic("trace_sanity_check").unwrap().domain,
        "scheduler-kernel"
    );
    assert_eq!(
        skill
            .strategy("cold-start-scheduler-topdown")
            .unwrap()
            .metadata
            .status,
        "approved"
    );
    assert_eq!(skill.route_question("冷启动为什么慢"), "scheduler-kernel");
}
