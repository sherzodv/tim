use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use prost_types::Timestamp;
use uuid::Uuid;

use tower::{Service, Layer};
use http::Request;
use std::task::{Context, Poll};

use crate::api::{AuthenticateReq, ClientInfo, Session, Timite};

const SESSION_METADATA_KEY: &str = "tim-session-id";

#[derive(Clone)]
pub struct SessionService {
    store: Arc<RwLock<HashMap<String, SessionRecord>>>,
}

impl SessionService {
    pub fn new() -> Self {
        Self {
            store: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub fn ensure_session(&self, req: AuthenticateReq) -> Session {
        let session = Session {
            id: Uuid::new_v4().to_string(),
            created_at: Some(current_timestamp()),
        };

        let record = SessionRecord {
            session: session.clone(),
            timite: req.timite.unwrap_or_else(default_timite),
            client_info: req.client_info.unwrap_or_else(default_client_info),
        };

        self.store
            .write()
            .expect("session store poisoned")
            .insert(session.id.clone(), record);

        session
    }

    pub fn get(&self, session_id: &str) -> Option<SessionContext> {
        self.store
            .read()
            .expect("session store poisoned")
            .get(session_id)
            .map(|record| SessionContext {
                session_id: Some(record.session.id.clone()),
                timite: Some(record.timite.clone()),
                client_info: Some(record.client_info.clone()),
            })
    }
}

#[derive(Clone)]
struct SessionRecord {
    session: Session,
    timite: Timite,
    client_info: ClientInfo,
}

fn default_timite() -> Timite {
    Timite {
        id: 0,
        nick: String::new(),
    }
}

fn default_client_info() -> ClientInfo {
    ClientInfo {
        platform: String::new(),
    }
}

fn current_timestamp() -> Timestamp {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_else(|_| Duration::from_secs(0));
    Timestamp {
        seconds: now.as_secs() as i64,
        nanos: now.subsec_nanos() as i32,
    }
}

#[derive(Clone, Debug, Default)]
pub struct SessionContext {
    session_id: Option<String>,
    timite: Option<Timite>,
    client_info: Option<ClientInfo>,
}

impl SessionContext {
    pub fn session_id(&self) -> Option<&str> {
        self.session_id.as_deref()
    }

    #[allow(dead_code)]
    pub fn timite(&self) -> Option<&Timite> {
        self.timite.as_ref()
    }

    #[allow(dead_code)]
    pub fn client_info(&self) -> Option<&ClientInfo> {
        self.client_info.as_ref()
    }

    pub fn is_authenticated(&self) -> bool {
        self.session_id.is_some()
    }
}

#[derive(Clone)]
pub struct SessionLayer {
    sessions: Arc<SessionService>
}

impl SessionLayer {
    pub fn new(sessions: Arc<SessionService>) -> Self {
        Self { sessions }
    }
}

impl<S> Layer<S> for SessionLayer {
    type Service = SessionMiddleware<S>;
    fn layer(&self, inner: S) -> Self::Service {
        SessionMiddleware {
            inner,
            sessions: self.sessions.clone(),
        }
    }
}

#[derive(Clone)]
pub struct SessionMiddleware<S> {
    inner: S,
    sessions: Arc<SessionService>
}

impl<S, Body> Service<http::Request<Body>> for SessionMiddleware<S>
where
    S: Service<Request<Body>>,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = S::Future;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: http::Request<Body>) -> Self::Future {
        let path = req.uri().path();
        if path != "/tim.api.g1.TimApi/Authenticate" {
            // check auth and set session metadata
        } else {
            // skip check
        }
        self.inner.call(req)
    }
}