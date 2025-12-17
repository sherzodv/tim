use std::sync::Arc;
use std::task::Context;
use std::task::Poll;

use chrono::DateTime;
use chrono::Utc;
use futures::future::Either;
use futures::future::Ready;
use http::Request;
use http::Response;
use prost_types::Timestamp;
use rand::Rng;
use tonic::body::Body as GrpcBody;
use tower::Layer;
use tower::Service;
use tracing::error;
use tracing::instrument;
use tracing::trace;

use crate::api::ClientInfo;
use crate::api::Session;
use crate::api::Timite;
use crate::tim_storage::TimStorage;
use crate::tim_storage::TimStorageError;

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
        if req.uri().path() == "/tim.api.g1.TimGrpcApi/TrustedConnect"
            || req.uri().path() == "/tim.api.g1.TimGrpcApi/TrustedRegister"
        {
            return Either::Left(self.inner.call(req));
        }

        if let Some(session) = extract_session(&self.sessions, &req) {
            req.extensions_mut().insert(session);
        }

        Either::Left(self.inner.call(req))
    }
}

#[instrument(
    skip(sessions, req),
    level = "trace",
    fields(service = "session_middleware")
)]
fn extract_session<B>(sessions: &Arc<TimSession>, req: &http::Request<B>) -> Option<Session> {
    trace!("req path: {}", req.uri().path());
    let token = req.headers().get(SESSION_METADATA_KEY)?.to_str().ok()?;
    match sessions.get(token) {
        Ok(Some(session)) => Some(session),
        Err(e) => {
            error!("failed to read session: {}", e);
            None
        }
        Ok(None) => {
            trace!("session not found");
            None
        }
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
