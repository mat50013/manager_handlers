use crate::core::communication::multibus::MultiBus;
use crate::core::{states::SharedState, traits::Base};
use crate::handler;
use async_trait::async_trait;
use std::sync::Arc;

handler! {
    RedisService1, redis_one;
    async fn run(&self, _: String, data: String) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        println!("Received data: {}", data);

        let response = self.publish_redis("Important stuff".to_string(), "redis_two".to_string(), Some(5)).await;

        Ok(response)
    },
}

handler! {
    RedisService2, redis_two;
    async fn run(&self, _: String, data: String) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        println!("Received data: {}", data);

        Ok("Hi from two".to_owned())
    },
}
