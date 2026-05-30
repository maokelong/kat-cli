# Harmony Trace Skill Self-Evolution Design

## Context

The repository contains a Codex skill package under `skill/` and a deterministic
Rust CLI runtime (`htrace`) under `cli/`. The skill analyzes HarmonyOS and
Perfetto-compatible trace files through an eight-stage workflow:

1. `collect_input`
2. `load_profile`
3. `overview_atomics`
4. `topdown_brief`
5. `strategy_selection`
6. `deep_analysis`
7. `replay_generation`
8. `final_report`

The self-evolution system must repeatedly improve the skill and CLI, install the
current skill into an isolated test location, run an end-to-end analysis against
`D:\work\self_improved\test\test.htrace`, collect issues, apply improvements,
record each round in `version.md`, and push each round to the remote repository.

The user selects the number of evolution rounds. Each round must be able to
survive context compaction by relying on files, commits, and round artifacts
rather than conversational memory.

## Goals

- Run an automatic closed-loop evolution process for a user-specified number of
  rounds.
- Use fresh Codex sessions or subagents for validation to avoid contaminating the
  main context.
- Validate every round end to end by completing all eight skill stages on
  `test.htrace`.
- Use multiple review agents per round to independently evaluate flow control,
  report quality, performance, and tool correctness.
- Use implementation worker subagents that explicitly apply Superpowers
  workflows when modifying code.
- Record every round in `version.md`.
- Commit and push every round's source changes to the remote branch.
- Create a handoff artifact after each round so the next round can resume cleanly
  after context compaction.

## Non-Goals

- Do not make this system depend on direct SQL calls to `trace_processor_shell`.
  All trace analysis must continue through `htrace` atomics, replay, or approved
  CLI entry points.
- Do not allow validation agents to reuse prior round conclusions as evidence for
  the current round.
- Do not treat a successful smoke test as a replacement for the full eight-stage
  skill workflow.
- Do not auto-approve product-direction decisions that require the user's
  judgment.

## Evolution Dimensions

Each round may improve one or more of these dimensions:

- User experience: stage panels, progress wording, blocked-state explanations,
  final report readability, actionable next steps.
- Tool bugfixes: CLI command failures, missing parameters, path handling,
  install or verification issues, malformed artifacts.
- Flow control: strict use of `run go`, `run guard`, `run validate`, and
  `run advance`; no manual `run-state.yaml` edits.
- Performance: atomic runtime, trace processor startup cost, output volume,
  memory-sensitive batching, repeated work elimination.
- Evidence quality: every conclusion cites atomic fields or artifact paths.
- Recoverability: resuming from `.last-run`, `run-state.yaml`, `progress.md`,
  `version.md`, and round `handoff.md`.
- Observability: logs and artifacts make failures diagnosable without relying on
  chat history.
- Portability: improvements should make later traces easier to analyze without
  overfitting to `test.htrace`.

## Architecture

### Orchestrator

The main Codex session acts as the orchestrator. It owns round scheduling,
agent dispatch, integration, final verification, `version.md`, commit, push, and
handoff creation.

The orchestrator must not rely on memory for critical state. At the start of each
round it reads:

- `version.md`, if present.
- The latest `test/evolution/round-*/handoff.md`, if present.
- Git status and the current commit.
- The selected round count and current round index.
- External helper scripts from `D:\work\self_improved\test\evolution-tools`.

### Implementation Workers

Implementation workers are subagents that modify `kat-rs` source files. They
must use relevant Superpowers workflows such as:

- `systematic-debugging` for failing E2E or CLI behavior.
- `test-driven-development` for bug fixes or new behavior where practical.
- `executing-plans` when implementing an approved multi-step plan.
- `verification-before-completion` before reporting success.

Each worker receives a disjoint ownership scope, for example:

