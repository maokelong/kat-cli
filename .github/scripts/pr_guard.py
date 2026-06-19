#!/usr/bin/env python3
"""Guard pull requests against unreviewable or policy-breaking changes."""

from __future__ import annotations

import argparse
import json
import os
import re
import subprocess
import sys
from dataclasses import dataclass, field
from pathlib import Path
from typing import Iterable


LARGE_CHANGE_LABEL = "approved-large-change"
LARGE_FIXTURE_LABEL = "approved-large-fixture"
BINARY_LABEL = "approved-binary-artifact"
NO_ISSUE_LABEL = "no-issue-needed"

COMMIT_IDENTITY_SEPARATOR = "\x1f"

MAX_CHANGED_FILES = 30
MAX_ADDITIONS = 1600
MAX_TOTAL_DIFF = 2400
MAX_SOURCE_FILE_ADDITIONS = 800
WARN_DELETIONS = 1000
WARN_DOC_ADDITIONS = 800
MAX_FIXTURE_FILE_BYTES = 100 * 1024
MAX_FIXTURE_TOTAL_BYTES = 300 * 1024

DOC_EXTENSIONS = {
    ".adoc",
    ".md",
    ".mdx",
    ".org",
    ".rst",
    ".txt",
}

SOURCE_EXTENSIONS = {
    ".c",
    ".cc",
    ".cpp",
    ".cs",
    ".go",
    ".h",
    ".hpp",
    ".java",
    ".js",
    ".jsx",
    ".kt",
    ".php",
    ".ps1",
    ".py",
    ".rb",
    ".rs",
    ".sh",
    ".swift",
    ".ts",
    ".tsx",
}

FIXTURE_COMPONENTS = {
    "__fixtures__",
    "fixture",
    "fixtures",
    "golden",
    "sample",
    "samples",
    "snapshot",
    "snapshots",
    "test_data",
    "testdata",
}

TEMP_COMPONENTS = {
    ".cache",
    ".tmp",
    "dump",
    "dumps",
    "generated",
    "generated-dump",
    "generated-dumps",
    "probe",
    "probes",
    "scratch",
    "temp",
    "tmp",
}

AI_COMPONENTS = {
    "ai-output",
    "ai_outputs",
    "ai_generated",
    "ai-generated",
    "chatgpt-export",
    "claude-output",
    "claude-export",
    "codex-output",
    "conversation-transcript",
    "llm-output",
    "llm_outputs",
}

AI_AGENT_IDENTITY_PATTERN = re.compile(
    r"(^|[^a-z0-9])"
    r"(aider|chatgpt|claude|codex|copilot|cursor|devin|doubao|gemini|kimi|qwen|trae|windsurf)"
    r"([^a-z0-9]|$)",
    re.IGNORECASE,
)

REQUIRED_WORKFLOW_SIGNALS = (
    "pull_request",
    "cargo check",
    "cargo test",
    "pr guard tests",
    "pr guard",
    "commit identity guard",
)


@dataclass(frozen=True)
class ChangedFile:
    path: str
    status: str = "M"
    old_path: str | None = None
    additions: int = 0
    deletions: int = 0
    binary: bool = False


@dataclass(frozen=True)
class CommitIdentity:
    sha: str
    subject: str
    author_name: str
    author_email: str
    committer_name: str
    committer_email: str


@dataclass(frozen=True)
class PullRequestContext:
    labels: set[str] = field(default_factory=set)
    body: str = ""
    base: str | None = None
    head: str | None = None


@dataclass
class Evaluation:
    failures: list[str] = field(default_factory=list)
    warnings: list[str] = field(default_factory=list)
    metrics: dict[str, int] = field(default_factory=dict)
    changed_files: list[ChangedFile] = field(default_factory=list)

    @property
    def ok(self) -> bool:
        return not self.failures


def run_git(args: list[str], repo: Path, check: bool = True) -> str:
    result = subprocess.run(
        ["git", *args],
        cwd=str(repo),
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        check=False,
    )
    if check and result.returncode != 0:
        raise RuntimeError(
            "git {} failed with exit {}:\n{}".format(
                " ".join(args), result.returncode, result.stderr.strip()
            )
        )
    return result.stdout


