use crate::core::communication::multibus::{MultiBus, create_bus};
use crate::core::communication::pub_sub;
use crate::{
    core::routes::files::{process_download, process_metadata, process_upload},
    core::routes::general::{process_request, shutdown_from_http, unauthorized},
    core::routes::middleware::authentication_middleware,
    core::security::cert::CustomClientCertVerifier,
    core::states::{RequestRedis, SharedState, StateType},
    core::traits::Base,
};
use actix_web::http::KeepAlive;
use actix_web::{
    App, HttpServer,
    dev::Server,
    middleware::{self, from_fn},
    rt::System,
    web::{self, Data},
};
use futures_util::future;
use rustls::server::WebPkiClientVerifier;
use rustls::server::danger::ClientCertVerifier;
use rustls::{RootCertStore, ServerConfig};
use serde_json::Value;
use std::fs::File;
use std::io::BufReader;
use std::time::Duration;
use std::{
    collections::{HashMap, HashSet},
    pin::Pin,
    sync::{Arc, atomic::AtomicBool},
};
use tokio::{
    spawn,
    sync::{Mutex, Notify, Semaphore},
};

#[derive(Clone)]
pub struct Config {
    pub(crate) api_key: String,
}

/// Manages the lifecycle of the registered handlers, communication lines, and listeners.
pub struct Manager {
    pub(crate) instance: HashMap<String, Arc<Box<dyn Fn() -> Box<dyn Base + Send + Sync> + Send + Sync>>>,
    pub(crate) number_replicas: HashMap<String, i32>,
    pub(crate) communication_line: Arc<MultiBus>,
    pub(crate) listeners: Vec<tokio::task::JoinHandle<()>>,
    pub(crate) nr_requests: i32,
    pub(crate) shared_state: Arc<SharedState>,
    pub(crate) api_key: String,
    pub(crate) cert_path: Option<String>,
    pub(crate) key_path: Option<String>,
    pub(crate) ca_path: Option<String>,
    pub(crate) allowed_names: Option<Vec<String>>,
    pub(crate) keep_alive: Option<i32>,
    pub(crate) activate_debug: bool,
    pub(crate) ignore_server_init: bool,
    pub(crate) redis_url: Option<String>,
}

impl Manager {
    /// Creates a new instance of `Manager`.
    /// # Returns
    /// * A `Manager` with no registered handlers, initialized listeners, and communication line.
    pub fn new(
        nr_requests: i32,
        api_key: String,
        cert_path: Option<String>,
        key_path: Option<String>,
        ca_path: Option<String>,
        allowed_names: Option<Vec<String>>,
        keep_alive: Option<i32>,
        activate_debug: bool,
        redis_url: Option<String>,
    ) -> Manager {
        Manager {
            instance: HashMap::new(),
            number_replicas: HashMap::new(),
            communication_line: create_bus(),
            listeners: Vec::new(),
            nr_requests,
            shared_state: Arc::new(SharedState { elements: Default::default() }),
            api_key,
            cert_path,
            key_path,
            ca_path,
            allowed_names,
            keep_alive,
            activate_debug,
            ignore_server_init: false,
            redis_url,
        }
    }

    /// Creates a new `Manager` with default settings.
    ///
    /// This factory method initializes a `Manager` with:
    /// * Empty handler registry
    /// * No replicas configured
    /// * Default communication bus
    /// * Single request concurrency (nr_requests = 1)
    /// * Empty shared state
    /// * No API key authentication
    /// * No TLS configuration
    /// * Default keep-alive settings
    ///
    /// # Returns
    /// * A new `Manager` instance with default settings
    ///
    /// # Examples
    /// ```
    /// use manager_handlers::manager::Manager;
    ///
    /// let manager = Manager::new_default();
    /// ```
    pub fn new_default() -> Manager {
        Manager {
            instance: HashMap::new(),
            number_replicas: HashMap::new(),
            communication_line: create_bus(),
            listeners: Vec::new(),
            nr_requests: 1,
            shared_state: Arc::new(SharedState { elements: Default::default() }),
            api_key: "".to_string(),
            cert_path: None,
            key_path: None,
            ca_path: None,
            allowed_names: None,
            keep_alive: None,
            activate_debug: false,
            ignore_server_init: false,
            redis_url: None,
        }
    }

