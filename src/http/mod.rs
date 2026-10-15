//! HTTP client: validation, execution and cancellation, independent of the UI.

pub mod client;
pub mod request;
pub mod service;

pub use client::{ClientConfig, HttpError, HttpResponse};
pub use request::{BodyKind, Method, PreparedRequest, RequestDraft, ValidationError};
pub use service::{HttpEvent, HttpService, Notify, RequestId};
