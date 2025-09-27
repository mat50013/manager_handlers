use crate::core::communication::multibus::create_bus;
use crate::core::{states::SharedState, states::StateType, traits::Base};
use crate::manager::{Config, Manager};
use crate::tests::data::test_2::{MyHandler, OtherHandler};
use crate::tests::data::test_3::{MyHandler3, OtherHandler3};
use crate::tests::data::test_5::{DownloadHandler, MetadataHandler, UploadHandler};
use crate::tests::data::test_6::{RedisService1, RedisService2};
use crate::{
    core::routes::files::{process_download, process_metadata, process_upload},
    core::routes::general::{process_request, shutdown_from_http, unauthorized},
    core::routes::middleware::authentication_middleware,
};
use actix_web::body::{MessageBody, to_bytes};
use actix_web::dev::{ServiceFactory, ServiceRequest, ServiceResponse};
use actix_web::http::StatusCode;
use actix_web::middleware::from_fn;
use actix_web::web::Data;
use actix_web::{App, Error, middleware, test, web};
use bytes::Bytes;
use futures_util::stream;
use serde_json::Value;
use std::collections::HashMap;
use std::pin::Pin;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Once};
use tokio::sync::{Notify, Semaphore};
use tokio_stream::Stream;

static INIT_LOGGER: Once = Once::new();

fn init_logger() {
    INIT_LOGGER.call_once(|| {
        let _ = env_logger::builder().is_test(true).try_init();
    });
}

fn create_app(
    number_of_replicas: HashMap<String, i32>,
    instance: HashMap<String, Arc<Box<dyn Fn() -> Box<dyn Base + Send + Sync> + Send + Sync>>>,
) -> App<impl ServiceFactory<ServiceRequest, Config = (), Response = ServiceResponse<impl MessageBody>, Error = Error, InitError = ()>> {
    let request_max_per_handler = Arc::new(
        number_of_replicas
            .iter()
            .map(|val| (val.0.clone(), Arc::new(Semaphore::new(*val.1 as usize))))
            .collect::<HashMap<String, Arc<Semaphore>>>(),
    );

    let end_notifier = Arc::new(Notify::new());
    let one_request_at_a_time = Arc::new(Semaphore::new(20));
    let has_been_called = Arc::new(AtomicBool::new(false));
    let api_key = Config { api_key: "abc".to_string() };
    let instance_clone = Arc::new(instance.clone());

    App::new()
        .app_data(web::PayloadConfig::new(10 * 1024 * 1024 * 1024))
        .app_data(web::JsonConfig::default().limit(1024 * 1024 * 1024))
        .app_data(Data::new(end_notifier.clone()))
        .app_data(Data::new(one_request_at_a_time.clone()))
        .app_data(Data::new(request_max_per_handler.clone()))
        .app_data(Data::new(has_been_called.clone()))
        .app_data(Data::new(api_key.clone()))
        .app_data(Data::new(instance_clone.clone()))
        .wrap(from_fn(authentication_middleware))
        .wrap(middleware::Logger::default())
        .service(web::resource("/shutdown").to(shutdown_from_http))
        .service(web::resource("/{handler_name}").to(process_request))
        .service(
            web::scope("/stream")
                .service(web::resource("/upload/{file_name}").to(process_upload))
                .service(web::resource("/download/{file_id}").to(process_download))
                .service(web::resource("/metadata/{file_id}").to(process_metadata)),
        )
        .default_service(web::route().to(unauthorized))
}

/// Test case 1 - Shutdown
#[actix_web::test]
async fn test_shutdown() {
    let request_max_per_handler: HashMap<String, i32> = HashMap::new();
    let instance: HashMap<String, Arc<Box<dyn Fn() -> Box<dyn Base + Send + Sync> + Send + Sync>>> = HashMap::new();
    unsafe {
        std::env::set_var("RUST_LOG", "debug");
    }
    init_logger();
    let app = test::init_service(create_app(request_max_per_handler.clone(), instance.clone())).await;
    let req = test::TestRequest::post().uri("/shutdown").insert_header(("Authorization", "ab")).to_request();

    let resp = test::call_and_read_body(&app, req).await;

    assert_eq!(String::from_utf8(resp.to_vec()).unwrap(), "{\"status\":\"Shutting down\"}");
}

/// Test case 2 - Communication between handlers
#[actix_web::test]
async fn test_communication_handlers() {
    let mut manager = Manager::new_default();
    manager.with_api_key("abc".to_string());
    manager.with_max_requests(20);
    manager.with_ignore_server_init(true);
    manager.add_handler::<MyHandler>("my_handler", 1);
    manager.add_handler::<OtherHandler>("other_handler", 5);
    manager.start().await;

    unsafe {
        std::env::set_var("RUST_LOG", "debug");
    }
    init_logger();
    let app = test::init_service(create_app(manager.number_replicas.clone(), manager.instance.clone())).await;
    let req = test::TestRequest::post()
        .uri("/my_handler")
        .insert_header(("Authorization", "abc"))
        .set_payload("random_string_body")
        .to_request();

    let resp = test::call_and_read_body(&app, req).await;

    assert_eq!(
        String::from_utf8(resp.to_vec()).unwrap(),
        r#"{"message":"Processed data with response: \"Hi back\"","status":"Ok"}"#
    );
}

