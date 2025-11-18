## Integration test naming

- TimApi flow tests should live in `tim-code/tests` and follow the `tim_api_flow_<scenario>.rs` pattern.
- Keep scenario names short and outcome-oriented, e.g. `tim_api_flow_trusted_events.rs`.
- When adding more flows, prefer one scenario per file to keep the iteration footprint small.
