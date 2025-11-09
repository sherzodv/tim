use std::collections::HashMap;
use std::sync::atomic::AtomicU64;
use std::sync::{Arc, RwLock};

use prost_types::Timestamp;

use chrono::{DateTime, Utc};
use futures::future::{Either, Ready};
use http::{Request, Response};
use std::task::{Context, Poll};
use tonic::body::Body as GrpcBody;
use tower::{Layer, Service};

use crate::api::{AuthenticateReq, Session, Timite};

const SESSION_METADATA_KEY: &str = "tim-session-id";

#[derive(Clone, Debug)]
pub struct TimSession {
    pub id: u64,
    pub timite: Timite,
    pub created_at: DateTime<Utc>,
}

#[derive(Clone)]
pub struct TimSessionService {
    sid_counter: Arc<AtomicU64>,
    store: Arc<RwLock<HashMap<u64, TimSession>>>,
}

fn to_proto_timestamp(dt: &DateTime<Utc>) -> Timestamp {
    Timestamp {
        seconds: dt.timestamp(),
        nanos: dt.timestamp_subsec_nanos() as i32,
    }
}
impl TimSessionService {

    pub fn new() -> TimSessionService {
        TimSessionService {
            sid_counter: Arc::new(AtomicU64::new(1)),
            store: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub fn create(&self, req: AuthenticateReq) -> Result<Session, String> {
        let session = TimSession {
            id: self
                .sid_counter
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed),
            timite: req.timite.ok_or("termite expected")?,
            created_at: Utc::now(),
        };

        self.store
            .write()
            .expect("session store poisoned")
            .insert(session.id, session.clone());

        Ok(Session {
            id: session.id,
            created_at: Some(to_proto_timestamp(&session.created_at)),
        })
    }

    pub fn get(&self, session_id: u64) -> Option<TimSession> {
        self.store
            .read()
            .expect("session store poisoned")
            .get(&session_id)
            .map(|record| record.clone())
    }
}

#[derive(Clone)]
pub struct SessionLayer {
    sessions: Arc<TimSessionService>,
}

impl SessionLayer {
    pub fn new(sessions: Arc<TimSessionService>) -> Self {
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
    sessions: Arc<TimSessionService>,
}

impl<S, Body> Service<http::Request<Body>> for SessionMiddleware<S>
where
    S: Service<Request<Body>, Response = Response<GrpcBody>>,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = Either<S::Future, Ready<Result<Self::Response, Self::Error>>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, mut req: http::Request<Body>) -> Self::Future {
        let path = req.uri().path();
        if path != "/tim.api.g1.TimApi/Authenticate" {
            let context_opt = req
                .headers()
                .get(SESSION_METADATA_KEY)
                .and_then(|value| value.to_str().ok())
                .and_then(|session_id| self.sessions.get(session_id.parse().ok()?));
            if let Some(context) = context_opt {
                req.extensions_mut().insert(context);
            }
        }
        Either::Left(self.inner.call(req))
    }
}
