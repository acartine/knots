---
"knots": minor
---

### Features
- Add NDJSON streaming output for `kno ls --stream`
- Add SQL-level pagination with `kno ls --limit` and `--offset` flags
- Add exploration workflow for lightweight investigations
- Add refine-knot-scope managed skill
- Support Codex project-level skills in `.agents/skills/`
- Add managed knots-create skill
- Add explore knot type with renamed builtin workflows
- Auto-register builtin workflows when config is missing
- Emit "no claimable knots found" in `poll --json` mode

### Fixes
- Handle workflow ID mismatches during sync gracefully
- Skip step metadata enrichment for unknown profiles
- Fix loom owner projection
- Repair legacy builtin workflow IDs
- Fix integration tests for project-only Codex skills

### Chores
- Remove legacy workflow runtime fallbacks and compatibility aliases
