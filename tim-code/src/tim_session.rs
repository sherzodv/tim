use std::sync::Arc;

use prost_types::Timestamp;
use rand::Rng;

use chrono::{DateTime, Utc};
use futures::future::{Either, Ready};
use http::{Request, Response};
use std::task::{Context, Poll};
use tonic::body::Body as GrpcBody;
use tower::{Layer, Service};

use crate::api::{Session, TrustedConnectReq};
use crate::tim_storage::TimStorage;

const SESSION_METADATA_KEY: &str = "tim-session-key";

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

#[derive(Clone)]
pub struct TimSessionService {
    storage: Arc<dyn TimStorage>,
}
impl TimSessionService {
    pub fn new(storage: Arc<dyn TimStorage>) -> TimSessionService {
        TimSessionService { storage }
    }

    pub fn create(&self, req: TrustedConnectReq) -> Result<Session, String> {
        let timite = req.timite.ok_or("timite expected")?;
        let client_info = req.client_info.ok_or("client_info expected")?;
        let key = generate_session_key();
        let created_at = Utc::now();

        let session = Session {
            key,
            timite_id: timite.id,
            created_at: Some(to_proto_timestamp(&created_at)),
            client_info: Some(client_info),
        };

        self.storage
            .store_session(&session)
            .map_err(|e| e.to_string())?;

        Ok(session)
    }

    pub fn get(&self, session_key: &str) -> Option<Session> {
        self.storage.find_session(session_key).ok()
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
        if path != "/tim.api.g1.TimApi/TrustedConnect" {
            let context_opt = req
                .headers()
                .get(SESSION_METADATA_KEY)
                .and_then(|value| value.to_str().ok())
                .and_then(|session_key| self.sessions.get(session_key));
            if let Some(context) = context_opt {
                req.extensions_mut().insert(context);
            }
        }
        Either::Left(self.inner.call(req))
    }
}