def parse_numstat(text: str) -> dict[str, tuple[int, int, bool]]:
    files: dict[str, tuple[int, int, bool]] = {}
    for raw_line in text.splitlines():
        line = raw_line.rstrip("\n")
        if not line:
            continue
        parts = line.split("\t")
        if len(parts) < 3:
            continue
        added, deleted, path = parts[0], parts[1], parts[-1]
        binary = added == "-" or deleted == "-"
        additions = 0 if binary else int(added)
        deletions = 0 if binary else int(deleted)
        files[path] = (additions, deletions, binary)
    return files


def parse_name_status(text: str) -> list[ChangedFile]:
    files: list[ChangedFile] = []
    for raw_line in text.splitlines():
        line = raw_line.rstrip("\n")
        if not line:
            continue
        parts = line.split("\t")
        status = parts[0]
        code = status[:1]
        if code in {"R", "C"} and len(parts) >= 3:
            files.append(ChangedFile(path=parts[2], status=code, old_path=parts[1]))
        elif len(parts) >= 2:
            files.append(ChangedFile(path=parts[1], status=code))
    return files


def merge_diff_data(
    name_status: Iterable[ChangedFile],
    numstat: dict[str, tuple[int, int, bool]],
) -> list[ChangedFile]:
    files: list[ChangedFile] = []
    seen: set[str] = set()
    for item in name_status:
        additions, deletions, binary = numstat.get(item.path, (0, 0, False))
        files.append(
            ChangedFile(
                path=item.path,
                status=item.status,
                old_path=item.old_path,
                additions=additions,
                deletions=deletions,
                binary=binary,
            )
        )
        seen.add(item.path)
    for path, (additions, deletions, binary) in numstat.items():
        if path in seen:
            continue
        files.append(
            ChangedFile(
                path=path,
                additions=additions,
                deletions=deletions,
                binary=binary,
            )
        )
    return files


def collect_changed_files(base: str, head: str, repo: Path) -> list[ChangedFile]:
    diff_range = f"{base}...{head}"
    name_status = parse_name_status(
        run_git(["diff", "--name-status", "--no-renames", diff_range], repo)
    )
    numstat = parse_numstat(run_git(["diff", "--numstat", "--no-renames", diff_range], repo))
    return merge_diff_data(name_status, numstat)


def is_zero_sha(ref: str) -> bool:
    return bool(re.fullmatch(r"0+", ref))


def parse_commit_identities(text: str) -> list[CommitIdentity]:
    commits: list[CommitIdentity] = []
    for raw_line in text.splitlines():
        line = raw_line.rstrip("\n")
        if not line:
            continue
        parts = line.split(COMMIT_IDENTITY_SEPARATOR, 5)
        if len(parts) != 6:
            continue
        sha, author_name, author_email, committer_name, committer_email, subject = parts
        commits.append(
            CommitIdentity(
                sha=sha,
                subject=subject,
                author_name=author_name,
                author_email=author_email,
                committer_name=committer_name,
                committer_email=committer_email,
            )
        )
    return commits


def collect_commit_identities(base: str, head: str, repo: Path) -> list[CommitIdentity]:
    commit_format = (
        f"%H%x1f%an%x1f%ae%x1f%cn%x1f%ce%x1f%s"
    )
    rev_range = head if is_zero_sha(base) else f"{base}..{head}"
    output = run_git(["log", f"--format={commit_format}", rev_range], repo)
    return parse_commit_identities(output)


def parse_event(path: str | None) -> PullRequestContext:
    if not path:
        return PullRequestContext()
    event_path = Path(path)
    if not event_path.exists():
        return PullRequestContext()

    data = json.loads(event_path.read_text(encoding="utf-8"))
    pr = data.get("pull_request") or {}
    labels = {
        str(label.get("name", "")).strip()
        for label in pr.get("labels", [])
        if isinstance(label, dict) and label.get("name")
    }
    body = pr.get("body") or ""
    base = (pr.get("base") or {}).get("sha") or (pr.get("base") or {}).get("ref")
    head = (pr.get("head") or {}).get("sha") or (pr.get("head") or {}).get("ref")
    return PullRequestContext(labels=labels, body=body, base=base, head=head)


def has_label(labels: set[str], label: str) -> bool:
    return label in {item.lower() for item in labels}


def has_linked_issue(body: str) -> bool:
    pattern = re.compile(
        r"(?im)\b(close|closes|closed|fix|fixes|fixed|resolve|resolves|resolved)\s+"
        r"((?:[\w.-]+/[\w.-]+)?#\d+|https://github\.com/[\w.-]+/[\w.-]+/issues/\d+)\b"
    )
    return bool(pattern.search(body or ""))


