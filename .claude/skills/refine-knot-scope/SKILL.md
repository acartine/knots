---
name: refine-knot-scope
description: >-
  Refine a knot's scope by tightening its title, rewriting its description for
  clarity, and defining testable acceptance criteria. Use when creating or
  updating a knot that needs sharper scope definition.
---

# Refine Knot Scope

You are refining a newly created engineering work item (knot).
Tighten the title, rewrite the description for clarity, and define or tighten
acceptance criteria. Keep the scope unchanged — do not broaden the request or
add speculative work.

## Input

- **Title**: {{title}}
- **Description**: {{description}}
- **Scope** (optional): {{scope}}

## Refinement Steps

### 1. Tighten the title

- Rewrite to be concise, clear, and action-oriented.
- Maximum 72 characters.
- Use imperative mood (e.g., "Add retry logic to webhook handler").
- Remove filler words, vague qualifiers, and unnecessary context.

### 2. Rewrite the description

- Improve clarity, remove ambiguity, and add specificity.
- State what the change does and why it matters.
- Include relevant technical context and constraints.
- Remove speculative or aspirational language.
- Keep it to 2-4 sentences.

### 3. Define acceptance criteria

**This is the most critical output.** Well-defined acceptance criteria are the
single most important factor in a successful work item. Without clear,
testable criteria the work item is incomplete regardless of how good the
title or description are.

- Produce 3-5 acceptance criteria.
- Each criterion must be testable and independently verifiable.
- Use numbered list format.
- Start each criterion with a verb (e.g., "Returns", "Validates", "Logs").
- Avoid subjective language ("looks good", "works well").
- If scope input is provided, use it to inform and tighten criteria.
- If no scope input is provided, derive criteria from the title and
  description.

## Output

Respond with **only** a valid JSON object — no markdown fences, no
explanation, no preamble. The object must have exactly three fields:

```json
{
  "title": "<refined title, ≤72 chars>",
  "description": "<refined description, 2-4 sentences>",
  "acceptance": "<numbered acceptance criteria, 3-5 items>"
}
```

All values are strings. The `acceptance` field uses newline-separated
numbered items (e.g., `"1. First criterion\n2. Second criterion"`).
