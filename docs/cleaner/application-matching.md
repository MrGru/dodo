# Application matching (planned)

This phase does not implement installed-app indexing or orphan matching yet, but sets the structure needed for typed metadata:

- `ItemMetadata::Application`
- category boundaries for `InstalledApps` and `OrphanedFiles`

Future phases will add:

- bundle-id/team-id/entitlement-aware matching,
- confidence scoring,
- shared-container safeguards,
- uninstall preview with explicit risk presentation.
