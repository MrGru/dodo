# Code Review Workspace

Per BMAD best practice, each completed story's code review produces a Markdown file:

```
implementation-artifacts/review/
└── {story-key}-
    ├── code-review.md            # Detailed findings
    └── review-status.yaml        # Status: approved / changes_requested
```

The `bmad-code-review` workflow writes here when invoked. Run:

```
/bmad-code-review ./implementation-artifacts/stories/{story-key}.md
```

It updates both this file AND `sprint-status.yaml`.

## Review-status keys

- `approved` — story may proceed to validation
- `changes_requested` — story back to `in-progress`
- `blocked` — manual intervention required

See also: `_bmad-output/implementation-artifacts/agent-execution-loop.md`.
