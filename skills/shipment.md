# Shipment

## Input
- Knot in `ready_for_shipment` state
- Approved implementation on feature branch

## Purpose
The implementation has been reviewed and approved. Your job is to
promote it to its final destination (merge, push, verify). Do not
re-review or second-guess the approved work.

## Locating the Implementation
Find the feature branch by reading the knot metadata:
1. Check `commit:` tags on the knot — these are the implementation
   commit hashes.
2. Read the most recent handoff capsules — they typically name the
   branch (e.g., `worktree-<knot-id>-*`).
3. Run `git branch -a --contains <tagged-commit>` to confirm which
   branch holds the work.

If the tagged commits are already on main, shipment is already done —
verify CI is green and advance. Do not roll back.

## Invariant Adherence
- If the knot has invariants, verify they still hold after merge and
  before pushing to remote.
- Scope invariants: confirm no out-of-scope changes leaked into the
  merge.
- State invariants: confirm the required properties hold in the merged
  code on main.

## Step Boundary
- This session is authorized only for `shipment`.
- Complete exactly one shipment action, then stop.
- Allowed resting states after this session: `ready_for_shipment_review`
  or `ready_for_implementation`.
- Do not perform shipment review or final sign-off in this step.
- After the merge, push, handoff, and transition commands for shipment
  succeed, stop immediately.

## Actions
1. Locate the feature branch using the steps above
2. Perform the shipment action that matches the profile output mode:
   `{{ output }}` = `remote_main` means merge the feature branch to main.
   `{{ output }}` = `pr` means merge the approved pull request instead of
   performing a branch-only review flow.
3. Tag the knot with any new commit hashes created during merge using
   the `commit:` prefix:
   `short_hash=$(git rev-parse --short=12 <commit>)`
   `kno update <id> --add-tag "commit:${short_hash}"`
   Run this for each new commit created during shipment.
   Use short hashes only; do not use the full 40-character hash.
4. Push or verify the shipped main-branch result required by the output
   mode:
   `{{ output }}` = `remote_main` means push main after the merge.
   `{{ output }}` = `pr` means verify the merged PR produced the intended
   main-branch result and that the remote reflects it.
5. Verify CI passes on remote

## Output
- Code merged and pushed to main
- CI green on remote
- Add a handoff capsule summarizing shipment:
  `kno update <id> --add-handoff-capsule "<handoff_capsule>"`
- Transition:
  `kno next <id> <currentState> --actor-kind agent --agent-name <AGENT_NAME>`
  `--agent-model <AGENT_MODEL> --agent-version <AGENT_VERSION>`

## When to Roll Back
Only roll back to `ready_for_implementation` when the merge itself
fails (conflicts, CI red after merge). Finding unmerged commits is
the normal starting condition — that is what shipment is for.

## Failure Modes
- Merge conflicts:
  `kno update <id> --status ready_for_implementation`
  `--add-note "<blocker details>"`
  `kno update <id> --add-handoff-capsule "<merge conflict details>"`
- CI failure after merge:
  `kno update <id> --status ready_for_implementation`
  `--add-note "<blocker details>"`
  `kno update <id> --add-handoff-capsule "<CI failure details>"`
