# manager_handlers

[![Crates.io](https://img.shields.io/crates/v/manager_handlers.svg)](https://crates.io/crates/manager_handlers)
[![Documentation](https://docs.rs/manager_handlers/badge.svg)](https://docs.rs/manager_handlers)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)

A scalable, async-driven microservice framework for Rust that enables dynamic handler registration with HTTP endpoints and internal pub/sub messaging.

## Overview

`manager_handlers` is built on top of Actix Web and Tokio, providing a robust foundation for building microservice architectures. It allows you to create handlers that process HTTP requests and communicate with each other through an internal message bus, making it highly modular and scalable.

## Features

- **🚀 Dynamic Handler Registration**: Register handlers at runtime with configurable replica counts for horizontal scaling
- **📬 Dual Communication**: Internal pub/sub messaging bus + Redis pub/sub for distributed systems
- **🔒 Security First**: Built-in TLS support with optional client certificate authentication
- **📁 File Operations**: Streaming file upload/download with metadata support
- **🎯 Concurrency Control**: Semaphore-based request limiting with per-handler concurrency settings
- **💾 Shared State**: Thread-safe state management with support for primitives and function pointers
- **🔄 Async by Design**: Built on Tokio for high-performance async I/O operations
- **⚡ Zero-Copy Streaming**: Efficient file handling without loading entire files into memory
- **🛑 Graceful Shutdown**: Coordinated service termination with cleanup

## Requirements

- Rust 1.85+ (2024 edition)
- Tokio runtime
- OpenSSL (for TLS support)

## Installation

Add this to your `Cargo.toml`:

```toml
[dependencies]
manager_handlers = "0.7.1"
async-trait = "0.1"
```

Or use cargo:

```bash
cargo add manager_handlers async-trait
```

## Quick Start

```rust
use manager_handlers::manager::Manager;
use manager_handlers::handler;
use async_trait::async_trait;

// Define a simple handler using the macro
handler!(HelloHandler, hello;
    async fn run(&self, _: String, data: String) -> Result<String, Box<dyn std::error::Error + Send + Sync>> { 
     Ok(format!("Hello, {}!", data))
    }
);

#[tokio::main]
async fn main() {
    let mut manager = Manager::new_default();
    
    // Register the handler with 3 replicas
    manager.add_handler::<HelloHandler>("hello", 3);
    
    // Start the server on port 8080
    manager.start().await;
}
```

## Core Concepts

### Handlers

Handlers are the core processing units that:
- Process incoming HTTP requests
- Communicate with other handlers via message bus
- Access shared state
- Handle file operations

### Message Bus

The internal `MultiBus` provides:
- Async message passing between handlers
- Request/response pattern with `publish()`
- Fire-and-forget pattern with `dispatch()`
- Backpressure and timeout handling

### Shared State

Thread-safe storage supporting:
- Primitive types (Int, Float, String, etc.)
- Synchronous and async function pointers
- Custom types via `AnyType` wrapper

## Usage Examples

### Creating Custom Handlers

Implement the `Base` trait for full control:

```rust
use async_trait::async_trait;
use std::sync::Arc;
use futures::future::{BoxFuture, FutureExt};
use std::time::Duration;
use tokio::time::sleep;
use manager_handlers::multibus::MultiBus;
use manager_handlers::manager::{StateType, SharedState, Base};

pub struct MyHandler {
   communication_line: Arc<MultiBus>,
   shared_state: Arc<SharedState>
};

#[async_trait]
impl Base for MyHandler {
    async fn run(&self, src: String, data: String) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        // Process the incoming data
        println!("Received data: {}", data);
        
        // Example: Publishing a message and awaiting response
        let response = self.publish(
            data.clone(),
            "other_handler".to_string()
        ).await;
        
        // Example: Fire-and-forget dispatch
        self.dispatch(
            "notification data".to_string(), 
            "notification_handler".to_string()
        ).await;
        
        // Example: Store a value in shared state
        self.shared_state.insert(&"counter".to_string(), StateType::Int(42)).await;
        
        // Example: Store a synchronous function
        let shared_function: Arc<dyn Fn(String) -> String + Sync + Send> = Arc::new(|input: String| -> String {
          println!("Hello, {}!", input);
          input + " pesto"
        });
        self.shared_state.insert(&"sync_func".to_string(), StateType::FunctionSync(shared_function)).await;
        
        // Example: Store an asynchronous function
        let shared_async_function: Arc<dyn Fn(String) -> BoxFuture<'static, String> + Send + Sync> = Arc::new(|input: String| async move {
          println!("Got in the async function");
          sleep(Duration::from_secs(5)).await;
          "Done".to_string()
        }.boxed());
        self.shared_state.insert(&"async_func".to_string(), StateType::FunctionAsync(shared_async_function)).await;
        
        Ok(format!("Processed data with response: {}", response))
    }

   fn get_shared_state(&self) -> Arc<SharedState> {
      Arc::clone(&self.shared_state)
   }
   fn get_communication_line(&self) -> Arc<MultiBus> {
      Arc::clone(&self.communication_line)
   }
   fn get_name(&self) -> String {
      "myhandler".to_string()
   }
   fn new(communication_line: Arc<MultiBus>, shared_state: Arc<SharedState>) -> Self {
      MyHandler {communication_line, shared_state}
   }
}
```

### Using the Handler Macro

For simpler handlers, use the `handler!` macro:

```rust
use manager_handlers::handler;

// Simple handler
handler!(EchoHandler, echo; 
    async fn run(&self, _: String, data: String) -> Result<String, Box<dyn std::error::Error + Send + Sync>>  {
      Ok(format!("Echo from {}: {}", src, data))
    }
);

// Handler with state access
handler!(CounterHandler, counter;
   async fn run(&self, _: String, data: String) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
       let state = self.get_shared_state();
       
       // Increment counter
       let counter = match state.get(&"counter".to_string()).await {
           Some(StateType::Int(val)) => val + 1,
           _ => 1,
       };
       
       state.insert(&"counter".to_string(), StateType::Int(counter)).await;
       Ok(format!("Counter: {}", counter))
   }
);
```

### Manager Configuration

```rust
use manager_handlers::manager::Manager;
use std::collections::HashMap;

#[tokio::main]
async fn main() {
    // Create a new Manager instance
    let mut manager = Manager::new_default();
    
    // Optional: Configure TLS
    manager.with_tls("path/to/cert.pem", "path/to/key.pem", Some("path/to/ca.pem"));
    
    // Optional: Set allowed client certificate names if using client cert auth
    manager.with_allowed_names(vec!["client1".to_string(), "client2".to_string()]);
    
    // Optional: Set API key for authentication
    manager.with_api_key("my-secret-api-key");
    
    // Optional: Configure maximum concurrent requests
    manager.with_max_requests(100);
    
    // Optional: Configure keep-alive timeout
    manager.with_keep_alive(30);
    
    // Register handlers with their replica counts
    manager.add_handler::<MyHandler>("my_handler", 5);
    manager.add_handler::<OtherHandler>("other_handler", 2);
    
    println!("Starting manager...");
    
    // Start the manager
    manager.start().await;
}
```

## Advanced Features

### Redis Integration

Enable distributed pub/sub with Redis:

```rust
// Configure Redis URL
manager.with_redis_url(Some("redis://localhost:6379".to_string()));

// In your handler, use Redis pub/sub
let response = self.publish_redis(
    "message".to_string(), 
    "remote_handler".to_string(), 
    Some(5000) // 5 second timeout
).await;

// Subscribe to Redis topics
let request = self.subscribe_topic_redis("my_topic".to_string()).await?;
```

### File Operations

Implement file handling capabilities:

1. **UploadHandler**: Implement this trait to customize how files are uploaded and stored.
   ```rust
   #[async_trait]
   impl Base for UploadHandler {
       async fn run_stream(&self, src: String, mut stream: Pin<Box<dyn Stream<Item=Bytes> + Send>>, file_name: String, approx_size: usize) -> Result<String, Box<dyn Error + Send + Sync>> {
          todo!()
       }
   }
   ```

2. **DownloadHandler**: Implement this trait to customize how files are downloaded.
   ```rust
   #[async_trait]
   impl Base for DownloadHandler {
       async fn run_file(&self, src: String, filename: String) -> Result<(Box<dyn AsyncRead + Send + Unpin>, u64), Box<dyn Error + Send + Sync>> {
          todo!()    
       }
   }
   ```

3. **MetadataHandler**: Implement this trait to customize how file metadata is retrieved.
   ```rust
   #[async_trait]
   impl Base for MetadataHandler {
       async fn run_metadata(&self, src: String, filename: String) -> Result<String, Box<dyn Error + Send + Sync>> {
          todo!() 
       }
   }
   ```

## API Reference

### HTTP Endpoints

### Handler Endpoints

#### `POST /{handler_name}`

Send a request to a registered handler.

Example:
```bash
curl -X POST http://localhost:8080/my_handler \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer YOUR_API_KEY" \
  -d '{"type":"request","src":"client","data":"hello world"}'
```

### File Operations

#### Upload a File: `POST /stream/upload/{file_name}`

Upload files to the server.

Example:
```bash
curl -X POST http://localhost:8080/stream/upload/example.txt \
  -H "Authorization: Bearer YOUR_API_KEY" \
  --data-binary "@/path/to/local/example.txt"
```

#### Download a File: `GET /stream/download/{file_id}`

Download a previously uploaded file.

Example:
```bash
curl -X GET http://localhost:8080/stream/download/abc123 \
  -H "Authorization: Bearer YOUR_API_KEY" \
  --output downloaded_file.txt
```

#### Retrieve File Metadata: `GET /stream/metadata/{file_id}`

Get metadata for a file.

Example:
```bash
curl -X GET http://localhost:8080/stream/metadata/abc123 \
  -H "Authorization: Bearer YOUR_API_KEY"
```

### System Management

#### Shutdown Server: `POST /shutdown`

Gracefully shut down the server.

Example:
```bash
curl -X POST http://localhost:8080/shutdown \
  -H "Authorization: Bearer YOUR_API_KEY"
```

## Security

### Authentication Methods

1. **API Key Authentication**
   - Set via `with_api_key()`
   - Pass as Bearer token in Authorization header

2. **TLS/SSL Support**
   - Configure with `with_tls()`
   - Optional client certificate verification

3. **Client Certificate Authentication**
   - Specify allowed certificate names with `with_allowed_names()`
   - Requires CA certificate configuration

### Example Security Configuration

```rust
let mut manager = Manager::new_default();

// Enable API key authentication
manager.with_api_key("your-secret-api-key");

// Configure TLS with client certificates
manager.with_tls(
    "path/to/server-cert.pem",
    "path/to/server-key.pem", 
    Some("path/to/ca-cert.pem")
);

// Allow specific client certificates
manager.with_allowed_names(vec![
    "trusted-client-1".to_string(),
    "trusted-client-2".to_string()
]);
```

## Performance Tuning

### Concurrency Settings

```rust
// Limit total concurrent HTTP requests
manager.with_max_requests(1000);

// Configure handler replicas for load distribution
manager.add_handler::<MyHandler>("heavy_processor", 10);

// Set keep-alive for connection reuse
manager.with_keep_alive(60);
```

### Resource Limits

- Maximum payload size: 10 GiB
- Maximum JSON size: 1 GiB
- Default request timeout: 120 seconds
- WebSocket ping interval: 10 seconds

## Error Handling

The framework provides comprehensive error handling:

```rust
// Handler errors are automatically caught and returned
async fn run(&self, src: String, data: String) -> Result<String, Box<dyn Error + Send + Sync>> {
    // Your error will be properly formatted and returned to client
    Err(Box::new(std::io::Error::new(
        std::io::ErrorKind::NotFound,
        "Resource not found"
    )))
}
```

### Common Error Responses

```json
// Handler not found
{
    "status": "error",
    "message": "Handler not found: invalid_handler"
}

// Authentication failure
{
    "status": "error", 
    "message": "Unauthorized"
}

// Internal error
{
    "status": "error",
    "message": "Internal server error: details..."
}
```

## Contributing

Contributions are welcome! Please feel free to submit a Pull Request.

## License

This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details.

## Author

Matei Aruxandei - [stefmatei22@gmail.com](mailto:stefmatei22@gmail.com)