    /// Configures TLS settings for secure HTTPS connections.
    ///
    /// Sets the certificate, private key, and optional CA certificate paths for TLS.
    /// When these are set, the server will use HTTPS instead of HTTP.
    ///
    /// # Arguments
    /// * `cert_path` - Path to the server's TLS certificate file in PEM format
    /// * `key_path` - Path to the server's private key file in PEM format
    /// * `ca_path` - Optional path to the CA certificate used for client certificate verification
    ///
    /// # Examples
    /// ```
    /// use manager_handlers::manager::Manager;
    ///
    /// let mut manager = Manager::new_default();
    /// // Enable TLS with server cert and key
    /// manager.with_tls(
    ///     Some("path/to/cert.pem".to_string()),
    ///     Some("path/to/key.pem".to_string()),
    ///     None
    /// );
    ///
    /// // Enable TLS with client certificate verification
    /// manager.with_tls(
    ///     Some("path/to/cert.pem".to_string()),
    ///     Some("path/to/key.pem".to_string()),
    ///     Some("path/to/ca.pem".to_string())
    /// );
    /// ```
    pub fn with_tls(&mut self, cert_path: Option<String>, key_path: Option<String>, ca_path: Option<String>) {
        self.cert_path = cert_path;
        self.key_path = key_path;
        self.ca_path = ca_path;
    }

    /// Configures allowed client certificate Common Names (CNs) for authentication.
    ///
    /// When TLS is configured with client certificate verification, this method sets
    /// which certificate CNs are allowed to connect. If not set, all valid client
    /// certificates will be accepted.
    ///
    /// # Arguments
    /// * `allowed_names` - Optional vector of allowed certificate Common Names
    ///
    /// # Examples
    /// ```
    /// use manager_handlers::manager::Manager;
    ///
    /// let mut manager = Manager::new_default();
    /// manager.with_tls(
    ///     Some("path/to/cert.pem".to_string()),
    ///     Some("path/to/key.pem".to_string()),
    ///     Some("path/to/ca.pem".to_string())
    /// );
    /// // Only allow specific client certificates
    /// manager.with_allowed_names(Some(vec![
    ///     "client1.example.com".to_string(),
    ///     "client2.example.com".to_string()
    /// ]));
    /// ```
    pub fn with_allowed_names(&mut self, allowed_names: Option<Vec<String>>) {
        self.allowed_names = allowed_names;
    }

    /// Sets the HTTP keep-alive timeout in seconds.
    ///
    /// Keep-alive determines how long idle connections are kept open.
    /// Setting a proper value helps balance resource usage and connection efficiency.
    ///
    /// # Arguments
    /// * `keep_alive` - Optional keep-alive timeout in seconds
    ///                  (None uses the default of 5 seconds)
    ///
    /// # Examples
    /// ```
    /// use manager_handlers::manager::Manager;
    ///
    /// let mut manager = Manager::new_default();
    /// // Set keep-alive timeout to 30 seconds
    /// manager.with_keep_alive(Some(30));
    /// ```
    pub fn with_keep_alive(&mut self, keep_alive: Option<i32>) {
        self.keep_alive = keep_alive;
    }

    /// Sets the maximum number of concurrent HTTP requests allowed.
    ///
    /// This configures the global concurrency limit controlling how many requests
    /// the server can process simultaneously across all handlers.
    ///
    /// # Arguments
    /// * `max_requests` - The maximum number of concurrent requests to allow
    ///
    /// # Examples
    /// ```
    /// use manager_handlers::manager::Manager;
    ///
    /// let mut manager = Manager::new_default();
    /// // Allow up to 100 concurrent requests
    /// manager.with_max_requests(100);
    /// ```
    pub fn with_max_requests(&mut self, max_requests: i32) {
        self.nr_requests = max_requests;
    }

    /// Enables or disables debug mode for the manager.
    ///
    /// When debug mode is enabled, additional logging and diagnostic information
    /// will be output during operation.
    ///
    /// # Arguments
    ///
    /// * `debug` - Set to `true` to enable debug mode, `false` to disable
    pub fn with_activate_debug(&mut self, debug: bool) {
        self.activate_debug = debug;
    }

