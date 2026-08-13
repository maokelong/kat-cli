# 发布候选演练

本手册证明 KAT 的 tag-only 发布拓扑可以由后续维护者重复执行。日常 PR 只验证临时
artifact；改动 release tag、host、announce、finalizer、公开资产集合或发布通道时，必须把
代码作为 prerelease RC 合入 `main`，再用同一份 `dist` 生成 workflow 完成一次真实演练。
这里的“可重复”指流程、拓扑和合同可重复，不承诺 runner、外部 action 或构建产物位级复现。

PR 门禁只决定 RC 代码能否进入集成分支；真实 tag 演练与证据评审决定能否关闭交付 Issue、
进入 stable promotion。演练 Release 必须是 prerelease，不得成为 Latest。Windows 证据仍只
覆盖 builder-image candidate；干净 Windows 客户端支持继续由 Issue #143 跟踪。

GitHub 的 Create/Update Release API 会比较目标 commit 与默认分支的 workflow tree；目标
新增或修改 `.github/workflows/**` 时还需要 Workflows write，而 Actions `GITHUB_TOKEN` 无法
获得该权限。因此，修改发布 workflow 的同一个 PR 不能在合并前用现有 token 完成 Release
演练。本手册选择合并 RC 后演练，不引入 PAT、GitHub App、第二套 rehearsal workflow 或手改
生成 workflow。

## 当前 `dist 0.32` 发布适配合同

固定版本的 `dist`（原 cargo-dist）继续拥有版本与 tag、原生目标 runner、local/global
artifact jobs、托管和 GitHub Release。`dist 0.32` 不能发现自定义 global job 产出的 opaque
Skill，因此当前适配只补足两处工具缺口：自定义 global job 装配最终 Skill 与配套 SHA-256，
官方 `post-announce` job 收尾公开资产。job 顺序和发布生命周期不由 KAT 重新实现。

生成的 host job 会收集 `artifacts-*`。因此最终 Skill artifact 保留此前缀；只服务装配的
`kat-workflow-wheel`、`kat-linux-payload` 与 `kat-windows-payload` 必须避开此前缀。
`checksum = "false"` 只阻止 `dist` 为私有原生 payload 规划公开校验文件，不改变最终 Skill
与 SHA-256 必须由同一个 global job 产出的合同。

`post-announce` finalizer 只接受两种资产状态：

- 发布前态：`dist-manifest.json`、Skill 与 SHA-256；核对 tag、commit、prerelease 状态、
  下载后 SHA-256 和完整集合后删除 manifest。
- 完成态：只有 Skill 与 SHA-256；重跑时重新核对并直接成功，不删除或重建 Release。

其他资产集合一律失败。finalizer 已启动后的失败可以把 Release 撤回为 draft prerelease；tag
一经 push 永久消耗，不移动、不复用。manifest 在 finalizer 前会短暂公开，所以其中不得包含
秘密。当前拓扑不支持 GitHub Immutable Releases；启用该策略前必须先迁移到成熟工具提供的
pre-publish 生命周期接缝。

### `extra-artifacts` probe 与退出条件

本适配评估过 `dist 0.32` 的 package-local `extra-artifacts`。计划级 probe 在
`release/kat/dist.toml` 临时加入：

```toml
[[dist.extra-artifacts]]
artifacts = [
    "../../target/distrib/release/kat-skill-<release-version>.tar.gz",
    "../../target/distrib/release/kat-skill-<release-version>.tar.gz.sha256",
]
build = ["python", "-c", "raise SystemExit('plan-only probe')"]
```

`dist plan --output-format=json` 仍同时规划两个 generic 原生归档；`dist generate` 仍让 built-in
global job 与 custom payload job 并列，host 等待两者。把 `binaries` 临时改成 `[]` 会得到
`This workspace doesn't have anything for dist to Release!`；恢复后把 targets 临时改成 `[]`
会得到 `specified no targets to build!`。因此不能关闭 generic binary/target 计划来只保留
extra artifacts。

