use std::sync::Arc;
use std::task::{Context, Poll};
use tracing::error;

use prost_types::Timestamp;
use rand::Rng;

use chrono::{DateTime, Utc};
use futures::future::{Either, Ready};
use http::{Request, Response};
use tonic::body::Body as GrpcBody;
use tower::{Layer, Service};

use crate::api::{ClientInfo, Session, Timite};
use crate::tim_storage::{TimStorage, TimStorageError};

const SESSION_METADATA_KEY: &str = "tim-session-key";

#[derive(Debug, thiserror::Error)]
pub enum TimSessionError {
    #[error("Store error: {0}")]
    StorageError(#[from] TimStorageError),
}

#[derive(Clone)]
pub struct TimSession {
    storage: Arc<TimStorage>,
}

impl TimSession {
    pub fn new(storage: Arc<TimStorage>) -> Self {
        Self { storage }
    }

    pub fn create(
        &self,
        timite: &Timite,
        client_info: &ClientInfo,
    ) -> Result<Session, TimSessionError> {
        let key = generate_session_key();
        let created_at = Utc::now();
        let session = Session {
            key,
            timite_id: timite.id,
            created_at: Some(to_proto_timestamp(&created_at)),
            client_info: Some(client_info.clone()),
        };
        self.storage.store_session(&session)?;
        Ok(session)
    }

    pub fn get(&self, session_key: &str) -> Result<Option<Session>, TimSessionError> {
        Ok(self.storage.find_session(session_key)?)
    }
}

#[derive(Clone)]
pub struct SessionLayer {
    sessions: Arc<TimSession>,
}

impl SessionLayer {
    pub fn new(sessions: Arc<TimSession>) -> Self {
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
    sessions: Arc<TimSession>,
}

impl<'a, S, Body> Service<http::Request<Body>> for SessionMiddleware<S>
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
        if req.uri().path() == "/tim.api.g1.TimApi/TrustedConnect" {
            return Either::Left(self.inner.call(req));
        }

        if let Some(session) = extract_session(&self.sessions, &req) {
            req.extensions_mut().insert(session);
        }

        Either::Left(self.inner.call(req))
    }
}

fn extract_session<B>(sessions: &Arc<TimSession>, req: &http::Request<B>) -> Option<Session> {
    let token = req.headers().get(SESSION_METADATA_KEY)?.to_str().ok()?;
    match sessions.get(token) {
        Ok(Some(session)) => Some(session),
        Err(e) => {
            error!("Failed to read session: {}", e);
            None
        }
        Ok(None) => None,
    }
}

fn generate_session_key() -> String {
    let mut rng = rand::thread_rng();
    let random_bytes: [u8; 32] = rng.gen();
    hex::encode(random_bytes)
}

fn to_proto_timestamp(dt: &DateTime<Utc>) -> Timestamp {
    Timestamp {
        seconds: dt.timestamp(),
        nanos: dt.timestamp_subsec_nanos() as i32,
    }
}