    /// Sets the API key used for authenticating incoming HTTP requests.
    ///
    /// When set, all requests must include this key in the Authorization header
    /// using the Bearer scheme. If not set or empty, authentication is disabled.
    ///
    /// # Arguments
    /// * `api_key` - The API key to use for request authentication
    ///
    /// # Examples
    /// ```
    /// use manager_handlers::manager::Manager;
    ///
    /// let mut manager = Manager::new_default();
    /// // Enable authentication with a specific API key
    /// manager.with_api_key("your-secret-api-key".to_string());
    /// ```
    pub fn with_api_key(&mut self, api_key: String) {
        self.api_key = api_key;
    }

    /// Configures the Redis connection URL for distributed pub/sub messaging.
    ///
    /// When a Redis URL is provided, handlers can use Redis-based pub/sub methods
    /// in addition to the internal message bus. This enables communication across
    /// multiple service instances.
    ///
    /// # Arguments
    ///
    /// * `redis_url` - Redis connection URL (e.g., "redis://localhost:6379")
    pub fn with_redis_url(&mut self, redis_url: String) {
        self.redis_url = Some(redis_url);
    }

    /// Controls whether to skip HTTP server initialization.
    ///
    /// When set to `true`, the manager will not start the HTTP server.
    /// This is useful for testing or when running handlers in non-HTTP contexts.
    ///
    /// # Arguments
    ///
    /// * `ignore_server_init` - Set to `true` to skip server initialization
    pub fn with_ignore_server_init(&mut self, ignore_server_init: bool) {
        self.ignore_server_init = ignore_server_init;
    }

