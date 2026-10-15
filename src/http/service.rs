use std::collections::HashMap;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender, channel};

use tokio::runtime::Runtime;
use tokio::task::AbortHandle;

use super::client::{BodySink, ClientConfig, HttpError, HttpResponse, build_client, execute};
use super::resolve::ResolvedRequest;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RequestId(u64);

#[derive(Debug)]
pub struct HttpEvent {
    pub id: RequestId,
    pub result: Result<HttpResponse, HttpError>,
}

/// Called from worker threads when an event is ready (e.g. request a repaint).
pub type Notify = Arc<dyn Fn() + Send + Sync>;

/// Runs requests on a shared Tokio runtime, off the UI thread.
pub struct HttpService {
    runtime: Runtime,
    client: reqwest::Client,
    config: ClientConfig,
    tasks: HashMap<RequestId, AbortHandle>,
    next_id: u64,
    tx: Sender<HttpEvent>,
    rx: Receiver<HttpEvent>,
    notify: Notify,
}

impl HttpService {
    pub fn new(config: ClientConfig, notify: Notify) -> Result<Self, HttpError> {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .thread_name("se-http")
            .enable_all()
            .build()
            .map_err(|e| HttpError::Other(format!("could not start the HTTP runtime: {e}")))?;
        // The client registers its connection pool with the runtime it is used on.
        let client = {
            let _guard = runtime.enter();
            build_client(&config)?
        };
        let (tx, rx) = channel();
        Ok(Self {
            runtime,
            client,
            config,
            tasks: HashMap::new(),
            next_id: 1,
            tx,
            rx,
            notify,
        })
    }

    /// Starts a request. The result arrives later through [`Self::poll`].
    pub fn send(&mut self, request: ResolvedRequest) -> RequestId {
        self.send_to(request, BodySink::Preview)
    }

    /// Starts a request whose body goes to `sink`.
    pub fn send_to(&mut self, request: ResolvedRequest, sink: BodySink) -> RequestId {
        let id = RequestId(self.next_id);
        self.next_id += 1;
        let client = self.client.clone();
        let tx = self.tx.clone();
        let notify = Arc::clone(&self.notify);
        let limit = self.config.preview_limit;
        let handle = self.runtime.spawn(async move {
            let result = execute(&client, request, sink, limit).await;
            // The receiver only disappears when the app is shutting down.
            let _ = tx.send(HttpEvent { id, result });
            notify();
        });
        self.tasks.insert(id, handle.abort_handle());
        id
    }

    /// Cancels a request. Its connection and body read are dropped; no event
    /// is delivered for it afterwards.
    pub fn cancel(&mut self, id: RequestId) {
        if let Some(handle) = self.tasks.remove(&id) {
            handle.abort();
        }
    }

    pub fn in_flight(&self) -> usize {
        self.tasks.len()
    }

    /// Returns finished requests. Results of cancelled requests are dropped.
    pub fn poll(&mut self) -> Vec<HttpEvent> {
        let mut events = Vec::new();
        while let Ok(event) = self.rx.try_recv() {
            if self.tasks.remove(&event.id).is_some() {
                events.push(event);
            }
        }
        events
    }

    /// Blocks until an event arrives (for tests and tools, not the UI).
    pub fn wait(&mut self, timeout: std::time::Duration) -> Option<HttpEvent> {
        let deadline = std::time::Instant::now() + timeout;
        loop {
            let left = deadline.saturating_duration_since(std::time::Instant::now());
            let event = self.rx.recv_timeout(left).ok()?;
            if self.tasks.remove(&event.id).is_some() {
                return Some(event);
            }
        }
    }
}
