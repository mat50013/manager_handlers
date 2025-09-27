use crate::core::communication::multibus::MultiBus;
use crate::core::communication::pub_sub;
use crate::core::{states::RequestRedis, states::SharedState, states::StateType};
use async_trait::async_trait;
use bytes::Bytes;
use futures::Stream;
use futures_util::StreamExt;
use serde_json::Value;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::AsyncRead;
use uuid::Uuid;

/// A trait that all handlers must implement. It provides methods for running
/// business logic, publishing messages, and dispatching messages via a
/// communication line (MultiBus).
#[async_trait]
pub trait Base {
    /// Asynchronously runs the handler logic, processing incoming data and using
    /// the provided communication line.
    ///
    /// # Arguments
    /// * `src` - The source of the data.
    /// * `data` - The data to be processed.
    ///
    /// # Returns
    /// * A `Result<String, Box<dyn std::error::Error>>` that can either return
    ///   a string message or an error.
    async fn run(&self, _src: String, _data: String) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        Err("run() not implemented for this handler.".into())
    }

    /// Processes a readable stream, suitable for streaming data from a request.
    ///
    /// # Arguments
    /// * `src` - The source of the data
    /// * `stream` - A readable stream for receiving incoming data.
    /// * `file_name` - The name of the file to be created from the stream content.
    /// * `lower_bound` - The approximate size or lower bound of the incoming data in bytes.
    ///
    /// # Returns
    /// * A `Result<String, Box<dyn std::error::Error + Send + Sync>>` - On success, returns a `String` indicating success or relevant output.
    ///   In case of an error, a boxed error type is returned.
    async fn run_stream(&self, _src: String, _stream: Pin<Box<dyn Stream<Item = Bytes> + Send>>, _file_name: String, _lower_bound: usize) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        Err("run_stream() not implemented for this handler.".into())
    }

    /// Processes a filename and returns a bytestream and its size.
    ///
    /// # Arguments
    /// * `src` - The source of the data.
    /// * `filename` - The name of the file to process.
    ///
    /// # Returns
    /// * A `Result<(Box<dyn AsyncRead + Send + Unpin>, u64), Box<dyn std::error::Error + Send + Sync>>`, where the first component
    ///   is a readable stream, and the second is the `Content-Length` (file size).
    async fn run_file(&self, _src: String, _filename: String) -> Result<(Box<dyn AsyncRead + Send + Unpin>, u64), Box<dyn std::error::Error + Send + Sync>> {
        Err("run_file() not implemented for this handler.".into())
    }

    /// Returns metadata for a given file name.
    ///
    /// # Arguments
    /// * `src` - The source or origin of the request.
    /// * `filename` - The name of the file whose metadata is being processed.
    ///
    /// # Returns
    /// * `Result<String, Box<dyn std::error::Error + Send + Sync>>` - On success, a `String` containing
    ///   metadata information is returned. On failure, a boxed error is returned.
    async fn run_metadata(&self, _src: String, _filename: String) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        Err("run_metadata() not implemented for this handler.".into())
    }

    /// Publishes a message to a specific target via the communication line.
    ///
    /// # Arguments
    /// * `message` - The message to publish.
    /// * `src` - The source of the message.
    /// * `to` - The target of the message.
    /// * `communication_line` - The communication line (MultiBus) for message exchange.
    ///
    /// # Returns
    /// * A `String` that contains the data from the response.
    async fn publish(&self, message: String, to: String) -> String {
        let result = pub_sub::publish(self.get_name(), to, message, self.get_communication_line()).await;
        let parsed: Value = serde_json::from_str(&result).unwrap();
        parsed["data"].to_string()
    }

    /// Dispatches a message to a specific target without awaiting a response.
    ///
    /// # Arguments
    /// * `message` - The message to dispatch.
    /// * `to` - The target to which the message is dispatched.
    async fn dispatch(&self, message: String, to: String) {
        pub_sub::dispatch("not important".to_string(), to, message, self.get_communication_line()).await;
    }

    /// Publishes a message to a Redis channel and waits for a response.
    ///
    /// This method sends a message through Redis pub/sub and blocks until a response
    /// is received or the timeout expires. Requires Redis to be configured in the manager.
    ///
    /// # Arguments
    ///
    /// * `message` - The message content to send
    /// * `to` - The target Redis channel name
    /// * `timeout` - Optional timeout in milliseconds (defaults to 10000ms if None)
    ///
    /// # Returns
    ///
    /// The response message from the subscriber, or "Timeout" if no response received
    async fn publish_redis(&self, message: String, to: String, timeout: Option<u64>) -> String {
        let redis_url = self.get_shared_state().clone().get(&"redis_url".to_owned()).await;
        match redis_url {
            StateType::String(url) => {
                if url.len() > 0 {
                    let client = redis::Client::open(url.as_str()).unwrap();
                    let mut pub_con = client.get_multiplexed_async_connection().await.unwrap();
                    let (mut sink, mut receiver_stream) = client.get_async_pubsub().await.unwrap().split();

                    let id_req = Uuid::new_v4().to_string();
                    let request = RequestRedis { id: id_req.clone(), message: message.clone(), reply_channel: format!("{}_{}", self.get_name(), id_req) };

                    sink.subscribe(request.reply_channel.clone()).await.unwrap();
                    let _: () = redis::cmd("PUBLISH").arg(to).arg(request).query_async(&mut pub_con).await.unwrap();

                    let timeout_duration = match timeout {
                        Some(t) => Duration::from_secs(t),
                        None => Duration::from_secs(30),
                    };
                    let msg = tokio::time::timeout(timeout_duration, receiver_stream.next()).await;
                    if msg.is_err() {
                        return "Timeout reached while waiting for response".to_owned();
                    }
                    let payload: String = msg.unwrap().unwrap().get_payload().unwrap();
                    payload
                } else {
                    "No redis url provided".to_owned()
                }
            }
            _ => "No redis url provided".to_owned(),
        }
    }

    /// Dispatches a message to a Redis channel without waiting for a response.
    ///
    /// This is a fire-and-forget method that publishes a message to Redis pub/sub.
    /// The method returns immediately after sending. Requires Redis to be configured.
    ///
    /// # Arguments
    ///
    /// * `message` - The message content to send
    /// * `to` - The target Redis channel name
    async fn dispatch_redis(&self, message: String, to: String) {
        let redis_url = self.get_shared_state().clone().get(&"redis_url".to_owned()).await;
        match redis_url {
            StateType::String(url) => {
                if url.len() > 0 {
                    let client = redis::Client::open(url.as_str()).unwrap();
                    let mut pub_con = client.get_multiplexed_async_connection().await.unwrap();
                    let id_req = Uuid::new_v4().to_string();
                    let request = RequestRedis { id: id_req.clone(), message: message.clone(), reply_channel: "".to_owned() };
                    let _: () = redis::cmd("PUBLISH").arg(to).arg(request).query_async(&mut pub_con).await.unwrap();
                }
            }
            _ => {}
        }
    }

    /// Subscribes to a Redis topic and waits for the next message.
    ///
    /// This method subscribes to a Redis pub/sub topic and blocks until a message
    /// is received. Useful for implementing event-driven architectures.
    ///
    /// # Arguments
    ///
    /// * `topic` - The Redis topic to subscribe to
    ///
    /// # Returns
    ///
    /// A `RequestRedis` containing the received message data and metadata, or an error
    async fn subscribe_topic_redis(&self, topic: String) -> Result<RequestRedis, Box<dyn std::error::Error + Send + Sync>> {
        let redis_url = self.get_shared_state().clone().get(&"redis_url".to_owned()).await;
        match redis_url {
            StateType::String(url) => {
                if url.len() > 0 {
                    let client = redis::Client::open(url).unwrap();
                    let (mut sink, mut receiver_stream) = client.get_async_pubsub().await.unwrap().split();
                    sink.subscribe(topic.clone()).await.unwrap();

                    let msg = receiver_stream
                        .next()
                        .await
                        .ok_or_else(|| <&str as Into<Box<dyn std::error::Error + Send + Sync>>>::into("Redis stream ended unexpectedly"))?;
                    let payload: String = msg.get_payload()?;
                    let result = serde_json::from_str(&payload)?;
                    Ok(result)
                } else {
                    Err("No redis url provided".into())
                }
            }
            _ => Err("No redis url provided".into()),
        }
    }

    /// Factory method for creating a new instance of the struct implementing the trait.
    fn new(communication_line: Arc<MultiBus>, shared_state: Arc<SharedState>) -> Self
    where
        Self: Sized;

    /// Method to get the name of the handler, user for publishing
    fn get_name(&self) -> String;

    /// Returns a reference to the internal communication line (MultiBus).
    ///
    /// This provides access to the message bus for internal handler-to-handler
    /// communication within the same service instance.
    fn get_communication_line(&self) -> Arc<MultiBus>;

    /// Returns a reference to the shared state storage.
    ///
    /// This provides access to the thread-safe shared state that can be used
    /// to store and retrieve data across all handlers in the service.
    fn get_shared_state(&self) -> Arc<SharedState>;
}
