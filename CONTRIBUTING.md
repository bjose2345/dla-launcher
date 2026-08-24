# Contributing

Thanks for helping improve DLA Launcher.

## Before changing code

Preserve the dependency direction: UI to frontend gateway to Tauri binding to
application use case to an application-owned port and platform adapter.

Local filesystem, database, scan, media, and launch operations must stay behind
Tauri bindings. Do not add a localhost application API. Keep catalog/cache data
separate from user-owned library state.

## Development rules

- Use the exact dependency versions already pinned by the workspace.
- Keep Tauri bindings thin and framework-specific imports out of `shared/ui`.
- Put authored user-facing copy in every locale catalog; parity tests must pass.
- Do not commit real catalog exports, `.dla` packages, databases, library paths,
  signing keys, logs, support bundles, or copyrighted user media.
- Synthetic fixtures must be minimal, clearly fictional, and test-only.
- Add focused tests for changed behavior and run the verification in README.
- Record any native platform gate that could not be executed.

## Pull requests

Explain the user-visible outcome, important design choices, verification run,
and any remaining platform limitation. Keep unrelated formatting or refactors
out of the same change.

By contributing, you agree that your contribution is licensed under
AGPL-3.0-only with the rest of the project.
