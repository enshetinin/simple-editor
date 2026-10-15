//! HTTP client: request model, variables, validation, execution and
//! cancellation, cURL import/export. Independent of the UI.

pub mod client;
pub mod curl;
pub mod json;
pub mod model;
pub mod request;
pub mod resolve;
pub mod sanitize;
pub mod secrets;
pub mod service;
pub mod vars;

pub use client::{BodySink, ClientConfig, HttpError, HttpResponse, JsonView, TextLines};
pub use model::{Auth, Body, BodyMode, KeyValue, RequestDefinition, Variable};
pub use request::Method;
pub use resolve::{FieldError, ResolvedRequest, resolve};
pub use service::{HttpEvent, HttpService, Notify, RequestId};