def path_components(path: str) -> list[str]:
    return [part.lower() for part in Path(path).parts]


def is_source_file(path: str) -> bool:
    return Path(path).suffix.lower() in SOURCE_EXTENSIONS


def is_doc_file(path: str) -> bool:
    return Path(path).suffix.lower() in DOC_EXTENSIONS


def is_fixture_file(path: str) -> bool:
    parts = path_components(path)
    if any(part in FIXTURE_COMPONENTS for part in parts):
        return True
    return Path(path).suffix.lower() in {".snap", ".snapshot"}


def is_forbidden_intermediate_path(path: str) -> bool:
    parts = path_components(path)
    if any(part in TEMP_COMPONENTS or part in AI_COMPONENTS for part in parts):
        return True
    name = Path(path).name.lower()
    return bool(
        re.search(r"(^|[-_.])(dump|generated|generated-dump|probe|scratch|tmp|temp)([-_.]|$)", name)
        or re.search(r"(^|[-_.])(chatgpt-export|claude-output|codex-output|llm-output|ai-generated|ai_output)([-_.]|$)", name)
    )


def is_workflow_file(path: str) -> bool:
    normalized = path.replace("\\", "/").lower()
    return (
        normalized.startswith(".github/workflows/")
        and Path(normalized).suffix in {".yml", ".yaml"}
    )


def strip_line_comments(text: str) -> str:
    stripped: list[str] = []
    for line in text.splitlines():
        stripped.append(line.split("#", 1)[0])
    return "\n".join(stripped)


def significant_lines(text: str) -> list[tuple[int, str]]:
    lines: list[tuple[int, str]] = []
    for raw in strip_line_comments(text).splitlines():
        if not raw.strip():
            continue
        indent = len(raw) - len(raw.lstrip(" "))
        lines.append((indent, raw.strip().lower()))
    return lines


def has_pull_request_trigger(text: str) -> bool:
    lines = significant_lines(text)
    for index, (indent, line) in enumerate(lines):
        if re.match(r"^on\s*:\s*pull_request\s*$", line):
            return True
        if re.match(r"^on\s*:\s*\[.*\bpull_request\b.*\]\s*$", line):
            return True
        if re.match(r"^on\s*:\s*$", line):
            for child_indent, child in lines[index + 1 :]:
                if child_indent <= indent:
                    break
                if child_indent == indent + 2 and re.match(r"^pull_request\s*:", child):
                    return True
    return False


def disabled_job_patterns(text: str) -> list[str]:
    lines = significant_lines(text)
    findings: list[str] = []
    in_jobs = False
    jobs_indent = -1
    current_job: str | None = None
    current_job_indent = -1
    for indent, line in lines:
        if re.match(r"^jobs\s*:\s*$", line):
            in_jobs = True
            jobs_indent = indent
            current_job = None
            current_job_indent = -1
            continue
        if in_jobs and indent <= jobs_indent:
            in_jobs = False
            current_job = None
            current_job_indent = -1
        if not in_jobs:
            continue
        if indent == jobs_indent + 2 and re.match(r"^[a-z0-9_.-]+\s*:\s*$", line):
            current_job = line.split(":", 1)[0]
            current_job_indent = indent
            continue
        if (
            current_job
            and indent == current_job_indent + 2
            and re.match(r"^if\s*:\s*(?:false|\$\{\{\s*false\s*\}\})\s*$", line)
        ):
            findings.append(f"job:{current_job}:if:false")
    return findings


def workflow_disabled_patterns(text: str) -> list[str]:
    findings: list[str] = []
    findings.extend(disabled_job_patterns(text))
    content = strip_line_comments(text).lower()
    for key in ("branches-ignore", "paths-ignore"):
        if re.search(rf"(?m)^\s*{key}\s*:\s*\[\s*['\"]?\*\*['\"]?\s*\]\s*$", content):
            findings.append(f"{key}:**")
        if re.search(rf"(?ms)^\s*{key}\s*:\s*\n(?:\s+-\s*['\"]?\*\*['\"]?\s*\n?)+", content):
            findings.append(f"{key}:**")
    return findings


