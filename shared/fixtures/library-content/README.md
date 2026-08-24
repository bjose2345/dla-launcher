# Library-content scenarios

`manifest-v1.json` records the deterministic expected classification and safe
action candidates for the synthetic folder trees beside it. The contract is
`shared/contracts/library-content/v1/schema.json`.

The payloads are inert text markers with representative names. They are not
executables, media, applications, or copies of user content. A classifier that
inspects signatures must materialize appropriate synthetic byte fixtures in a
temporary test directory while preserving these expected paths and outcomes.

The Tauri production classifier executes against every tree and must reproduce
the complete expected output in this manifest. The classifier remains pure: it
receives scanner entries and does not read these paths itself.
