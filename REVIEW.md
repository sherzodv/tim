# Critical Bug Report - Tim Monorepo

**Review Date:** 2025-11-10
**Reviewer:** Claude Code
**Status:** Early WIP MVP Prototype

This review focuses on critical technical bugs that could cause system failures, crashes, or data corruption. Missing functionality is not included as this is an early prototype.

---

## Summary

Found **6 critical and high-severity bugs** that will cause system failures in production use:
- 2 Critical bugs that make the system unusable after first disconnect
- 1 High-severity bug causing infinite loops
- 3 Moderate bugs affecting reliability and security

---

## 🔴 CRITICAL #1: Message Broadcast Failure - Single Disconnect Breaks Entire System

**Severity:** CRITICAL
**Location:** `tim-code/src/tim_space.rs:78-81`

### Problem

When broadcasting messages to subscribers, the code fails catastrophically if ANY subscriber's channel is closed:

```rust
for sub in snapshot {
    if !sub.receive_own_messages && sub.session.timite.id == session.timite.id {
        continue;
    }
    let upd_id = self.upd_counter.fetch_add(1, Ordering::Relaxed);
    let msg_id = self.msg_counter.fetch_add(1, Ordering::Relaxed);
    sub.chan
        .send(update_new_message(upd_id, msg_id, &req, &session))
        .await
        .map_err(|e| e.to_string())?;  // ❌ Propagates error, stops iteration
}
```

### Impact

- If a single client disconnects without cleanup, their channel closes
- The next message sent by ANY user will fail when trying to send to the dead channel
- Error propagates back to the sender (who did nothing wrong), not the disconnected client
- **The entire messaging system becomes unusable after one disconnect**
- Subsequent message attempts continue to fail
- System effectively locks up

### Reproduction

1. Connect two clients (A and B)
2. Client A subscribes to space
3. Client B subscribes to space
4. Client A sends a message (works fine)
5. Client B disconnects abruptly (browser close, network failure)
6. Client A tries to send another message
7. **Result:** Client A's send fails with an error because Client B's channel is closed

### Recommended Fix

Handle closed channels gracefully and continue iteration:

```rust
for sub in snapshot {
    if !sub.receive_own_messages && sub.session.timite.id == session.timite.id {
        continue;
    }
    let upd_id = self.upd_counter.fetch_add(1, Ordering::Relaxed);
    let msg_id = self.msg_counter.fetch_add(1, Ordering::Relaxed);

    // Ignore send failures - subscriber likely disconnected
    let _ = sub.chan
        .send(update_new_message(upd_id, msg_id, &req, &session))
        .await;
}
```

Even better: Track which sends failed and clean up those subscribers.

---

## 🔴 CRITICAL #2: Memory Leak - Unbounded Subscriber Growth

**Severity:** CRITICAL
**Location:** `tim-code/src/tim_space.rs:87-106`

### Problem

Subscribers are added to the HashMap on connection but never removed when they disconnect:

```rust
pub fn subscribe(
    &self,
    req: &SubscribeToSpaceReq,
    session: &TimSession,
) -> mpsc::Receiver<SpaceUpdate> {
    let (sender, receiver) = mpsc::channel(BUFFER_SIZE);
    let mut guard = self
        .subscribers
        .write()
        .expect("space updates subscribers lock poisoned");
    guard.insert(
        session.id,
        Subscriber {
            receive_own_messages: req.receive_own_messages,
            chan: sender,
            session: session.clone(),
        },
    );  // ❌ Never removed
    receiver
}
```

There is no corresponding `unsubscribe()` method or cleanup mechanism.

### Impact

- Every connection adds an entry to `subscribers` HashMap that stays forever
- Memory usage grows unbounded with each new connection
- Each disconnected client leaves behind a `Subscriber` with a closed channel
- Eventually leads to memory exhaustion and server crash
- **Combines with Bug #1:** Each dead subscriber causes message broadcast failures
- System becomes progressively more unstable over time
- Memory leak accelerates with connection churn

