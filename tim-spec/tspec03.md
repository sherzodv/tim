## Sessions

Keep work strictly scoped. Keep changes minimal. Pay attention to codebase consistent naming. Use Result where needed.

### Step 1

Refactor api.proto, change session message to

Session:
  key: string // secret
  timite_id: uint64
  created_at: Timestamp
  client_info: ClientInfo

Fix code everywhere accordinly, no tests. Things should compile and work. Let review, hand test & iterate adjustments.

### Step 2

Rename TimApi::Authenticate -> TimApi::Connect in proto & do corresponding code updates in all places tim-(agents, web, code).
Meaning: Authenticate will be added later, Connect is for local trusted connect.

Review iterations.

### Step 3

TimStorage:
  store(session):
    "s:{session.key}" -> session

  find(key) -> session

Write tests. Review iterations.

### Step 4

Switch TimSessionService to TimStorage for session storage.

No tests. Review iterations.

### Step 5: Client session flow

Clients need to save a session in a persistent store and reuse accross multiple runs.

Initially client sends trusted register to establish a session.

TimApi:
  TrustedRegister:
    TrustedRegisterReq:
      string nick;
      ClientInfo client_info;

    TrustedRegisterRes:
      Session session;

Subsequent connections to server are done via TrustedConnect.