构建级 probe 只证明 `dist build --artifacts=global` 会登记和复制 helper 生成的文件；
`ExtraArtifact` 不关联 checksum，托管阶段仍会公开无法描述最终 opaque Skill 的
`dist-manifest.json`。这些结果是固定 `dist 0.32` 期间采用 custom global job 与 finalizer 的
适用依据，不是长期架构原则。当稳定版 `dist` 能声明并校验自定义 global artifact、配套
checksum，并提供公开前完成资产校验的生命周期接缝时，应删除本节适配，而不是增加第二套
发布编排。

## 状态与职责

每个 RC 独立记录状态，不得用新 RC 覆盖旧记录：

1. `PRE-MERGE VERIFIED`：本地校验、PR checks 和代码型 review 已通过。
2. `MERGED / COMMIT VERIFIED`：merge commit `C` 的父提交和 tree 已与 PR 候选核对；Issue
   保持打开。
3. `TAGGED / RUN ACTIVE`：唯一 canonical prerelease tag 已指向 `C`，发布操作者正在值守。
4. `REHEARSAL SUCCEEDED`：真实 tag run、Release 和 finalizer 完成态重跑均成功。
5. `EVIDENCE ACCEPTED`：证据型 `kat-pr-review` 无未处理的 P0、P1 或 P2，允许关闭交付
   Issue，并允许独立 stable promotion PR。

合并身份不符时标记 `REPAIR REQUIRED`；tag run 或资产验证失败时标记 `WITHDRAWN`；Release
无法确认隔离时标记 `ISOLATION UNCONFIRMED`。这些状态都禁止关闭 Issue 和 stable promotion。

需要三个明确责任人：

- 发布操作者：冻结候选、推送 tag、值守 run、采集证据和执行隔离。
- 仓库 owner：在 merge 身份核对期间冻结 main，并从 tag 前检查到 finalizer 重跑或失败隔离
  完成期间冻结默认分支的 `.github/workflows/**` tree 与 Immutable Releases 策略。
- 证据 reviewer：只使用已合并候选、作者证据和可核验 API 状态完成 `kat-pr-review`。

## 固定输入和证据模板

在 Issue 的当前 RC 独立小节填写以下字段；没有取值时保留为空，不得猜测：

```text
REPO=maokelong/kat-cli
PR=
ISSUE=
VERSION=
WHEEL_VERSION=
TAG=kat/<VERSION>

BASE_SHA=B=
HEAD_SHA=H=
PR_MERGE_SHA=M=
PR_ACTUAL_CHECKOUT_SHA=A=
PR_ACTUAL_CHECKOUT_TREE=T=
PR_RUN_ID=
PR_RUN_ATTEMPT=
MERGE_COMMIT_SHA=C=
DEFAULT_BRANCH_SHA_AT_TAG=D=

TAG_RUN_ID=
TAG_RUN_ATTEMPT=
RELEASE_ID=
FINALIZER_JOB_DATABASE_ID=

WORKFLOW_TREE_FREEZE_OWNER=
WORKFLOW_TREE_FREEZE_START=
WORKFLOW_TREE_FREEZE_END=
IMMUTABLE_POLICY_FREEZE_START=
IMMUTABLE_POLICY_FREEZE_END=
LATEST_BEFORE_HTTP=
LATEST_BEFORE_ID=
LATEST_BEFORE_TAG=
LATEST_AFTER_HTTP=
LATEST_AFTER_ID=
LATEST_AFTER_TAG=
```

本次第一个候选使用：

```text
VERSION=0.1.1-rc.1
WHEEL_VERSION=0.1.1rc1
TAG=kat/0.1.1-rc.1
```

长期证据写入 Issue 纯文本。Release/tag 保留；Actions 日志和临时 artifact 只作保留期内的
辅助证据。

## 1. PRE-MERGE VERIFIED

先完成本地轻量校验，不在本地构建或下载 Platform Payload：

