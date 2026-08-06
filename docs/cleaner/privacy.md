# Cleaner privacy posture

Cleaner is local-only by design.

## Guaranteed in phase 1

- No telemetry.
- No remote upload of file paths, metadata, or scan output.
- No content inspection beyond mock data generation.
- No credential store reads.

## Future implementation requirements

- Keep Mail/AI prompt content out of logs and diagnostics.
- Keep scanner reports local unless user explicitly exports to local file.
- Preserve typed boundaries so permission checks and filesystem access stay isolated under platform modules.