def contains_required_workflow_signal(text: str, signal: str) -> bool:
    content = strip_line_comments(text).lower()
    if signal == "pull_request":
        return has_pull_request_trigger(content)
    if signal == "cargo check":
        return bool(re.search(r"\bcargo\s+check\b", content))
    if signal == "cargo test":
        return bool(re.search(r"\bcargo\s+test\b", content))
    if signal == "pr guard tests":
        return bool(re.search(r"\bpython(?:3)?\s+\.github/scripts/test_pr_guard\.py\b", content))
    if signal == "pr guard":
        return any(
            re.search(r"\bpython(?:3)?\s+\.github/scripts/pr_guard\.py\b", line)
            and "--identity-only" not in line
            for line in content.splitlines()
        )
    if signal == "commit identity guard":
        return bool(
            re.search(
                r"\bpython(?:3)?\s+\.github/scripts/pr_guard\.py\b[^\n]*\s--identity-only\b",
                content,
            )
        )
    return signal in content


def workflow_content_preserves_signal(
    old_content: str,
    new_content: str,
    signal: str,
) -> bool:
    if not contains_required_workflow_signal(new_content, signal):
        return False
    if signal != "pull_request" and contains_required_workflow_signal(old_content, "pull_request"):
        return contains_required_workflow_signal(new_content, "pull_request")
    return True


def workflow_signal_failures(
    path: str,
    old_content: str,
    new_content: str,
    other_new_workflow_contents: Iterable[str] = (),
) -> list[str]:
    failures: list[str] = []
    replacement_contents = [new_content, *other_new_workflow_contents]
    for signal in REQUIRED_WORKFLOW_SIGNALS:
        if not contains_required_workflow_signal(old_content, signal):
            continue
        if any(
            workflow_content_preserves_signal(old_content, content, signal)
            for content in replacement_contents
        ):
            continue
        failures.append(f"CI workflow `{path}` weakens required signal `{signal}`.")
    return failures


def git_show(repo: Path, ref: str, path: str) -> str | None:
    result = subprocess.run(
        ["git", "show", f"{ref}:{path}"],
        cwd=str(repo),
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        check=False,
    )
    if result.returncode != 0:
        return None
    return result.stdout


def git_blob_size(repo: Path, ref: str, path: str) -> int:
    result = subprocess.run(
        ["git", "cat-file", "-s", f"{ref}:{path}"],
        cwd=str(repo),
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        check=False,
    )
    if result.returncode != 0:
        return 0
    try:
        return int(result.stdout.strip())
    except ValueError:
        return 0


def is_ai_agent_identity(name: str, email: str) -> bool:
    return bool(AI_AGENT_IDENTITY_PATTERN.search(f"{name} {email}"))


def format_identity_violation(commit: CommitIdentity, role: str) -> str:
    if role == "author":
        name = commit.author_name
        email = commit.author_email
    else:
        name = commit.committer_name
        email = commit.committer_email
    return f"{commit.sha[:7]} {role} {name} <{email}>"


def find_ai_agent_commit_identities(commit_identities: Iterable[CommitIdentity]) -> list[str]:
    violations: list[str] = []
    for commit in commit_identities:
        if is_ai_agent_identity(commit.author_name, commit.author_email):
            violations.append(format_identity_violation(commit, "author"))
        if is_ai_agent_identity(commit.committer_name, commit.committer_email):
            violations.append(format_identity_violation(commit, "committer"))
    return violations


def evaluate_commit_identities(commit_identities: list[CommitIdentity]) -> tuple[list[str], int]:
    violations = find_ai_agent_commit_identities(commit_identities)
    if not violations:
        return [], 0
    return [
        "AI agent commit identities are not allowed. Rewrite the commits so both author "
        "and committer identify a human responsible for the change: "
        + "; ".join(violations)
        + "."
    ], len(violations)