```bash
python -I -B build/verify_release_versions.py --tag "$TAG"
python -I -B -m unittest discover -s build/tests -p "test_release_versions.py"
python -I -B -m unittest discover -s build/tests -p "test_ci_artifact_lifecycle.py"
python -I -B -m unittest discover -s build/tests -p "test_workflow_wheel.py"
cargo metadata --locked --no-deps --format-version 1
dist generate --check
```

提交候选后只运行正常 PR 自动门禁。PR 事件中的 host、announce 和 finalizer 必须 skipped；
build、assemble、双资产校验、Linux 完整闭环和 Windows builder-image candidate smoke 必须
成功。保存 `Release` run ID、attempt 和完整 jobs JSON。

`build-payloads-ci.yml` 的 release-channel job 会打印唯一 checkout SHA 和 tree。先从指定
attempt 的日志提取，随后用 tree 对照冻结的 PR merge ref `M`；不能把 PR API 的 head SHA
当成实际 checkout：

```bash
log="$(gh run view "$PR_RUN_ID" --repo "$REPO" --attempt "$PR_RUN_ATTEMPT" --log)"
sha_matches="$(grep -o 'KAT_RELEASE_CHECKOUT_SHA=[0-9a-f]\{40\}' <<< "$log" | sort -u)"
tree_matches="$(grep -o 'KAT_RELEASE_CHECKOUT_TREE=[0-9a-f]\{40\}' <<< "$log" | sort -u)"
test "$(wc -l <<< "$sha_matches")" -eq 1
test "$(wc -l <<< "$tree_matches")" -eq 1
A="${sha_matches#KAT_RELEASE_CHECKOUT_SHA=}"
T="${tree_matches#KAT_RELEASE_CHECKOUT_TREE=}"
```

PR 正文必须使用 `Refs #<issue>`，不能在真实演练和证据评审之前自动关闭交付 Issue；PR
Guard 同时接受非关闭式 `Refs` 和普通 closing keyword。请求最终 PR 门禁前，先把交付 Issue
中的旧“合并自动关闭”说明改成“`EVIDENCE ACCEPTED` 后人工关闭”，再执行：

```bash
pr_body="$(gh pr view "$PR" --repo "$REPO" --json body --jq .body)"
grep -Eiq "(^|[[:space:]])Refs[[:space:]]+#${ISSUE}([^0-9]|$)" <<< "$pr_body"
! grep -Eiq \
  "(^|[[:space:]])(close|closes|closed|fix|fixes|fixed|resolve|resolves|resolved)[[:space:]]+#${ISSUE}([^0-9]|$)" \
  <<< "$pr_body"
test "$(gh issue view "$ISSUE" --repo "$REPO" --json state --jq .state)" = "OPEN"
```

## 2. 冻结、合并并核对 commit

以下命令以 Bash 表示；变量取值必须写入证据模板：

```bash
REPO=maokelong/kat-cli
PR=<number>
ISSUE=<number>
VERSION=<SemVer-prerelease>
WHEEL_VERSION=<PEP-440-version>
TAG="kat/$VERSION"

B="$(gh pr view "$PR" --repo "$REPO" --json baseRefOid --jq .baseRefOid)"
H="$(gh pr view "$PR" --repo "$REPO" --json headRefOid --jq .headRefOid)"
git fetch origin "+pull/$PR/merge:refs/kat-rehearsal/pr-$PR-merge"
M="$(git rev-parse "refs/kat-rehearsal/pr-$PR-merge")"

test "$T" = "$(git rev-parse "$M^{tree}")"
test "$(git ls-remote origin refs/heads/main | awk '{print $1}')" = "$B"
test "$(gh pr view "$PR" --repo "$REPO" --json headRefOid --jq .headRefOid)" = "$H"

gh pr merge "$PR" --repo "$REPO" --merge --match-head-commit "$H"
C="$(gh pr view "$PR" --repo "$REPO" --json mergeCommit --jq .mergeCommit.oid)"
git fetch --no-tags origin main

test "$(git rev-parse "$C^1")" = "$B"
test "$(git rev-parse "$C^2")" = "$H"
test "$(git rev-parse "$C^{tree}")" = "$(git rev-parse "$M^{tree}")"
```

