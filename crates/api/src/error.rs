use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use ferristrace_domain::{IngestError, SinkError, UnknownKey};
use ferristrace_otlp::DecodeError;
use tonic::Status;

#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    #[error("the request carries no bearer ingest key")]
    MissingBearer,
    #[error("the content type is not supported for OTLP")]
    UnsupportedContentType,
    #[error(transparent)]
    Decode(#[from] DecodeError),
    #[error(transparent)]
    Ingest(#[from] IngestError),
}

impl ApiError {
    fn status(&self) -> StatusCode {
        match self {
            Self::MissingBearer | Self::Ingest(IngestError::UnknownKey(UnknownKey)) => {
                StatusCode::UNAUTHORIZED
            }
            Self::UnsupportedContentType => StatusCode::UNSUPPORTED_MEDIA_TYPE,
            Self::Decode(_) | Self::Ingest(IngestError::Sink(SinkError::TimestampOutOfRange)) => {
                StatusCode::BAD_REQUEST
            }
            Self::Ingest(IngestError::Capped { .. }) => StatusCode::TOO_MANY_REQUESTS,
            Self::Ingest(IngestError::Sink(SinkError::Unavailable)) => {
                StatusCode::SERVICE_UNAVAILABLE
            }
            Self::Ingest(IngestError::Sink(SinkError::BatchTooLarge { .. })) => {
                StatusCode::PAYLOAD_TOO_LARGE
            }
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let status = self.status();
        if status.is_server_error() {
            tracing::error!(error = %self, "the gateway refused a payload");
        }
        (status, self.to_string()).into_response()
    }
}

impl From<ApiError> for Status {
    fn from(error: ApiError) -> Self {
        let message = error.to_string();
        match error {
            ApiError::MissingBearer | ApiError::Ingest(IngestError::UnknownKey(UnknownKey)) => {
                Self::unauthenticated(message)
            }
            ApiError::UnsupportedContentType
            | ApiError::Decode(_)
            | ApiError::Ingest(IngestError::Sink(SinkError::TimestampOutOfRange)) => {
                Self::invalid_argument(message)
            }
            ApiError::Ingest(IngestError::Capped { .. }) => Self::resource_exhausted(message),
            ApiError::Ingest(IngestError::Sink(SinkError::Unavailable)) => {
                Self::unavailable(message)
            }
            ApiError::Ingest(IngestError::Sink(SinkError::BatchTooLarge { .. })) => {
                Self::resource_exhausted(message)
            }
        }
    }
}
