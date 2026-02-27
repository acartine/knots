# Implementation Review

## Input
- Knot in `ready_for_implementation_review` state
- Feature branch with implementation

## Actions
1. Review code changes for correctness and style
2. Verify tests cover the requirements
3. Verify all sanity gates pass
4. Validate no security issues or regressions introduced
5. Approve or request changes

## Output
- Approved: `kno next <id>`
- Needs changes: `kno update <id> --status ready_for_implementation --add-note "<feedback>"`

## Failure Modes
- Critical issues found: `kno update <id> --status ready_for_implementation --add-note "<feedback>"`
- Architecture concern: `kno update <id> --status ready_for_implementation --add-note "<feedback>"`
