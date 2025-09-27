use crate::core::routes::general::handle_common_ending;
use crate::core::traits::Base;
use actix_web::web::{Data, Path};
use actix_web::{HttpRequest, HttpResponse, Responder, web};
use async_stream::stream;
use bytes::Bytes;
use futures_util::StreamExt;
use serde_json::json;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::AsyncReadExt;
use tokio::spawn;
use tokio::sync::{Notify, Semaphore, mpsc};
use tokio::time::timeout;
use tokio_stream::wrappers::ReceiverStream;

/// Handles file download requests.
///
/// This function retrieves a file identified by the provided ID and streams it to the client.
/// It manages concurrency control with both global and handler-specific semaphores.
///
/// # Arguments
/// * `path` - The path parameter containing the file ID to download.
/// * `permits` - A semaphore controlling global concurrency across all handlers.
/// * `manager` - A notifier for handling service shutdown events.
/// * `request_max_per_handler` - A map of semaphores controlling per-handler concurrency limits.
/// * `instance` - A map of registered handlers identified by name.
/// * `communication_line` - The communication bus for message exchange between handlers.
/// * `shared_state` - The shared state for accessing shared memory between handlers.
///
/// # Returns
/// * On success, returns a streaming HTTP response with the file content.
/// * On error, returns an appropriate error response:
///   - `500 Internal Server Error` if the file processing fails
///   - `400 Bad Request` if there's an error in the request
///   - `503 Service Unavailable` if the server is shutting down
pub async fn process_download(
    path: Path<String>,
    permits: Data<Arc<Semaphore>>,
    manager: Data<Arc<Notify>>,
    request_max_per_handler: Data<Arc<HashMap<String, Arc<Semaphore>>>>,
    instance: Data<Arc<HashMap<String, Arc<Box<dyn Fn() -> Box<dyn Base + Send + Sync> + Send + Sync>>>>>,
) -> impl Responder {
    let file_id = path.into_inner();
    let file_id_clone = file_id.clone();
    let str_handler: String = "download".to_string();
    let str_handler_copy = str_handler.clone();
    let permits_clone = permits.get_ref().clone();
    let permits_per_handler_clone = request_max_per_handler.get_ref().get(&str_handler_copy).unwrap().clone();
    let instance_to_run: Arc<Box<dyn Fn() -> Box<dyn Base + Send + Sync> + Send + Sync>> = instance.get_ref().get(&str_handler_copy).unwrap().clone();
    let processor = tokio::spawn(async move {
        let permit = permits_clone.clone().acquire_owned().await.unwrap();
        let permit_handler = permits_per_handler_clone.clone().acquire_owned().await.unwrap();
        let result = instance_to_run().run_file(str_handler_copy, file_id_clone).await;
        drop(permit);
        drop(permit_handler);
        result
    });
    tokio::select! {
        result = processor => {
            println!("Processed a request to handler {}", str_handler);
            match result {
                Ok(result) => {
                    match result {
                        Ok((mut source, _)) => {
                            let response_stream = stream! {
                                let mut buffer = vec![0; 128 * 1024];
                                loop {
                                    match source.read(&mut buffer).await {
                                        Ok(0) => break,
                                        Ok(n) => yield Ok(Bytes::copy_from_slice(&buffer[0..n])),
                                        Err(e) => yield Err(e)
                                    }
                                }
                            };
                            HttpResponse::Ok().streaming(response_stream)
                        },
                        Err(e) => {
                            let error_body = json!({
                                "status": "Error",
                                "message": e.to_string()
                            }).to_string();
                            HttpResponse::InternalServerError().json(error_body)
                        },
                    }
                }
                Err(e) => {
                     let error_body = json!({
                        "status": "Error",
                        "message": e.to_string()
                    }).to_string();
                    HttpResponse::BadRequest().json(error_body)
                }
            }
        },
        _ = manager.notified() => {
            println!("Service is stopped forcefully, request aborted");
             let error_body = json!({
                "status": "Error",
                "message": "Service closed forcefully"
            }).to_string();
            HttpResponse::ServiceUnavailable().json(error_body)
        }
    }
}

