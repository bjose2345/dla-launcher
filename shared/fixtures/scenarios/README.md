# Scanner scenarios

`manifest-v1.json` is the portable scanner-evidence fixture shared by native
implementations. Its schema is `shared/contracts/scanner/v1/schema.json`.

The manifest describes logical filesystem observations and expected identity
decisions without requiring platform-specific paths, permissions, executable
binaries, archives, or symbolic links in Git. The scenario directories are
reserved for materialized payloads when the traversal adapter is implemented.

Fixtures use synthetic names and metadata only. They must not contain a user's
library paths, file contents, credentials, or remotely downloaded private data.
