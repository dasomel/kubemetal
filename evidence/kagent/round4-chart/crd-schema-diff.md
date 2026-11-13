# CRD property/type comparison

[pending probe] Agent, ModelConfig and RemoteMCPServer field-level differences between 0.9.12 and 1.0.0-alpha17 require the new probe output. The previous schematic was not a captured 0.9.12 Agent schema and has been removed.

[pending probe] `probe.sh OUTPUT_DIR [OLD_VERSION [NEW_VERSION]]` produces per-version `*-paths.json`, `*-inventory.json` and `schema-diff.json`. Paths include object containers, array items (`[]`), map values (`.*`) and indexed schema alternatives. Every old/new API-version pair of each target kind is compared explicitly.

[unverified] Added/removed/type-changed paths do not establish requiredness, defaults, CEL validation, admission, controller behavior or runtime compatibility. Those require separate evidence.