- `skill/` only.
- `cli/src/run/` only.
- `cli/src/commands/` only.
- `D:\work\self_improved\test\evolution-tools\` only for helper script work
  that must remain outside the repository.

Workers must not revert or overwrite unrelated changes. Their final response must
include changed files, workflow used, verification commands, results, and
residual risks.

### E2E Runner

The E2E runner is a fresh subagent or fresh Codex session. It receives only:

- The current round skill path.
- The test trace path.
- The round output directory.
- A task prompt requiring all eight stages.

It must produce current-round artifacts under `test/evolution/round-N/e2e/`.
It must not read previous round analysis reports except when the prompt
explicitly asks for regression comparison.

### Flow Auditor

The flow auditor independently reviews command logs, run-state artifacts, and
stage outputs. It checks:

- `htrace run go` is the first workflow command after locating the run.
- Stage transitions use `run advance`.
- Stage actions are allowed by `guard` or current stage metadata.
- `run validate` occurs after stage artifact writes.
- No direct `trace_processor_shell -Q`, `trace_processor_shell -q`, or ad hoc SQL
  path is used.
- `run-state.yaml` is not edited manually.

Flow violations are hard failures.

### Report Reviewer

The report reviewer reads the final report and supporting artifacts. It checks:

- Every major conclusion has an evidence citation.
- The report separates facts, inferences, and uncertainty.
- Missing trace fields and fallback methods are explained.
- The Chinese user-facing writing is clear and actionable.
- The report does not overstate unsupported conclusions.

### Performance Reviewer

The performance reviewer inspects runtime logs and output size. It records:

- Total E2E wall-clock time.
- Per-atomic runtime when available.
- Trace processor startup count.
- Largest artifacts.
- Repeated work or avoidable re-runs.
- Memory or concurrency risks.

## Round State Machine

Each round follows this state machine:

1. `prepare_round`
   - Create `test/evolution/round-N/`.
   - Record start commit, branch, remote, trace path, and source skill checksum.
   - Install or copy `kat-rs/skill` to
     `test/evolution/round-N/skills/harmony-trace-analysis`.

2. `baseline_or_target_selection`
   - For round 0, establish baseline E2E behavior.
   - For later rounds, select improvement targets from the previous round report.
   - Limit the round scope to a small number of fixes.

3. `implementation`
   - Dispatch one or more Superpowers implementation workers.
   - Integrate returned changes.
   - Inspect diffs before validation.

4. `package_verification`
   - Run Codex skill validation on `skill/`.
   - Run `skill/verify.ps1`.
   - Reinstall/copy the skill into the round test directory.

5. `fresh_e2e`
   - Dispatch E2E runner.
   - Require all eight stages.
   - Persist logs and artifacts.

6. `independent_reviews`
   - Dispatch flow auditor, report reviewer, and performance reviewer.
   - Keep reviews independent when possible.

7. `synthesis`
   - Merge findings into `round-report.md`.
   - Classify issues as hard failures, fixed issues, deferred issues, and next
     candidates.
   - Decide whether the round passes or stops.

8. `record_and_push`
   - Update `version.md`.
   - Commit the round.
   - Push to the remote branch.
   - Run the external submission gate and require `ok=true`.

9. `handoff_and_compaction`
   - Write `handoff.md`.
   - Close subagents.
   - Trigger context compaction if the host exposes a mechanism.
   - If no explicit compaction mechanism exists, start the next round by reading
     files only, not chat memory.

## Directory Contract

The source repository gains only versioned design and documentation files:

```text
kat-rs/
  version.md
  docs/superpowers/specs/2026-05-29-skill-self-evolution-design.md
  docs/superpowers/plans/2026-05-29-skill-self-evolution.md
```

Helper scripts are intentionally external and must not be added to the `kat-rs`
repository:

```text
test/
  evolution-tools/
    install-round-skill.ps1
    verify-round.ps1
    new-agent-prompts.ps1
    collect-round-report.ps1
    assert-round-submission.ps1
```

The test workspace uses:

```text
test/
  test.htrace
  evolution/
    round-000-baseline/
    round-001/
      skills/harmony-trace-analysis/
      e2e/
        command-log.md
        artifacts/
        final-report.md
      reviews/
        flow-audit.md
        report-review.md
        performance-review.md
      round-report.md
      handoff.md
```

Scripts under `D:\work\self_improved\test\evolution-tools\` are optional for the
first implementation pass and are non-versioned run helpers. The directory and
artifact layout are mandatory so each round can be audited and resumed without
polluting the source repository.

## E2E Requirements

An E2E run is valid only if all of these are true:

- The runner uses the round-installed skill path, not an older global skill copy.
- The test trace is `D:\work\self_improved\test\test.htrace`.
- The workflow reaches all eight stages in order.
- Each stage records either the expected artifact or a stage-specific reason why
  no artifact is possible.
- `htrace run go <run-dir> --json` is used before acting on a run.
- Stage writes are followed by `htrace run validate <run-dir> --json`.
- Stage transitions use `htrace run advance`.
- The final report exists and references evidence artifacts.

## Review Scoring

Each completed round receives 0-5 scores:

- UX: clarity of stage visibility, blocked states, and final guidance.
- Tool correctness: CLI, atomic, replay, install, and verification reliability.
- Flow control: compliance with the run workflow protocol.
- Performance: runtime, output volume, trace processor usage, and memory risk.
- Evidence quality: citation of atomic outputs and artifact paths.
- Recoverability: ability to resume after context compaction.

Scores are comparative. A later round should not regress without an explicit
reason in `version.md`.

## version.md Format

Each round appends:

```markdown
## Round N - YYYY-MM-DD HH:mm

