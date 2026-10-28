use std::sync::{Arc,LazyLock};
use tokio::sync::{Semaphore,OwnedSemaphorePermit};
use scribe_shared::{ApiError,ApiResult,ErrorCode};
pub const PROJECT_QUOTA_BYTES:usize=500*1024*1024;
pub const UPLOAD_BODY_BYTES:usize=PROJECT_QUOTA_BYTES+64*1024;
static UPLOAD:LazyLock<Arc<Semaphore>>=LazyLock::new(||Arc::new(Semaphore::new(1)));
pub fn acquire()->ApiResult<OwnedSemaphorePermit>{UPLOAD.clone().try_acquire_owned().map_err(|_|ApiError::new(ErrorCode::ServiceUnavailable,"已有文件上传或ZIP检查正在处理，请完成后重试"))}
/// Retain ordinary API timeout while allowing large POST upload bodies to arrive.
pub async fn timeout(req:axum::extract::Request,next:axum::middleware::Next)->axum::response::Response{
 let path=req.uri().path();let multipart=req.headers().get(axum::http::header::CONTENT_TYPE).and_then(|h|h.to_str().ok()).is_some_and(|h|h.starts_with("multipart/form-data"));
 let heavy=req.method()==axum::http::Method::POST&&multipart&&path.starts_with("/api/projects/")&&(path.ends_with("/files")||path.contains("/import-zip"));
 match tokio::time::timeout(std::time::Duration::from_secs(if heavy{1200}else{30}),next.run(req)).await{Ok(response)=>response,Err(_)=>axum::response::IntoResponse::into_response((axum::http::StatusCode::GATEWAY_TIMEOUT,"Request timed out"))}
}