    /// Registers a new handler to the manager.
    ///
    /// # Arguments
    /// * `name` - The name of the handler.
    /// * `nr_replicas` - The number of replicas a handler can have at any time
    ///
    /// # Panics
    /// * Panics if the handler with the same name is already added.
    pub fn add_handler<T>(&mut self, name: &str, nr_replicas: i32)
    where
        T: Base + Send + Sync + 'static,
    {
        if self.instance.contains_key(name) {
            panic!("Can't add handler {} because it is already added", name);
        }
        let communication_line = self.communication_line.clone();
        let shared_state = self.shared_state.clone();
        let factory: Box<dyn Fn() -> Box<dyn Base + Send + Sync> + Send + Sync> = Box::new(move || {
            let communication_line_cln = communication_line.clone();
            let shared_state_cln = shared_state.clone();
            Box::new(T::new(communication_line_cln, shared_state_cln))
        });
        self.instance.insert(name.to_string(), Arc::new(factory));
        self.number_replicas.insert(name.to_string(), nr_replicas);
    }
    /// Adds a value to the global shared state before starting the manager.
    ///
    /// This method allows pre-populating the shared state with values that
    /// handlers can access once the manager starts. This is useful for
    /// configuration values, initial data, or shared functions.
    ///
    /// # Arguments
    ///
    /// * `key` - The key to store the value under
    /// * `state_type` - The value to store, wrapped in a `StateType` enum
    ///
    /// # Example
    ///
    /// ```ignore
    /// manager.add_global_state("config_value", StateType::String("production".to_string())).await;
    /// manager.add_global_state("max_retries", StateType::Int(3)).await;
    /// ```
    pub async fn add_global_state(&mut self, key: &str, state_type: StateType) {
        self.shared_state.insert(&key.to_owned(), state_type).await;
    }
    async fn initialize_handlers(&mut self) {
        for (name, instance) in self.instance.iter() {
            pub_sub::setup_publishing(name.clone(), self.communication_line.clone()).await;
            let communication_line = Arc::clone(&self.communication_line);
            let instance_to_run: Arc<Box<dyn Fn() -> Box<dyn Base + Send + Sync> + Send + Sync>> = Arc::clone(instance);
            let nr_times = self.number_replicas.get(name).unwrap().clone();
            let name = name.clone();

            let handle = spawn(async move {
                let nr_times_cn = nr_times.clone().to_owned();
                let communication_line_clone_2 = communication_line.clone();
                let standby_handlers: Arc<Mutex<Vec<i32>>> = Arc::new(Mutex::new((0..nr_times_cn).collect()));
                let notification = Arc::new(Notify::new());
                let name_cn = name.clone();
                loop {
                    enum From {
                        Http(Option<String>),
                        Redis(RequestRedis),
                    }
                    let instance_to_run_redis = instance_to_run.clone()();
                    let redis_url_state = instance_to_run_redis.get_shared_state().get(&"redis_url".to_owned()).await;
                    let redis_is_configured = matches!(redis_url_state, StateType::String(url) if !url.is_empty());
                    let from_redis: Pin<Box<dyn Future<Output = Result<RequestRedis, _>> + Send>> =
                        if redis_is_configured { Box::pin(instance_to_run_redis.subscribe_topic_redis(name.clone())) } else { Box::pin(future::pending()) };
                    let from_http = communication_line.clone().request_data(name.clone());
                    let data = tokio::select! {
                        data = from_http => From::Http(data),
                        data = from_redis => From::Redis(data.unwrap())
                    };

                    let parsed_data_type;
                    let parsed_data_src;
                    let data_for_instance;
                    let data_type;
                    match data {
                        From::Http(data_str) => {
                            data_type = "http".to_owned();
                            let parsed_data: Value = serde_json::from_str(&data_str.unwrap()).unwrap();
                            data_for_instance = parsed_data["data"].as_str().expect("Should have at least an empty string").to_string();
                            parsed_data_type = parsed_data["type"].as_str().unwrap().to_string();
                            parsed_data_src = parsed_data["src"].as_str().unwrap().to_string();
                        }
                        From::Redis(data_req) => {
                            data_type = "redis".to_owned();
                            data_for_instance = data_req.message;
                            parsed_data_src = data_req.reply_channel;
                            if parsed_data_src.len() == 0 {
                                parsed_data_type = "dispatch".to_owned();
                            } else {
                                parsed_data_type = "publish".to_owned();
                            }
                        }
                    }

                    let standby_handlers_i = standby_handlers.clone();
                    let name_cn_i = name_cn.clone();
                    let communication_line_i = communication_line_clone_2.clone();
                    let instance_run_i = instance_to_run.clone();
                    let notification_i = notification.clone();

                    let mut standby_handlers_lock = standby_handlers_i.lock().await;
                    let mut id = standby_handlers_lock.pop();
                    drop(standby_handlers_lock);
                    if id == None {
                        notification.clone().notified().await;
                        standby_handlers_lock = standby_handlers_i.lock().await;
                        id = standby_handlers_lock.pop();
                        drop(standby_handlers_lock);
                    }
                    let id_unwrapped = id.unwrap();
                    spawn(async move {
                        let result = instance_run_i().run(name_cn_i.clone(), data_for_instance).await;
                        if let Err(e) = result {
                            eprintln!("Errored while running handler {}: {}", name_cn_i, e);
                        } else {
                            let value_to_return = result.unwrap();
                            if data_type == "redis" && parsed_data_type == "publish" {
                                instance_run_i().dispatch_redis(value_to_return.clone(), parsed_data_src.to_string()).await;
                            } else if data_type == "http" && parsed_data_type == "publish" {
                                pub_sub::dispatch(name_cn_i.to_string(), parsed_data_src.to_string(), value_to_return, communication_line_i.clone()).await;
                            }
                        }
                        let mut standby_handlers_lock = standby_handlers_i.lock().await;
                        standby_handlers_lock.push(id_unwrapped);
                        drop(standby_handlers_lock);
                        notification_i.notify_waiters();
                    });
                }
            });
            self.listeners.push(handle);
        }
    }
    /// Starts the manager, initializes the handlers, and launches the Rocket server.
    ///
    /// This method also sets up listeners for incoming messages for each handler.
    ///
    /// # Panics
    /// * Panics if the Rocket server fails to launch.
    pub async fn start(&mut self) {
        println!("Initializing the listeners for the handlers");
        pub_sub::setup_publishing("manager".to_owned(), self.communication_line.clone()).await;

        self.initialize_handlers().await;

        if !self.ignore_server_init {
            println!("Initializing the server listener");

            rustls::crypto::aws_lc_rs::default_provider().install_default().unwrap();

            let mut tls_config: Option<ServerConfig> = None;

            if let Some(cert_path) = self.cert_path.clone() {
                if let Some(key_cert) = self.key_path.clone() {
                    let mut certs_file = BufReader::new(File::open(cert_path).unwrap());
                    let mut key_file = BufReader::new(File::open(key_cert).unwrap());
                    let tls_certs = rustls_pemfile::certs(&mut certs_file).collect::<Result<Vec<_>, _>>().unwrap();
                    let tls_key = rustls_pemfile::rsa_private_keys(&mut key_file).next().unwrap().unwrap();

                    if let Some(ca_path) = self.ca_path.clone() {
                        let mut ca_file = BufReader::new(File::open(ca_path).unwrap());
                        let ca_certs = rustls_pemfile::certs(&mut ca_file).collect::<Result<Vec<_>, _>>().unwrap();
                        let mut root_store = RootCertStore::empty();
                        for cert in ca_certs {
                            root_store.add(cert).unwrap();
                        }

                        let mut verifier: Arc<dyn ClientCertVerifier>;

                        let default_verifier = WebPkiClientVerifier::builder(<Arc<RootCertStore>>::from(root_store))
                            .build()
                            .expect("Failed to create client certificate verifier");

                        verifier = default_verifier;

                        if let Some(allowed_names) = self.allowed_names.clone() {
                            let allowed_names_set: HashSet<String> = HashSet::from_iter(allowed_names);
                            verifier = Arc::new(CustomClientCertVerifier { default_verifier: verifier, allowed_names_set })
                        }

                        tls_config = Some(
                            ServerConfig::builder()
                                .with_client_cert_verifier(verifier)
                                .with_single_cert(tls_certs, rustls::pki_types::PrivateKeyDer::Pkcs1(tls_key))
                                .unwrap(),
                        );
                    } else {
                        tls_config = Some(
                            ServerConfig::builder()
                                .with_no_client_auth()
                                .with_single_cert(tls_certs, rustls::pki_types::PrivateKeyDer::Pkcs1(tls_key))
                                .unwrap(),
                        )
                    }
                }
            }

            let end_notifier = Arc::new(Notify::new());
            let one_request_at_a_time = Arc::new(Semaphore::new(self.nr_requests as usize));
            let has_been_called = Arc::new(AtomicBool::new(false));

            let api_key = Config { api_key: self.api_key.clone() };

            let request_max_per_handler = Arc::new(
                self.number_replicas
                    .iter()
                    .map(|val| (val.0.clone(), Arc::new(Semaphore::new(*val.1 as usize))))
                    .collect::<HashMap<String, Arc<Semaphore>>>(),
            );

            let instance_clone = Arc::new(self.instance.clone());
            let end_notifier_clone = end_notifier.clone();

            let mut keep_al = 5;
            if let Some(keep_time) = self.keep_alive.clone() {
                keep_al = keep_time;
            }

            if self.activate_debug {
                unsafe {
                    std::env::set_var("RUST_LOG", "debug");
                }
            } else {
                unsafe {
                    std::env::set_var("RUST_LOG", "info");
                }
            }
            env_logger::init();
            let server_cfg = HttpServer::new(move || {
                App::new()
                    .app_data(web::PayloadConfig::new(10 * 1024 * 1024 * 1024)) // 10 GiB
                    .app_data(web::JsonConfig::default().limit(1024 * 1024 * 1024)) // 1 GiB
                    .app_data(Data::new(end_notifier_clone.clone()))
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
            })
            .client_request_timeout(Duration::from_secs(0))
            .max_connection_rate(5000)
            .shutdown_timeout(15)
            .keep_alive(KeepAlive::Timeout(Duration::from_secs(keep_al as u64)));

            let server: Server;

            if let Some(tls) = tls_config {
                server = server_cfg.bind_rustls_0_23(("0.0.0.0", 8080), tls).unwrap().run();
            } else {
                server = server_cfg.bind(("0.0.0.0", 8080)).unwrap().run();
            }

            self.shared_state
                .clone()
                .insert(&"redis_url".to_owned(), StateType::String(self.redis_url.clone().unwrap_or("".to_string())))
                .await;

            tokio::select! {
                _ = server => {},
                _ = end_notifier.notified() => {
                    System::current().stop();
                }
            }
        }
    }

    /// Forcefully terminates all listeners managed by the `Manager`.
    ///
    /// This function aborts all tasks without waiting for them to finish, ensuring
    /// an immediate stop of all handlers and listeners.
    pub fn force_finish_all(&mut self) {
        self.listeners.iter().for_each(|elem| {
            elem.abort();
        });
        self.listeners.clear();
    }
}