/// Test case 3 - Shared data works with different types
#[actix_web::test]
async fn test_shared_data() {
    let mut manager = Manager::new_default();
    manager.with_api_key("abc".to_string());
    manager.with_max_requests(20);
    manager.with_ignore_server_init(true);
    manager.add_handler::<OtherHandler3>("other_handler", 1);
    manager.add_handler::<MyHandler3>("my_handler", 1);
    manager.start().await;

    unsafe {
        std::env::set_var("RUST_LOG", "debug");
    }
    init_logger();
    let app = test::init_service(create_app(manager.number_replicas.clone(), manager.instance.clone())).await;
    let req = test::TestRequest::post()
        .uri("/my_handler")
        .insert_header(("Authorization", "abc"))
        .set_payload("something")
        .to_request();

    let resp = test::call_and_read_body(&app, req).await;
    let actual_str = String::from_utf8(resp.to_vec()).unwrap();

    assert_eq!(
        actual_str,
        r#"{"message":"\"{\\\"async_func\\\":\\\"Done\\\",\\\"counter\\\":42,\\\"double\\\":52.5,\\\"string\\\":\\\"Hello\\\",\\\"sync_func\\\":\\\"Important stuff pesto\\\"}\"","status":"Ok"}"#
    );
}

/// Test case 4 - Reject invalid api
#[actix_web::test]
async fn test_invalid_api() {
    let mut manager = Manager::new_default();
    manager.with_api_key("abc".to_string());
    manager.with_max_requests(20);
    manager.with_ignore_server_init(true);
    manager.add_handler::<MyHandler>("my_handler", 1);
    manager.start().await;

    unsafe {
        std::env::set_var("RUST_LOG", "debug");
    }
    init_logger();
    let app = test::init_service(create_app(manager.number_replicas.clone(), manager.instance.clone())).await;
    let req = test::TestRequest::post()
        .uri("/my_handler")
        .insert_header(("Authorization", "ab"))
        .set_payload("random_string_body")
        .to_request();

    let resp = test::try_call_service(&app, req).await;

    assert_eq!(resp.err().unwrap().to_string(), "Invalid API Key");
}

/// Test case 5 - Test download, upload, metadata
#[actix_web::test]
async fn test_metadata() {
    let mut manager = Manager::new_default();
    manager.with_api_key("abc".to_string());
    manager.with_max_requests(20);
    manager.with_ignore_server_init(true);
    manager.add_handler::<MetadataHandler>("metadata", 1);
    manager.start().await;

    unsafe {
        std::env::set_var("RUST_LOG", "debug");
    }
    init_logger();
    let app = test::init_service(create_app(manager.number_replicas.clone(), manager.instance.clone())).await;
    let req = test::TestRequest::post()
        .uri("/stream/metadata/132")
        .insert_header(("Authorization", "abc"))
        .set_payload("random_string_body")
        .to_request();

    let resp = test::call_and_read_body(&app, req).await;
    let actual_str = String::from_utf8(resp.to_vec()).unwrap();

    assert_eq!(actual_str, r#"{"message":"metadata","status":"Ok"}"#);
}

#[actix_web::test]
async fn test_download() {
    let mut manager = Manager::new_default();
    manager.with_api_key("abc".to_string());
    manager.with_max_requests(20);
    manager.with_ignore_server_init(true);
    manager.add_handler::<DownloadHandler>("download", 1);
    manager.start().await;

    unsafe {
        std::env::set_var("RUST_LOG", "debug");
    }
    init_logger();
    let app = test::init_service(create_app(manager.number_replicas.clone(), manager.instance.clone())).await;
    let req = test::TestRequest::post()
        .uri("/stream/download/132")
        .insert_header(("Authorization", "abc"))
        .set_payload("random_string_body")
        .to_request();

    let resp = test::call_service(&app, req).await;

    assert_eq!(resp.status(), StatusCode::OK);

    let body = to_bytes(resp.into_body()).await;
    if body.is_err() {
        panic!("Error reading body");
    }
    if let Ok(body) = body {
        let actual_str = String::from_utf8(body.to_vec()).unwrap();
        assert_eq!(actual_str, "test data");
    }
}

#[actix_web::test]
async fn test_upload() {
    let data_chunks = vec![Bytes::from("Hello, "), Bytes::from("this is "), Bytes::from("a test stream.")];
    let stream = Box::pin(stream::iter(data_chunks.into_iter())) as Pin<Box<dyn Stream<Item = Bytes> + Send>>;

    let multi_bus = create_bus();
    let shared_data = Arc::new(SharedState { elements: Default::default() });

    let result = UploadHandler::new(multi_bus, shared_data)
        .run_stream("source".into(), stream, "test.txt".into(), 0)
        .await
        .expect("Handler failed");

    assert_eq!(result, "Hello, this is a test stream.");
}

#[actix_web::test]
async fn test_redis() {
    let mut manager = Manager::new_default();
    manager.add_global_state("redis_url", StateType::String("redis://127.0.0.1/".to_owned())).await;
    manager.with_api_key("abc".to_string());
    manager.with_max_requests(20);
    manager.with_ignore_server_init(true);
    manager.add_handler::<RedisService2>("redis_two", 1);
    manager.add_handler::<RedisService1>("redis_one", 1);
    manager.start().await;

    unsafe {
        std::env::set_var("RUST_LOG", "debug");
    }
    init_logger();
    let app = test::init_service(create_app(manager.number_replicas.clone(), manager.instance.clone())).await;
    let req = test::TestRequest::post()
        .uri("/redis_one")
        .insert_header(("Authorization", "abc"))
        .set_payload("something")
        .to_request();

    let resp = test::call_and_read_body(&app, req).await;
    let actual_str = String::from_utf8(resp.to_vec()).unwrap();

    let outer: Value = serde_json::from_str(&actual_str).unwrap();
    assert_eq!(outer["status"], "Ok");

    let inner_str = outer["message"].as_str().expect("message must be string");
    let inner: Value = serde_json::from_str(inner_str).expect("inner JSON");

    assert_eq!(inner["message"], "Hi from two");
    assert_eq!(inner["reply_channel"], "");
    assert!(inner.get("id").is_some(), "expected an id field");
}