只有父提交正确且 `tree(C) == tree(M)` 时进入 `MERGED / COMMIT VERIFIED`。不满足时标记
`REPAIR REQUIRED`，不打 tag。合并后交付 Issue 必须仍为 open；`main` 中的 prerelease RC
不是 stable 交付。

## 3. Tag 前硬门禁

从 `C` 的干净 checkout 执行以下检查。先把 `dist plan` 保存为证据，再机器断言唯一版本、
精确 tag 和 prerelease 状态；只打印 JSON 不算门禁：

```bash
test "$(git rev-parse HEAD)" = "$C"
PLAN_JSON="dist-plan-${VERSION}.json"
dist plan --tag="$TAG" --output-format=json > "$PLAN_JSON"
jq -e --arg tag "$TAG" --arg version "$VERSION" '
  .announcement_tag == $tag
  and .announcement_is_prerelease == true
  and ([.releases[].app_version] | unique) == [$version]
' "$PLAN_JSON" >/dev/null
python -I -B build/verify_release_versions.py --tag "$TAG"
```

stable tag 或 stable plan 一律 STOP。失败后不得用同一版本改成 prerelease；必须提交
`rc.N+1` follow-up PR。

### 默认分支 workflow 与策略

GitHub Release API 的额外 Workflows write 要求必须在不可逆 tag push 前排除。owner 记录
workflow tree 与 Immutable Releases 的冻结窗口：

```bash
git fetch --no-tags origin main
D="$(git rev-parse origin/main)"
git merge-base --is-ancestor "$C" "$D"
git diff --quiet "$C" "$D" -- .github/workflows
gh api --include "repos/$REPO/immutable-releases"
```

`git diff` 非零即 STOP。Immutable Releases 查询唯一允许继续的结果是明确的 HTTP 404；
HTTP 200、403、超时、权限不足、无法分辨 404 来源或其他结果一律 STOP。冻结开始后，直到
成功完成 finalizer 重跑或失败 Release 已隔离，default branch 的 workflow tree 和 Immutable
Releases 策略不得改变；main 的其他文件可以继续前进。

### 身份和 Latest 基线

同时满足：

- 本地和远端 `refs/tags/$TAG` 都不存在；`git ls-remote` 自身必须成功且结果为空。
- 全量 Release 列表（包括 draft）没有同名 `tag_name`。
- 保存 `GET /repos/$REPO/releases/latest` 的 HTTP 状态；200 时保存 numeric ID/tag，404 也
  原样记录。
- `C` 仍是 `main` 的祖先，且 `C` 与当前 default branch 的 workflow tree 仍相同。

```bash
test -z "$(git ls-remote --tags origin "refs/tags/$TAG")"

release_count="$(
  gh api --paginate --slurp "repos/$REPO/releases?per_page=100" |
    jq --arg tag "$TAG" '[.[][] | select(.tag_name == $tag)] | length'
)"
test "$release_count" -eq 0

gh api --include "repos/$REPO/releases/latest"
```

tag/Release 已存在、查询失败或结果不唯一时 STOP，不能删除旧对象后复用身份。

## 4. 创建唯一 lightweight tag

只使用 lightweight tag，不使用 annotated、signed 或 movable tag：

```bash
git -c tag.gpgSign=false tag "$TAG" "$C"
test "$(git cat-file -t "refs/tags/$TAG")" = commit
test "$(git rev-parse "refs/tags/$TAG^{commit}")" = "$C"
git push origin "refs/tags/$TAG:refs/tags/$TAG"
test "$(git ls-remote origin "refs/tags/$TAG" | awk '{print $1}')" = "$C"
```

