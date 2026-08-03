# Validation Workspace

Per BMAD validation gist pattern, each validated story produces:

```
implementation-artifacts/validation/
└── {story-key}-
    ├── validation-report.md      # Detailed PASS/CONCERNS/FAIL findings
    └── validation-status.yaml    # Status: pass / concerns / fail
```

Validation runs AFTER code review passes. Run:

```
/bmad-validate ./implementation-artifacts/stories/{story-key}.md
```

A story is `done` only after BOTH review AND validation pass.

## Status keys

- `pass` — story marked `done` in sprint-status.yaml
- `concerns` — story back to dev with notes
- `fail` — story blocked, escalation required