### Reproduction

1. Start server
2. Connect/disconnect 1000+ clients in a loop
3. Observe memory growth
4. Memory never decreases
5. Eventually: OutOfMemory or system slowdown

### Recommended Fix

Multiple approaches:

**Option A - Explicit Cleanup:**
Add an unsubscribe method and detect when channels close:

```rust
pub fn unsubscribe(&self, session_id: u64) {
    let mut guard = self.subscribers.write().expect(...);
    guard.remove(&session_id);
}
```

**Option B - Lazy Cleanup:**
During broadcast, detect closed channels and remove them:

```rust
pub async fn process(&self, req: SendMessageReq, session: TimSession) -> Result<SendMessageRes, String> {
    let snapshot = { /* ... */ };
    let mut failed_sessions = Vec::new();

    for sub in snapshot {
        if sub.chan.send(...).await.is_err() {
            failed_sessions.push(sub.session.id);
        }
    }

    // Cleanup failed subscribers
    if !failed_sessions.is_empty() {
        let mut guard = self.subscribers.write().expect(...);
        for id in failed_sessions {
            guard.remove(&id);
        }
    }

    Ok(SendMessageRes { error: None })
}
```

**Option C - Weak References:**
Use a cleanup task that periodically checks for closed channels.

---

## 🟠 HIGH SEVERITY #3: Agent Infinite Loop

**Severity:** HIGH
**Location:** `tim-agent/src/main.rs:18-53` and `tim-agent/src/llm.rs:155-169`

### Problem

Both agents (Jarvis and Alice) respond to ALL messages without filtering by sender:

```rust
async fn on_space_message(&mut self, _sender_id: u64, content: &str) -> Result<(), AgentError> {
    // Note: _sender_id is ignored!
    if !self.conf.response_delay.is_zero() {
        sleep(self.conf.response_delay).await;
    }
    self.push_history(DialogRole::Peer, content);
    let context = self.render_history();
    let prompt_body = if context.is_empty() {
        content.trim().to_string()
    } else {
        format!("Conversation so far:\n{context}\nRespond to the latest peer message.")
    };
    let reply = self.respond(&prompt_body).await?;
    self.push_history(DialogRole::Agent, &reply);
    self.client.send_message(&reply).await?;  // ❌ Always responds
    Ok(())
}
```

Combined with both agents sending initial messages on startup:
- Jarvis (ID 1): "Morning Alice, status update?"
- Alice (ID 2): "Jarvis, I can take the next task, thoughts?"

### Impact

**Infinite Conversation Loop:**
1. Jarvis starts, sends initial message to space
2. Alice receives Jarvis's message, responds
3. Jarvis receives Alice's response, responds back
4. Alice receives Jarvis's response, responds back
5. **Loop continues indefinitely**

**Consequences:**
- Rapidly fills the space with automated messages
- Makes the system unusable for actual human users
- Triggers OpenAI API rate limits
- **Cost explosion** - thousands of API calls per hour
- History buffers fill up with agent-to-agent chatter
- May exceed OpenAI monthly spending limits

### Reproduction

1. Start `tim-code` backend
2. Start `tim-agent` with both Jarvis and Alice
3. Observe logs - agents start responding to each other
4. Messages never stop
5. OpenAI API costs accumulate rapidly

### Recommended Fix

**Option A - Filter by Sender (Recommended):**
```rust
async fn on_space_message(&mut self, sender_id: u64, content: &str) -> Result<(), AgentError> {
    // Ignore messages from self
    if sender_id == self.timite_id {
        return Ok(());
    }

    // Ignore messages from other agents
    if self.is_agent(sender_id) {
        return Ok(());
    }

    // ... rest of logic
}
```

**Option B - Explicit Addressing:**
Only respond when explicitly mentioned:
```rust
if !content.to_lowercase().contains(&self.conf.nick.to_lowercase()) {
    return Ok(()); // Not addressed to this agent
}
```