def evaluate(
    context: PullRequestContext,
    changed_files: list[ChangedFile],
    repo: Path | None = None,
    commit_identities: list[CommitIdentity] | None = None,
) -> Evaluation:
    labels = {label.lower() for label in context.labels}
    failures: list[str] = []
    warnings: list[str] = []

    additions = sum(item.additions for item in changed_files)
    deletions = sum(item.deletions for item in changed_files)
    total_diff = additions + deletions
    binary_files = [item.path for item in changed_files if item.binary]
    changed_count = len(changed_files)

    if commit_identities is None and repo and context.base and context.head:
        commit_identities = collect_commit_identities(context.base, context.head, repo)
    commit_identities = commit_identities or []
    identity_failures, identity_violation_count = evaluate_commit_identities(commit_identities)
    failures.extend(identity_failures)

    if not has_label(labels, NO_ISSUE_LABEL) and not has_linked_issue(context.body):
        failures.append(
            f"Missing linked issue. Add a closing keyword such as `Closes #123` or apply `{NO_ISSUE_LABEL}`."
        )

    if not has_label(labels, LARGE_CHANGE_LABEL):
        if changed_count > MAX_CHANGED_FILES:
            failures.append(
                f"Changed files ({changed_count}) exceed limit ({MAX_CHANGED_FILES}). Apply `{LARGE_CHANGE_LABEL}` if approved."
            )
        if additions > MAX_ADDITIONS:
            failures.append(
                f"Additions ({additions}) exceed limit ({MAX_ADDITIONS}). Apply `{LARGE_CHANGE_LABEL}` if approved."
            )
        if total_diff > MAX_TOTAL_DIFF:
            failures.append(
                f"Total diff ({total_diff}) exceeds limit ({MAX_TOTAL_DIFF}). Apply `{LARGE_CHANGE_LABEL}` if approved."
            )
        for item in changed_files:
            if is_source_file(item.path) and item.additions > MAX_SOURCE_FILE_ADDITIONS:
                failures.append(
                    f"Source file `{item.path}` has {item.additions} additions, exceeding {MAX_SOURCE_FILE_ADDITIONS}. Apply `{LARGE_CHANGE_LABEL}` if approved."
                )

    if deletions > WARN_DELETIONS:
        warnings.append(f"Deletions ({deletions}) exceed warning threshold ({WARN_DELETIONS}).")

    for item in changed_files:
        if is_doc_file(item.path) and item.additions > WARN_DOC_ADDITIONS:
            warnings.append(
                f"Document `{item.path}` has {item.additions} additions, exceeding warning threshold {WARN_DOC_ADDITIONS}."
            )

    if binary_files and not has_label(labels, BINARY_LABEL):
        failures.append(
            "Binary files are not allowed without approval: "
            + ", ".join(f"`{path}`" for path in binary_files)
            + f". Apply `{BINARY_LABEL}` if approved."
        )

    forbidden_paths = [item.path for item in changed_files if is_forbidden_intermediate_path(item.path)]
    if forbidden_paths:
        failures.append(
            "Temporary, probe, or AI intermediate paths are not allowed: "
            + ", ".join(f"`{path}`" for path in forbidden_paths)
            + "."
        )

    if repo and context.head:
        fixture_files = [item for item in changed_files if is_fixture_file(item.path) and item.status != "D"]
        fixture_sizes = {
            item.path: git_blob_size(repo, context.head, item.path) for item in fixture_files
        }
        oversized = [
            (path, size)
            for path, size in fixture_sizes.items()
            if size > MAX_FIXTURE_FILE_BYTES
        ]
        fixture_total = sum(fixture_sizes.values())
        if not has_label(labels, LARGE_FIXTURE_LABEL):
            if oversized:
                failures.append(
                    "Fixture files exceed 100KB: "
                    + ", ".join(f"`{path}` ({size} bytes)" for path, size in oversized)
                    + f". Apply `{LARGE_FIXTURE_LABEL}` if approved."
                )
            if fixture_total > MAX_FIXTURE_TOTAL_BYTES:
                failures.append(
                    f"Fixture files total {fixture_total} bytes, exceeding {MAX_FIXTURE_TOTAL_BYTES}. Apply `{LARGE_FIXTURE_LABEL}` if approved."
                )

    workflow_failures = evaluate_workflows(context, changed_files, repo)
    failures.extend(workflow_failures)

    return Evaluation(
        failures=failures,
        warnings=warnings,
        metrics={
            "changed_files": changed_count,
            "additions": additions,
            "deletions": deletions,
            "total_diff": total_diff,
            "binary_files": len(binary_files),
            "commits_checked": len(commit_identities),
            "ai_agent_identity_hits": identity_violation_count,
        },
        changed_files=changed_files,
    )


