use crate::core::traits::Base;
use actix_web::web::{Data, Path};
use actix_web::{HttpResponse, Responder, web};
use serde_json::json;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::sync::{Notify, Semaphore};
use tokio::task::JoinHandle;

/// Returns an unauthorized HTTP response with a descriptive error message.
///
/// This function is used as a fallback handler for routes that require authorization but
/// where the client doesn't have proper authentication or authorization.
///
/// # Returns
/// * A JSON response with a 401 Unauthorized status code containing an error message
pub async fn unauthorized() -> impl Responder {
    HttpResponse::Unauthorized().json(json!({"message": "Unauthorized: Access is denied"}))
}

/// Handles HTTP requests to shut down the server.
///
/// This function sets a shutdown flag to signal that the server should be terminated and
/// notifies any waiting tasks of the shutdown signal.
///
/// # Arguments
/// * `shutdown_flag` - An atomic flag that indicates the server's shutdown state
/// * `manager` - A notification mechanism to signal shutdown to waiting tasks
///
/// # Returns
/// * A JSON response with a 200 OK status code confirming the shutdown has been initiated
pub async fn shutdown_from_http(shutdown_flag: Data<Arc<AtomicBool>>, manager: Data<Arc<Notify>>) -> impl Responder {
    println!("shutdown from http");
    shutdown_flag.store(true, Ordering::SeqCst);
    manager.notify_waiters();
    HttpResponse::Ok().json(json!({"status": "Shutting down"}))
}

/// Common handler for awaiting and processing the result of an asynchronous task.
///
/// This function handles the common pattern of awaiting a spawned task that processes a request,
/// while also being responsive to shutdown signals. It properly formats the response based on
/// the success or failure of the task.
///
/// # Arguments
/// * `processor` - A spawned task (JoinHandle) that is processing the request
/// * `manager` - A notification mechanism that signals when the server is shutting down
/// * `str_handler` - The name of the handler processing the request, used for logging
///
/// # Returns
/// * `HttpResponse::Ok` - If the processor completes successfully
/// * `HttpResponse::InternalServerError` - If the processor fails or the service is shutting down
pub(crate) async fn handle_common_ending(processor: JoinHandle<Result<String, Box<dyn std::error::Error + Send + Sync>>>, manager: Arc<Notify>, str_handler: String) -> HttpResponse {
    tokio::select! {
        result = processor => {
            #[cfg(feature = "log")]
            println!("Processed a request to handler {}", str_handler);
            match result {
                Ok(result) => {
                    match result {
                        Ok(output) => HttpResponse::Ok().json(json!({"status": "Ok", "message": output})),
                        Err(e) =>  HttpResponse::InternalServerError().json(json!({"status": "Error", "message": e.to_string()})),
                    }
                }
                Err(e) => {
                   HttpResponse::InternalServerError().json(json!({"status": "Error", "message": e.to_string()}))
                }
            }
        },
        _ = manager.notified() => {
            #[cfg(feature = "log")]
            println!("Service is stopped forcefully, request aborted");
            HttpResponse::InternalServerError().json(json!({"status": "Error", "message":  "Service closed forcefully"}))
        }
    }
}

/// Handles HTTP requests to process specific handlers.
///
/// This function processes incoming HTTP requests by routing them to the appropriate handler
/// based on the path parameter. It manages concurrency with semaphores and supports graceful
/// shutdown handling.
///
/// # Arguments
/// * `path` - The path parameter containing the handler name to process the request.
/// * `payload` - The request payload containing the data to be processed.
/// * `permits` - A semaphore controlling global concurrency across all handlers.
/// * `instance` - A map of registered handlers identified by name.
/// * `manager` - A notifier for handling service shutdown events.
/// * `communication_line` - The communication bus for message exchange between handlers.
/// * `shared_state` - The shared state for accessing shared memory between handlers.
/// * `request_max_per_handler` - A map of semaphores controlling per-handler concurrency limits.
///
/// # Returns
/// * A JSON response indicating the result of the handler's processing.
/// * On success, returns a 200 OK with the handler's output message.
/// * On failure, returns an appropriate error status with an explanatory message.
pub async fn process_request(
    path: Path<String>,
    payload: web::Payload,
    permits: Data<Arc<Semaphore>>,
    instance: Data<Arc<HashMap<String, Arc<Box<dyn Fn() -> Box<dyn Base + Send + Sync> + Send + Sync>>>>>,
    manager: Data<Arc<Notify>>,
    request_max_per_handler: Data<Arc<HashMap<String, Arc<Semaphore>>>>,
) -> impl Responder {
    let handler_name: String = path.into_inner();

    let permits_arc = permits.get_ref().clone();
    let per_handler = request_max_per_handler.get_ref().get(&handler_name).expect("per-handler semaphore missing").clone();
    let instance_factory = instance.get_ref().get(&handler_name).expect("handler instance missing").clone();

    let body_bytes = payload.to_bytes().await.expect("payload read failed");
    let data_str = match std::str::from_utf8(&body_bytes) {
        Ok(s) => s.to_owned(),
        Err(_) => String::from_utf8_lossy(&body_bytes).into_owned(),
    };

    let task_handler_name = handler_name.clone();
    let processor = tokio::spawn(async move {
        let _global = permits_arc.acquire_owned().await.unwrap();
        let _local = per_handler.acquire_owned().await.unwrap();
        instance_factory().run(task_handler_name, data_str).await
    });

    handle_common_ending(processor, manager.get_ref().clone(), handler_name).await
}
