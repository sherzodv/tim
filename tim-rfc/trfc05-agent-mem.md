# Agents memory & storage design

## Update after quick sketch implemenation

This is a wrong path. Storage basically replicates what tim-code already has. We should use tim-code as a storage. Timeline we already have there (only need API to access), and we need add API & functionality for knowledge base and operation memory (compacted contexts).
Reverting changes of this spec.

## Initial thoughts

- Two memory types:
  - Storage: long term, persistent
  - Memory: operational, in memory, computed

- Two storage types:
  - Timeline: events, conversations, DAG, big, raw
  - Knowledge base: factological, connected, tagged

- Timeline storage:
  - rocks db key format: `t:{timite_id}:{timestamp}:`
  - column familiy: `log`
  - header: what, type kind
  - content

## Implementation plan & discussion

### Storage implementation

1. Protobuf message for timeline storage

```proto
message TimelineEvent {
  uint64 timite_id = 1;
  string header = 2;
  string content = 3;
}
```

2. llm/storage.rs

Use tim-core/src/tim_storage.rs as a style & arch reference.

```rust
pub struct Storage {}

impl Storage {
    pub fn store_timeline_event(&self, timite_id: u64, event: &TimelineEvent) -> Result<(), StorageError>
    pub fn timeline_size(&self): Result<u64, StorageError>
    pub fn timeline(start: u64, size: u16): Result<Vec<TimelineEvent>, StorageError>
}
```

Implement To(TimelineEvent) for each space update.

3. llm/memory.rs

Use Storage. Remove current vector in memory storage.