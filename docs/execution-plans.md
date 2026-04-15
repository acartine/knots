# Execution Plans

An `execution_plan` knot is a coordination artifact. Instead of tracking one
piece of implementation work, it tracks how a set of other knots should be
executed.

Use it when you already know the work items you want to coordinate and you need
to say:

- which broad phases happen first
- which checkpoints inside a phase must happen in order
- which knots can run at the same time

## Taxonomy

Execution plans use a three-level structure:

1. **Waves** are the top-level phases.
2. **Steps** are ordered checkpoints inside a wave.
3. **Knots** attached to one step are the concurrent work set for that step.

The sequencing rule is:

- waves are sequential
- steps inside a wave are sequential
- knots inside one step are concurrently executable

That means a plan like this:

```text
Wave 1
  Step 1: knots-a, knots-b
  Step 2: knots-c

Wave 2
  Step 1: knots-d, knots-e
```

should be read as:

1. Start `knots-a` and `knots-b` together.
2. When both are done, run `knots-c`.
3. When wave 1 is complete, begin wave 2.
4. Start `knots-d` and `knots-e` together.

## When To Use One

Reach for an `execution_plan` knot when the work itself is already broken into
multiple knots and the missing piece is orchestration.

Common fits:

- a multi-agent feature rollout
- a migration that has hard sequencing boundaries
- a large refactor where several independent sub-knots can move in parallel
- a release plan where implementation, review, and rollout must happen in
  explicit waves

## Building A Plan From The CLI

This walkthrough builds the plan incrementally from the CLI.

### 1. Create the execution-plan knot

```bash
kno new \
  "Auth migration execution plan" \
  --type execution_plan \
  --desc "Coordinate schema, API, and rollout work"
```

The new knot is the plan container. It does not replace the work knots. It
describes how those work knots should be sequenced.

### 2. Create or identify the work knots

The step commands reference existing knots by id, so create the underlying work
items first if they do not exist yet.

```bash
kno new "Add auth schema changes"
kno new "Update auth API handlers"
kno new "Ship UI integration"
kno new "Run migration rollout"
```

For the rest of the examples below, assume the execution-plan knot id is
`plan-9ab3` and the work knot ids are:

- `auth-a1b2` for schema changes
- `auth-c3d4` for API handlers
- `auth-e5f6` for UI integration
- `auth-g7h8` for rollout

### 3. Add waves

Start with the high-level phases.

```bash
kno plan wave add \
  plan-9ab3 \
  --name "Wave 1" \
  --objective "Land backend prerequisites"

kno plan wave add \
  plan-9ab3 \
  --name "Wave 2" \
  --objective "Ship the product-facing rollout"
```

Waves are always ordered. `Wave 1` must finish before `Wave 2` starts.

If you need to insert a phase in the middle later, use `--at`:

```bash
kno plan wave add \
  plan-9ab3 \
  --name "Wave 1.5" \
  --objective "Stabilization" \
  --at 2
```

### 4. Add steps inside each wave

Now define the checkpoints inside a wave.

This first step groups two knots together, which means they are intended to be
executable concurrently:

```bash
kno plan step add \
  plan-9ab3 \
  --wave 1 \
  --knot-ids auth-a1b2,auth-c3d4 \
  --notes "Schema and API can move in parallel"
```

This second step happens after step 1, because steps are sequential inside the
wave:

```bash
kno plan step add \
  plan-9ab3 \
  --wave 1 \
  --knot-ids auth-e5f6 \
  --notes "UI integration starts after backend work lands"
```

Then define wave 2:

```bash
kno plan step add \
  plan-9ab3 \
  --wave 2 \
  --knot-ids auth-g7h8 \
  --notes "Rollout begins after wave 1 is complete"
```

### 5. Reorder the plan when needed

Use the move commands to reshape the sequence without rebuilding the whole
plan.

Move a wave:

```bash
kno plan wave move plan-9ab3 --from 3 --to 2
```

Move a step within one wave:

```bash
kno plan step move plan-9ab3 --wave 1 --from 2 --to 1
```

If a wave or step should be deleted entirely:

```bash
kno plan step remove plan-9ab3 --wave 1 --step 2
kno plan wave remove plan-9ab3 --wave 2
```

Removal commands can prompt before deleting nested structure. Use `--force`
when you want to skip that confirmation.

### 6. Inspect the current plan

Use `show --json` to inspect the stored structure.

```bash
kno show plan-9ab3 --json
```

The `execution_plan` field will contain the ordered `waves` array, and each
wave will contain its ordered `steps`.

## Practical Reading Guide

When you read an execution plan, interpret it in this order:

1. Read the waves from lowest `wave_index` to highest.
2. Inside each wave, read the steps from lowest `step_index` to highest.
3. Inside each step, treat all listed knot ids as the parallel work set.

If two knots must not start together, they belong in different steps. If two
groups must not overlap at all, they belong in different waves.

## Helpful CLI Entry Points

These help screens are the fastest way to explore the command surface while you
are editing a plan:

```bash
kno plan --help
kno plan wave --help
kno plan step --help
kno plan wave add --help
kno plan step add --help
```

The nested help output mirrors the same model described here:

- `kno plan --help` explains waves, steps, and concurrent knot groups
- `kno plan wave --help` focuses on top-level sequencing
- `kno plan step --help` focuses on ordered checkpoints and concurrency