**Option C - Turn-Based Protocol:**
Implement a protocol where agents don't respond to each other's messages without explicit turn-taking logic.

---

## 🟡 MODERATE #4: Race Condition in Message Broadcasting

**Severity:** MODERATE
**Location:** `tim-code/src/tim_space.rs:61-70`

### Problem

The code creates a snapshot of subscribers, releases the read lock, then iterates with stale data:

```rust
let snapshot = {
    let guard = self
        .subscribers
        .read()
        .expect("space updates subscribers lock poisoned");
    guard
        .iter()
        .map(|(_, entry)| entry.clone())
        .collect::<Vec<_>>()
}; // ❌ Read lock released here

// Time window where new subscribers can join but won't be in snapshot

for sub in snapshot {
    // Iterating with potentially stale data
    if !sub.receive_own_messages && sub.session.timite.id == session.timite.id {
        continue;
    }
    // ... send message
}
```

### Impact

- Subscribers who join during the time window between snapshot creation and message send won't receive the message
- Subscribers in the snapshot may have already disconnected
- Inconsistent message delivery depending on timing
- **Combines with Bug #1:** Dead subscribers in snapshot cause broadcast failures
- Race window can be significant under load
- Users may miss messages if they connect at the wrong time

### Analysis

This is a classic TOCTTOU (Time-Of-Check-Time-Of-Use) race condition. The subscriber list can change between when it's read and when it's used.

### Recommended Fix

**Option A - Accept the Behavior (Document It):**
This may be acceptable for an MVP. New subscribers naturally won't receive historical messages. Just ensure it's documented.

**Option B - Hold Lock Longer:**
Hold the read lock during iteration (but this blocks new subscriptions):
```rust
let guard = self.subscribers.read().expect(...);
for (_, sub) in guard.iter() {
    // ... send to sub
}
```
Note: This creates potential deadlock issues with async code.

**Option C - Use Arc and Lock-Free Structures:**
Use `Arc<DashMap>` for concurrent access without locks.

**Option D - Message Queue per Session:**
Store pending messages per session to handle race conditions.

For an MVP, Option A (document and accept) is reasonable.

---

## 🟡 MODERATE #5: Expect-Based Panics on Lock Poisoning

**Severity:** MODERATE
**Location:** Multiple locations

### Problem

The code uses `.expect()` when acquiring locks, which will panic and crash the entire server if a lock is poisoned:

**In `tim_space.rs`:**
```rust
let guard = self
    .subscribers
    .read()
    .expect("space updates subscribers lock poisoned");  // ❌ Panics on poison
```
Lines: 65, 96

**In `tim_session.rs`:**
```rust
self.store
    .write()
    .expect("session store poisoned")  // ❌ Panics on poison
```
Lines: 56, 68

### Impact

- If any thread panics while holding a lock, the lock becomes "poisoned"
- All subsequent attempts to acquire that lock will panic
- **Cascading failure** - one panic triggers more panics
- Entire server crashes with no recovery
- No graceful degradation
- In production, a single panic could take down the whole service
- Loss of all in-memory session and subscriber data

### Background

Rust's `RwLock` and `Mutex` become "poisoned" when a thread panics while holding the lock. This is a safety mechanism to prevent accessing potentially corrupted data.

### Recommended Fix

**Option A - Unwrap Poison (Unsafe but Pragmatic):**
```rust
let guard = match self.subscribers.read() {
    Ok(guard) => guard,
    Err(poisoned) => {
        tracing::error!("Lock poisoned, attempting recovery");
        poisoned.into_inner()  // Get the data anyway
    }
};
```

**Option B - Return Error:**
```rust
pub async fn process(
    &self,
    req: SendMessageReq,
    session: TimSession,
) -> Result<SendMessageRes, String> {
    let guard = self
        .subscribers
        .read()
        .map_err(|_| "subscriber lock poisoned")?;
    // ...
}
```

