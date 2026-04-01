# Implementation

## Input
- Knot in `ready_for_implementation` state
- Approved implementation plan (in knot notes)

## Invariant Adherence
- If the knot has invariants, strictly adhere to every invariant condition
  throughout implementation.
- Scope invariants limit what code, modules, or systems may be touched.
- State invariants define properties that must remain true at all times.
- If an implementation step would violate an invariant, stop and redesign
  the approach rather than proceeding.

## Step Boundary
- This session is authorized only for `implementation`.
- Complete exactly one implementation action, then stop.
- Allowed resting states after this session:
  `ready_for_implementation_review`, `blocked`, or `ready_for_planning`.
- Do not merge the feature branch to main, perform shipment work, or
  continue into later workflow stages in this session.
- Opening or updating a review artifact for the implementation branch is
  allowed only if the profile explicitly requires it.
- After the implementation handoff and transition commands succeed, stop
  immediately.

## Actions
1. Create a feature branch from main in a worktree
2. Implement changes following the plan while respecting all invariants
3. Write tests for all new behavior
4. Run any sanity gates defined in the project or the plan
5. Add a handoff_capsule to the knot with:
   `kno update <id> --add-handoff-capsule "<handoff_capsule>"`
6. Commit and push the feature branch
7. Tag the knot with each commit hash using the `commit:` prefix:
   `short_hash=$(git rev-parse --short=12 <commit>)`
   `kno update <id> --add-tag "commit:${short_hash}"`
   Run this for every commit created during implementation.
   Use short hashes only; do not use the full 40-character hash.
8. Make the implementation artifact explicit for the profile output mode:
   `{{ output }}` = `remote_main` means the review target is the pushed
   feature branch itself, so do not open a PR for this step.
   `{{ output }}` = `pr` means the review target is a pull request, so
   open or update the PR for the feature branch.

## Output
- Working implementation on feature branch
- All tests passing with coverage threshold met
- Transition:
  `kno next <id> <currentState> --actor-kind agent --agent-name <AGENT_NAME>`
  `--agent-model <AGENT_MODEL> --agent-version <AGENT_VERSION>`

## Failure Modes
- Blocked by dependency:
  `kno update <id> --status blocked --add-note "<blocker details>"`
  `kno update <id> --add-handoff-capsule "<blocking dependency details>"`
- Implementation infeasible:
  `kno update <id> --status ready_for_planning`
  `--add-note "<blocker details>"`
  `kno update <id> --add-handoff-capsule "<reason infeasible>"`