/// Handles file upload requests.
///
/// This function processes a file upload by streaming the payload data to a handler.
/// It manages concurrency with both global and handler-specific semaphores.
///
/// # Arguments
/// * `path` - The path parameter containing the name of the file being uploaded.
/// * `payload` - The request payload containing the file data as a stream.
/// * `req` - The HTTP request, used to extract the approximate file size from headers.
/// * `manager` - A notifier for handling service shutdown events.
/// * `permits` - A semaphore controlling global concurrency across all handlers.
/// * `instance` - A map of registered handlers identified by name.
/// * `request_max_per_handler` - A map of semaphores controlling per-handler concurrency limits.
/// * `communication_line` - The communication bus for message exchange between handlers.
/// * `shared_state` - The shared state for accessing shared memory between handlers.
///
/// # Returns
/// * On success, returns a JSON response with status "Ok" and a success message.
/// * On error, returns an appropriate error response:
///   - `400 Bad Request` if the required headers are missing
///   - `408 Request Timeout` if the stream read times out
///   - `500 Internal Server Error` if processing fails
///   - `503 Service Unavailable` if the server is shutting down
pub async fn process_upload(
    path: Path<String>,
    mut payload: web::Payload,
    req: HttpRequest,
    manager: Data<Arc<Notify>>,
    permits: Data<Arc<Semaphore>>,
    instance: Data<Arc<HashMap<String, Arc<Box<dyn Fn() -> Box<dyn Base + Send + Sync> + Send + Sync>>>>>,
    request_max_per_handler: Data<Arc<HashMap<String, Arc<Semaphore>>>>,
) -> impl Responder {
    let file_name = path.into_inner();
    let approx_size: usize;
    if let Some(limit) = req.headers().get("approximate-size") {
        approx_size = limit.to_str().unwrap().parse::<usize>().unwrap();
    } else {
        return HttpResponse::BadRequest().json(json!({"status": "Error", "message": "Upload request requires the Approximate-Size header"}));
    }
    let str_handler = "upload".to_string();
    let str_handler_copy = str_handler.clone();
    let permits_clone = permits.get_ref().clone();
    let permits_per_handler_clone = request_max_per_handler.get_ref().clone().get(&str_handler_copy).unwrap().clone();

    let (tx, rx) = mpsc::channel::<Bytes>(64);
    let instance_to_run: Arc<Box<dyn Fn() -> Box<dyn Base + Send + Sync> + Send + Sync>> = instance.get_ref().get(&str_handler_copy).unwrap().clone();
    let processor = spawn(async move {
        let permit = permits_clone.clone().acquire_owned().await.unwrap();
        let permit_handler = permits_per_handler_clone.clone().acquire_owned().await.unwrap();
        let mut stream_from_channel = ReceiverStream::new(rx);
        let result = instance_to_run()
            .run_stream(
                str_handler_copy,
                Box::pin(stream! {
                    while let Some(chunk) = stream_from_channel.next().await {
                        yield chunk;
                    }
                }),
                file_name,
                approx_size,
            )
            .await;
        drop(permit);
        drop(permit_handler);
        result
    });

    loop {
        match timeout(Duration::from_secs(20), payload.next()).await {
            Ok(None) => {
                break;
            }
            Ok(Some(Ok(chunk))) => {
                if tx.send(chunk.clone()).await.is_err() {
                    break;
                }
            }
            Ok(Some(Err(e))) => {
                eprintln!("Error reading stream: {}", e);
                break;
            }
            Err(_) => {
                eprintln!("Stream read timed out after 20 seconds");
                drop(tx);
                return HttpResponse::RequestTimeout().json(json!({"status": "Error", "message": "Stream read timed out"}));
            }
        }
    }
    drop(tx);
    handle_common_ending(processor, manager.get_ref().clone(), str_handler).await
}

/// Handles requests to retrieve metadata for a specific file.
///
/// This function retrieves metadata for a file identified by the provided ID.
/// It manages concurrency with both global and handler-specific semaphores.
///
/// # Arguments
/// * `path` - The path parameter containing the file ID to retrieve metadata for.
/// * `manager` - A notifier for handling service shutdown events.
/// * `permits` - A semaphore controlling global concurrency across all handlers.
/// * `instance` - A map of registered handlers identified by name.
/// * `request_max_per_handler` - A map of semaphores controlling per-handler concurrency limits.
///
/// # Returns
/// * On success, returns a JSON response with status "Ok" and the metadata.
/// * On error, returns an appropriate error response:
///   - `500 Internal Server Error` if the metadata processing fails
///   - `503 Service Unavailable` if the server is shutting down
pub async fn process_metadata(
    path: Path<String>,
    manager: Data<Arc<Notify>>,
    permits: Data<Arc<Semaphore>>,
    instance: Data<Arc<HashMap<String, Arc<Box<dyn Fn() -> Box<dyn Base + Send + Sync> + Send + Sync>>>>>,
    request_max_per_handler: Data<Arc<HashMap<String, Arc<Semaphore>>>>,
) -> impl Responder {
    let file_id = path.into_inner();
    let str_handler: String = "metadata".to_string();
    let str_handler_copy = str_handler.clone();
    let permits_clone = permits.get_ref().clone();
    let permits_per_handler_clone = request_max_per_handler.get_ref().clone().get(&str_handler_copy).unwrap().clone();
    let instance_to_run: Arc<Box<dyn Fn() -> Box<dyn Base + Send + Sync> + Send + Sync>> = instance.get_ref().get(&str_handler_copy).unwrap().clone();
    let processor = spawn(async move {
        let permit = permits_clone.clone().acquire_owned().await.unwrap();
        let permit_handler = permits_per_handler_clone.clone().acquire_owned().await.unwrap();
        let result = instance_to_run().run_metadata(str_handler_copy, file_id).await;
        drop(permit);
        drop(permit_handler);
        result
    });
    handle_common_ending(processor, manager.get_ref().clone(), str_handler).await
}