**Option C - Use Tokio Mutex:**
Tokio's async mutex doesn't poison:
```rust
use tokio::sync::RwLock;  // Instead of std::sync::RwLock
```

For an MVP, Option C (Tokio mutex) is cleanest since you're already using Tokio.

---

## 🟡 MODERATE #6: CORS Configuration Too Permissive

**Severity:** MODERATE (Security)
**Location:** `tim-code/src/main.rs:47-50`

### Problem

CORS configuration allows any origin to access the API:

```rust
let cors = CorsLayer::new()
    .allow_methods(Any)
    .allow_headers(Any)
    .allow_origin(Any);  // ❌ Allows any origin
```

### Impact

- Any website can make requests to your backend
- No protection against CSRF attacks
- Malicious third-party sites can:
  - Send messages as users
  - Subscribe to space updates
  - Potentially extract user data
- Exposes your API to abuse
- Privacy concerns if sensitive data is transmitted

### Security Implications

Even for an MVP/prototype:
- If deployed on a public network, this is exploitable
- Other users on the same network can access your API
- No authentication boundary at the HTTP layer

### Recommended Fix

**For Development:**
```rust
let cors = CorsLayer::new()
    .allow_methods(Any)
    .allow_headers(Any)
    .allow_origin("http://localhost:5173".parse::<HeaderValue>().unwrap())
    .allow_origin("http://127.0.0.1:5173".parse::<HeaderValue>().unwrap());
```

**For Production:**
```rust
let allowed_origin = std::env::var("ALLOWED_ORIGIN")
    .unwrap_or_else(|_| "http://localhost:5173".to_string());
let cors = CorsLayer::new()
    .allow_methods([Method::GET, Method::POST])
    .allow_headers([CONTENT_TYPE, "tim-session-id"])
    .allow_origin(allowed_origin.parse::<HeaderValue>().unwrap());
```

---

## Additional Observations (Not Critical)

### Minor Issues Found:

1. **No session cleanup:** Sessions in `TimSessionService` are never removed (minor memory leak)
2. **Hardcoded endpoints:** Agent code has hardcoded `http://127.0.0.1:8787` (not critical for prototype)
3. **No rate limiting:** Agents can spam messages without throttling (beyond response_delay)
4. **No message size limits:** Large messages could cause issues
5. **No authentication validation:** Sessions are created without password or token verification

These are acceptable for an early MVP but should be addressed before production use.

---

## Testing Recommendations

To verify these bugs and fixes:

1. **Test Bug #1 & #2:**
   - Create test with 3 clients
   - Connect all, subscribe all
   - Disconnect one abruptly (kill process)
   - Have another send message
   - Observe failure

2. **Test Bug #3:**
   - Start tim-code
   - Start tim-agent
   - Watch logs for infinite loop
   - Monitor OpenAI API usage

3. **Test Bug #5:**
   - Inject a panic during lock acquisition
   - Verify system doesn't crash entirely

---

## Priority Recommendations

### Immediate (Must Fix Before Any Multi-User Testing):
1. **Fix Bug #1** - Handle closed channels gracefully in broadcast
2. **Fix Bug #2** - Implement subscriber cleanup on disconnect
3. **Fix Bug #3** - Add sender filtering to prevent agent loops

### High Priority (Before Beta):
4. Fix Bug #4 - Document or improve race condition handling
5. Fix Bug #5 - Replace `.expect()` with error handling or Tokio mutexes

### Medium Priority (Before Production):
6. Fix Bug #6 - Restrict CORS to specific origins
7. Add session cleanup
8. Add rate limiting

---

## Conclusion

The system has good architectural bones and clean code organization. However, the combination of **Bugs #1 and #2** means the system will become completely unusable after the first client disconnect. These must be fixed before any real-world testing with multiple concurrent clients.

Bug #3 will cause immediate problems when running agents and could result in significant API costs.

All critical bugs are fixable with relatively small code changes. The architecture doesn't need fundamental redesign.