- Source commit before:
- Source commit after:
- Remote branch:
- Test trace:
- Installed skill path:
- E2E run dir:
- Final report:
- Scores:
  - UX:
  - Tool correctness:
  - Flow control:
  - Performance:
  - Evidence quality:
  - Recoverability:
- Fixed:
- Found:
- Deferred:
- Hard failures:
- Next round candidates:
- Context handoff:
- Submission gate:
```

The entry must be written before committing the round. A successful round is not
complete until `D:\work\self_improved\test\evolution-tools\assert-round-submission.ps1 -Round N`
confirms that the repository is clean, the round commit exists, and the remote
branch points at the same commit.

## Stop Conditions

The automatic loop must stop and ask the user when any of these occur:

- The E2E runner cannot complete all eight stages.
- A flow auditor finds a direct trace processor SQL call.
- A flow auditor finds manual run-state mutation.
- `quick_validate.py` or `skill/verify.ps1` fails.
- The final report lacks evidence for major conclusions.
- Git commit, push, or submission gate fails.
- The same hard failure repeats in two consecutive rounds.
- Two consecutive rounds show no score improvement and no fixed hard issue.
- The next change requires a product judgment rather than an engineering fix.
- The implementation scope would require touching overlapping files from multiple
  active workers.

## Git and Remote Policy

- Each round ends in exactly one source commit unless a hard failure prevents a
  valid round.
- Commit messages use:
  `evolution: round N <short outcome>`
- The commit includes `version.md`, source changes, and docs. External helper
  scripts under `D:\work\self_improved\test\evolution-tools\` are not committed.
- Large test artifacts under `test/evolution/` are not committed unless the user
  explicitly asks. Their paths are recorded in `version.md`.
- Push targets the current branch unless the user provides a different branch.
- After push, run
  `D:\work\self_improved\test\evolution-tools\assert-round-submission.ps1 -Round N`.
  If it does not return `ok=true`, the round is incomplete and the loop must
  stop before starting another round.
- If the worktree contains unrelated user changes, the orchestrator stages only
  files belonging to the current round.

## Agent Prompt Templates

### Implementation Worker

```text
Use Superpowers workflows to implement the assigned improvement in
D:\work\self_improved\kat-rs.

Ownership scope: <files or directories>.
Do not revert or overwrite unrelated changes. Other agents may be working in the
same repository.

Use the appropriate Superpowers workflow for the task. Before reporting success,
run relevant verification. Return changed files, workflow used, verification
commands, results, and residual risks.
```

### E2E Runner

```text
Use the skill at <round-skill-path> to analyze
D:\work\self_improved\test\test.htrace for a cold-start issue.

Write all current-round logs and artifacts under <round-e2e-dir>.
Complete the eight skill stages in order. Use htrace run go/guard/validate/advance
as required by the skill. Do not read previous round reports. Do not call
trace_processor_shell directly. Return stage completion status, artifact paths,
final report path, failures, and observations.
```

### Flow Auditor

```text
Review <round-e2e-dir> for workflow compliance. Check command logs, run-state,
progress, and artifacts. Report hard violations first. Do not suggest product
improvements unless they affect workflow correctness.
```

### Report Reviewer

```text
Review <final-report> and supporting artifacts. Score evidence quality, clarity,
uncertainty handling, and user experience. Identify unsupported claims and
actionable improvements.
```

### Performance Reviewer

```text
Review <round-e2e-dir> for performance. Record E2E runtime, atomic runtimes when
available, output size, trace processor usage, repeated work, and optimization
candidates.
```

## Compaction Strategy

After each round:

- `version.md` captures the durable history.
- `round-N/handoff.md` captures the next-round starting point.
- All subagents are closed.
- If an explicit context-compaction mechanism is available, use it.
- If no mechanism is available, the next round must begin by reading only
  `version.md`, the latest handoff, git status, and current files.

This prevents compaction from occurring during the critical E2E workflow.

## Open Risks

- The local environment may lack `cargo`, so Rust test execution may be blocked
  until a toolchain is installed or bundled.
- Full E2E execution on a large trace may be slow; the performance reviewer should
  identify whether CLI or skill changes can reduce repeated trace loading.
- `test/evolution/` artifacts may become large. The loop should avoid committing
  them by default.
- Multi-agent implementation can conflict if ownership scopes are not strict.
- A single `test.htrace` can overfit the skill. Later evolution should add more
  traces or synthetic replay fixtures.

## Acceptance Criteria

The design is ready for implementation when:

- The orchestrator can run a fixed number of rounds without relying on chat
  memory.
- Every round installs the current skill into an isolated test directory.
- Every round performs a fresh eight-stage E2E validation.
- Every round receives independent flow, report, and performance reviews.
- Implementation workers use Superpowers workflows and report verification
  evidence.
- `version.md` records every round.
- Successful rounds are committed and pushed.
- Stop conditions prevent silent bad evolution.
