use crate::core::communication::multibus::MultiBus;
use crate::core::{states::SharedState, traits::Base};
use crate::handler;
use async_trait::async_trait;
use std::sync::Arc;

handler! {
    MyHandler, my_handler;
    async fn run(&self, _: String, data: String) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        println!("Received data: {}", data);

        for i in 0..10 {
            self.dispatch(
                format!("Hello {}", i),
                "other_handler".to_string()
            ).await;
        }

        let response = self.publish(
            "Hello".to_string(),
            "other_handler".to_string(),
        ).await;
        Ok(format!("Processed data with response: {}", response))
    },
}

handler! {
    OtherHandler, other_handler;
    async fn run(&self, _: String, data: String) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        println!("Received data: {}", data);

        Ok(format!("Hi back"))
    },
}