def evaluate_workflows(
    context: PullRequestContext,
    changed_files: list[ChangedFile],
    repo: Path | None,
) -> list[str]:
    if not repo or not context.base or not context.head:
        return []

    failures: list[str] = []
    new_workflow_contents: dict[str, str] = {}
    for item in changed_files:
        if not is_workflow_file(item.path) or item.status == "D":
            continue
        new_content = git_show(repo, context.head, item.path)
        if new_content is not None:
            new_workflow_contents[item.path] = new_content

    for item in changed_files:
        old_workflow = is_workflow_file(item.old_path or item.path)
        new_workflow = is_workflow_file(item.path)
        if not old_workflow and not new_workflow:
            continue

        if old_workflow and item.status in {"D", "R"} and not new_workflow:
            failures.append(f"CI workflow `{item.old_path or item.path}` was deleted or moved out of workflows.")
            continue

        if item.status == "A":
            continue

        old_path = item.old_path or item.path
        old_content = git_show(repo, context.base, old_path)
        new_content = new_workflow_contents.get(item.path)
        if old_content is None:
            continue
        if new_content is None:
            failures.append(f"CI workflow `{old_path}` was removed.")
            continue

        disabled = workflow_disabled_patterns(new_content)
        if disabled:
            failures.append(f"CI workflow `{item.path}` appears disabled by {', '.join(disabled)}.")

        other_new_contents = [
            content for path, content in new_workflow_contents.items() if path != item.path
        ]
        failures.extend(
            workflow_signal_failures(item.path, old_content, new_content, other_new_contents)
        )

    return failures


def render_summary(evaluation: Evaluation) -> str:
    status = "PASS" if evaluation.ok else "FAIL"
    lines = [
        "# PR Guard",
        "",
        f"**Status:** {status}",
        "",
        "## Metrics",
        "",
        "| Metric | Value |",
        "| --- | ---: |",
    ]
    for key in (
        "changed_files",
        "additions",
        "deletions",
        "total_diff",
        "binary_files",
        "commits_checked",
        "ai_agent_identity_hits",
    ):
        if key not in evaluation.metrics:
            continue
        lines.append(f"| {key.replace('_', ' ').title()} | {evaluation.metrics.get(key, 0)} |")

    lines.extend(["", "## Failures"])
    if evaluation.failures:
        lines.extend(f"- {failure}" for failure in evaluation.failures)
    else:
        lines.append("- None")

    lines.extend(["", "## Warnings"])
    if evaluation.warnings:
        lines.extend(f"- {warning}" for warning in evaluation.warnings)
    else:
        lines.append("- None")

    lines.extend(["", "## Changed Files"])
    if evaluation.changed_files:
        lines.append("| Status | File | + | - | Binary |")
        lines.append("| --- | --- | ---: | ---: | --- |")
        for item in evaluation.changed_files:
            binary = "yes" if item.binary else "no"
            lines.append(
                f"| {item.status} | `{item.path}` | {item.additions} | {item.deletions} | {binary} |"
            )
    else:
        lines.append("- None")

    return "\n".join(lines) + "\n"


def append_step_summary(markdown: str) -> None:
    print(markdown, end="")
    summary_path = os.environ.get("GITHUB_STEP_SUMMARY")
    if not summary_path:
        return
    with open(summary_path, "a", encoding="utf-8") as handle:
        handle.write(markdown)


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        description="Check PR size, issue binding, artifacts, workflow safety, and commit identities."
    )
    parser.add_argument("--event", default=os.environ.get("GITHUB_EVENT_PATH"), help="Path to GitHub event JSON.")
    parser.add_argument("--base", help="Base git ref or SHA. Overrides event pull_request.base.")
    parser.add_argument("--head", help="Head git ref or SHA. Overrides event pull_request.head.")
    parser.add_argument("--repo", default=".", help="Repository path.")
    parser.add_argument(
        "--identity-only",
        action="store_true",
        help="Only check commit author and committer identities.",
    )
    return parser


def main(argv: list[str] | None = None) -> int:
    args = build_parser().parse_args(argv)
    repo = Path(args.repo).resolve()
    context = parse_event(args.event)
    if args.base:
        context = PullRequestContext(labels=context.labels, body=context.body, base=args.base, head=context.head)
    if args.head:
        context = PullRequestContext(labels=context.labels, body=context.body, base=context.base, head=args.head)

    if not context.base or not context.head:
        raise SystemExit("Both base and head refs are required via event JSON or --base/--head.")

    if args.identity_only:
        commit_identities = collect_commit_identities(context.base, context.head, repo)
        identity_failures, identity_violation_count = evaluate_commit_identities(commit_identities)
        evaluation = Evaluation(
            failures=identity_failures,
            metrics={
                "commits_checked": len(commit_identities),
                "ai_agent_identity_hits": identity_violation_count,
            },
        )
        append_step_summary(render_summary(evaluation))
        return 0 if evaluation.ok else 1

    changed_files = collect_changed_files(context.base, context.head, repo)
    evaluation = evaluate(context, changed_files, repo)
    append_step_summary(render_summary(evaluation))
    return 0 if evaluation.ok else 1


if __name__ == "__main__":
    raise SystemExit(main())
