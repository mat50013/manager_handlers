use crate::manager::Config;
use actix_web::Error;
use actix_web::body::MessageBody;
use actix_web::dev::{ServiceRequest, ServiceResponse};
use actix_web::middleware::Next;
use actix_web::web::Data;

/// Middleware to authenticate HTTP requests by validating API keys.
///
/// This middleware intercepts incoming requests and checks for a valid API key in the
/// "Authorization" header. If the API key is missing or invalid, the request is rejected
/// with an "Unauthorized" error response. Otherwise, the request continues to the next
/// middleware or handler in the chain.
///
/// # Arguments
/// * `req` - The service request being processed
/// * `next` - The next middleware or handler in the processing chain
///
/// # Returns
/// * `Ok(ServiceResponse)` - If authentication succeeds, the response from the next handler
/// * `Err(Error)` - If authentication fails, returns an unauthorized error
///
/// # Errors
/// * Returns `ErrorUnauthorized("Invalid API Key")` when the provided API key doesn't match
/// * Returns `ErrorUnauthorized("Missing API Key")` when no Authorization header is present
pub async fn authentication_middleware(req: ServiceRequest, next: Next<impl MessageBody>) -> Result<ServiceResponse<impl MessageBody>, Error> {
    if req.path() == "/shutdown" {
        next.call(req).await
    } else {
        let config = req.app_data::<Data<Config>>().map(|c| c.get_ref().clone());

        if let Some(config) = config {
            if let Some(auth_header) = req.headers().get("Authorization") {
                if auth_header.to_str().unwrap_or("") != config.api_key {
                    return Err(actix_web::error::ErrorUnauthorized("Invalid API Key"));
                }
            } else {
                return Err(actix_web::error::ErrorUnauthorized("Missing API Key"));
            }
        }
        next.call(req).await
    }
}
