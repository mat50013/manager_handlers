use crate::core::communication::multibus::MultiBus;
use crate::core::{states::SharedState, states::StateType, traits::Base};
use crate::handler;
use async_trait::async_trait;
use futures_util::FutureExt;
use futures_util::future::BoxFuture;
use serde_json::json;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::sleep;

handler! {
    MyHandler3, my_handler;
    async fn run(&self, _: String, data: String) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        println!("Received data: {}", data);

        self.shared_state.insert(&"counter".to_string(), StateType::Int(42)).await;

        self.shared_state.insert(&"double".to_string(), StateType::Double(52.5)).await;

        self.shared_state.insert(&"string".to_string(), StateType::String("Hello".to_string())).await;

        let shared_function: Arc<dyn Fn(String) -> String + Sync + Send> = Arc::new(|input: String| -> String {
            input + " pesto"
        });
        self.shared_state.insert(&"sync_func".to_string(), StateType::FunctionSync(shared_function)).await;

        let shared_async_function: Arc<dyn Fn(String) -> BoxFuture<'static, String> + Send + Sync> = Arc::new(|_: String| async move {
            println!("Got in the async function");
            sleep(Duration::from_secs(5)).await;
            "Done".to_string()
        }.boxed());
        self.shared_state.insert(&"async_func".to_string(), StateType::FunctionAsync(shared_async_function)).await;

        let response = self.publish(
            "Important stuff".to_string(),
            "other_handler".to_string(),
        ).await;
        Ok(response)
    },
}

handler! {
    OtherHandler3, other_handler;
    async fn run(&self, _: String, data: String) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        println!("Received data: {}", data);

        let counter: i32 = self.shared_state.get(&"counter".to_string()).await.into();

        let double: f64 = self.shared_state.get(&"double".to_string()).await.into();

        let string: String = self.shared_state.get(&"string".to_string()).await.into();

        let sync_func: Arc<dyn Fn(String) -> String + Sync + Send> = self.shared_state.get(&"sync_func".to_string()).await.into();
        let sync_func_value = sync_func(data.clone());

        let async_func: Arc<dyn Fn(String) -> BoxFuture<'static, String> + Send + Sync> = self.shared_state.get(&"async_func".to_string()).await.into();
        let async_func_value = async_func(data).await;

        let response = json!({
                "counter": counter,
                "sync_func": sync_func_value,
                "async_func": async_func_value,
                "double": double,
                "string": string
            });

        Ok(response.to_string())
    },
}