push 禁止 `--force`。从 push 成功开始，该 tag 永久消耗；失败不得移动、删除后复用或拿整
run rerun 恢复。

首次 workflow 注册可能存在短暂延迟，所以不使用尚未注册的 workflow path 作为查询条件。
在有界轮询内按 event、tag、commit 筛选，再要求 workflow 名和 API path 唯一匹配：

```bash
release_runs='[]'
for _ in $(seq 1 30); do
  runs="$(gh run list \
    --repo "$REPO" \
    --event push \
    --branch "$TAG" \
    --commit "$C" \
    --limit 100 \
    --json attempt,conclusion,createdAt,databaseId,event,headBranch,headSha,status,url,workflowName)"
  release_runs="$(jq '[.[] | select(.workflowName == "Release")]' <<< "$runs")"
  count="$(jq 'length' <<< "$release_runs")"
  test "$count" -le 1
  test "$count" -eq 1 && break
  sleep 10
done
test "$(jq 'length' <<< "$release_runs")" -eq 1
TAG_RUN_ID="$(jq -er '.[0].databaseId' <<< "$release_runs")"

run_state="$(gh api "repos/$REPO/actions/runs/$TAG_RUN_ID")"
jq -e --arg tag "$TAG" --arg sha "$C" '
  .event == "push" and .head_branch == $tag and .head_sha == $sha
' <<< "$run_state" >/dev/null
test "$(jq -r '.path | split("@")[0]' <<< "$run_state")" = ".github/workflows/kat-release.yml"
```

满足以上条件后进入 `TAGGED / RUN ACTIVE`。

## 5. 值守首次 tag run

发布操作者必须从 tag push 值守到 run 终态。以下 job 均须成功且不得意外 skipped：

- plan 和唯一 local-artifact wrapper；
- Workflow Host wheel、requirements locks、Linux/Windows Payload；
- global build、Skill assembly、双资产/SHA-256 校验；
- Linux 完整闭环和 Windows builder-image candidate smoke；
- host、announce、finalizer。

保存 plan-stage manifest 和 host upload manifest 的 `app_version`、`announcement_tag`、
`announcement_is_prerelease`。前者只是 early signal；创建 Release 的参数来自 host 第二份
manifest，最终状态还必须由 Release API独立确认。记录 action SHA、runner image、dist 和关键
工具版本。所有中间 manifest 与 artifact 都可能短暂公开，必须天然可公开且不得含秘密。

## 6. REHEARSAL SUCCEEDED

首次 run 成功后独立验证：

- 远端 lightweight tag、commit API 和 run `headSha` 都解析为 `C`；Release 的
  `targetCommitish` 只作记录，不作为身份权威。
- Release numeric ID 固定，`tagName == TAG`、`draft == false`、`prerelease == true`、
  `immutable == false`。
- 最终资产精确只有 `kat-skill-$VERSION.tar.gz` 和
  `kat-skill-$VERSION.tar.gz.sha256`。
- 保存两项资产的 numeric ID、name、size、API digest、创建/更新时间；下载到新的临时目录并
  运行 `sha256sum -c`，保存实算 SHA-256。
- 私有 wheel 名为 `kat_workflow-$WHEEL_VERSION-py3-none-any.whl`，构建日志中的
  `METADATA Version` 精确等于 `$WHEEL_VERSION`。
- Latest 后置 HTTP 状态、ID 和 tag 与 tag 前基线完全相同。

```bash
test "$(gh api "repos/$REPO/commits/$TAG" --jq .sha)" = "$C"
gh release view "$TAG" \
  --repo "$REPO" \
  --json assets,databaseId,isDraft,isImmutable,isPrerelease,tagName,targetCommitish
gh api --include "repos/$REPO/releases/latest"
```

`prerelease=true` 本身不能替代 Latest 基线对比。

## 7. 完成态 finalizer 定向重跑

