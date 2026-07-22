---
name: review-agent
description: "用隔离 subagent 评议 PR、设计与 Grill 推荐答案，并输出 1–5 星问题金字塔。"
disable-model-invocation: true
---

# Review Agent

Read [review-standard.md](references/review-standard.md) completely before every review. Treat it as the sole authority for evaluation semantics. Keep `evals/` outside runtime context.

## 1. Freeze the review packets

1. Determine `mode`: `pr`, `design`, or `grill`.
2. Gather the in-scope materials available from the request and workspace. Static inspection and authoritative documentation lookup are allowed; runtime experiments and implementation changes are outside this review.
3. Build the object and reference packets exactly as defined by the standard. Tag every SDD/ADR as `accepted`, `draft`, `superseded`, `rejected`, or unknown.
4. Freeze both packets for the run. Treat a necessary unknown authority state or missing baseline as an input limitation, not an invitation to infer intent.

This step is complete when the mode, both frozen packets, and every decision document's known authority state are explicit.

## 2. Dispatch isolated reviewers

Use one fresh subagent with no parent-history fork (`fork_turns: "none"`) for each applicable branch. The orchestrator coordinates and assembles; it does not independently perform a branch review.

1. Start the cold-read and peer reviewers in parallel. Construct each prompt from its matching section of the standard, the shared clauses applicable to its permitted materials, and only the packets that section permits.
2. Validate the returned cold capsule immediately under the retry rule below, then freeze its understanding.
3. Start the consistency reviewer for `pr` mode only after that freeze. Construct its prompt from the consistency section, shared decision-authority/scoring/evidence clauses, the frozen cold-understanding section—claims, fact/inference labels, and critical unknowns—and the authority-tagged SDD/ADR packet. Keep cold scores, cold findings, and peer output outside this context. For `design` and `grill`, do not create this reviewer.
4. Require every reviewer to return a branch capsule containing:
   - branch status and leaf scores;
   - evidence confidence and limiting finding IDs for each leaf;
   - score-driving findings in `problem / static evidence / project impact` form;
   - the exact materials it used.
5. Add only the applicable auxiliary fields: the cold reviewer returns its frozen understanding and critical unknowns; any reviewer may return genuinely runtime-dependent pending-validation questions together with why static evidence cannot resolve them; the peer reviewer may return baseline challenges. Empty auxiliary fields are valid and should be omitted from the final report.

Do not give a reviewer another branch's output. Do not let reviewers load the Eval expectations. If a reviewer crosses its material boundary or returns an incomplete capsule, discard it and rerun that branch once with `fork_turns: "none"`. If the retry is also invalid, stop and report a branch-execution failure without inventing a score. Replacing an upstream capsule invalidates every capsule derived from it; discard and rerun those dependants.

This step is complete when all applicable branch capsules are frozen or a required branch has explicitly returned `unreviewable`.

## 3. Assemble the report

1. Verify each capsule against the standard before using it. Return an invalid capsule to a fresh reviewer rather than repairing its judgment in the orchestrator.
2. Calculate every parent and the overall score mechanically from the applicable leaf scores. Propagate `unreviewable` as specified.
3. Preserve findings already root-cause-grouped within each branch. Keep different branches separate and preserve their judgments and material provenance; add no new review conclusions during assembly.
4. Render the final report in the standard's pyramid order and stop at the material boundaries.

The review is complete only when every applicable leaf is accounted for, the score arithmetic and status propagation are correct, all formal findings have the required three evidence fields, and the report contains neither fixes nor an adopt/reject decision.
