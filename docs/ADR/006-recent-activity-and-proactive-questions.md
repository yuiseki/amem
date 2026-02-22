# ADR 006: Recent Activity Tracking and Proactive Clarification

## Status
Accepted

## Context
The assistant should adapt to recent owner activity.
At the same time, acting on missing assumptions can produce low-quality outcomes.
A controlled proactive-question policy is needed.

## Decision
Always consider recent activity and ask clarifying questions only when required slots are missing or conflicting.

### 1. Recent activity model
- Store daily logs in `.amem/agent/activity/YYYY/MM/YYYY-MM-DD.md`.
- `amem context` reads the last 7 days by default.
- Include timestamp/location/event type when available.

### 2. Question trigger conditions
- Required task slots are missing.
- Conflicts exist across memory sources (e.g., schedule/location mismatch).
- Recent activity is too sparse to support reliable suggestions.

### 3. Question policy
- At most one clarification question per turn.
- Prefer concrete, answerable questions.
- Persist answers into `agent/activity/` or `owner/` to avoid repeated asks.

### 4. Safety controls
- If the user requests no proactive questions, switch to passive mode.
- Present low-confidence assumptions as hypotheses, not facts.

## Consequences
- Better context quality before task execution.
- Lower interaction overhead by limiting question frequency.
- Trigger thresholds strongly affect user experience and need tuning.