这一步只验证 finalizer 完成态的幂等性，不用于恢复失败 run。禁止整 run rerun、`--failed`、
按页面编号猜 job，或对失败的首次 run 执行本步骤。

从 attempt 1 的 jobs API 保存完整 JSON，按完整 job 名唯一定位 finalizer 的 API
`databaseId`，并在 30 天窗口内执行一次：

```bash
jobs="$(gh api \
  "repos/$REPO/actions/runs/$TAG_RUN_ID/attempts/1/jobs?per_page=100")"
FINALIZER_JOB_DATABASE_ID="$(jq -er '
  [.jobs[]
   | select(.name ==
       "custom-finalize-release-assets / Keep only the public KAT Skill assets")]
  | if length == 1 then .[0].id else error("finalizer job is not unique") end
' <<< "$jobs")"

gh api --method POST \
  "repos/$REPO/actions/jobs/${FINALIZER_JOB_DATABASE_ID}/rerun"
```

GitHub 的该 endpoint 会重跑指定 job 及其下游依赖；finalizer 必须是依赖图末端，因此新
attempt 只能执行 finalizer。若 host、announce 或其他发布 job 被调度，立即取消并把 RC 判为
失败。重跑成功后，Release ID、两项资产的 ID/name/size/digest/创建更新时间和实算 SHA-256
必须与首次完成态一致；`download_count` 不参与身份比较。

## 8. EVIDENCE ACCEPTED

把以下内容写入当前 RC 的 Issue 证据小节：

- `B/H/M/A/T/C/D`、PR/tag run ID、attempt 和 job database IDs；
- 两份 manifest 的通道字段及 host 数据流；
- tag commit、Release numeric ID、状态和 Immutable 检查；
- wheel 文件名与 METADATA 版本；
- 两项资产元数据、实算 SHA-256 和 finalizer 重跑前后对比；
- Latest 基线、workflow/策略冻结窗口、action SHA、runner/tool 版本；
- 人工确认 Windows 仍只是 builder-image candidate。

对已合并 merge commit `C` 和上述作者证据执行证据型 `kat-pr-review`。只有无未处理 P0、P1、
P2 时进入 `EVIDENCE ACCEPTED`，随后才能关闭交付 Issue，并允许独立 stable promotion PR。
证据不完整不能用 PR 已合并替代。

## 9. 失败与隔离

任何首次 tag run 非 success 都先按 tag 和 push 时间窗枚举 Release，不能猜 host 是否已经运行：

| 状态 | 处理 |
| --- | --- |
| 没有 run | 枚举 Release；没有则记录失败，有则按 ID 隔离；tag 已消耗。 |
| host 前失败 | 仍枚举 Release；没有则记录失败，有则按 ID 隔离。 |
| Release 存在 | 保存 numeric ID/tag/资产状态，再 PATCH 并按同一 ID 回读。 |
| API 不可用或回读不确定 | 标记 `ISOLATION UNCONFIRMED`，升级给 owner。 |

finalizer 内置隔离只覆盖 finalizer 已启动后的普通失败；announce failure、skip、cancel、timeout、
runner 丢失等情况必须由值守人处理。人工身份必须有 Release Update 所需权限，且 workflow tree
冻结仍有效：

```bash
jq -cn '{draft: true, prerelease: true, make_latest: "false"}' |
  gh api --method PATCH \
    "repos/$REPO/releases/${RELEASE_ID}" \
    --input -

gh api "repos/$REPO/releases/${RELEASE_ID}" \
  --jq '{id,tag_name,draft,prerelease,immutable}'
```

Release 转为 draft 后不得再依赖按 tag 查询；回读必须使用保存的 numeric ID。隔离无法确认时
禁止关闭 Issue 和 stable promotion。失败对象保留审计身份并标记 `WITHDRAWN`；修复必须通过
follow-up PR，把仓库版本提升为 `rc.N+1`，产生新的 merge commit 后完整重来。不得移动、删除
后复用旧 tag，也不得用整 run rerun 伪装成新 RC。
