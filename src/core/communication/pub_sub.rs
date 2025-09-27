use lazy_static::lazy_static;
use std::sync::Arc;
use std::sync::atomic::{AtomicI32, Ordering};

lazy_static! {
    static ref id_pub_channel: AtomicI32 = AtomicI32::new(0);
}
const MOD: i32 = 3571823;

/// Sets up publishing for a given topic by initializing the topic itself
///
/// # Arguments
/// * `topic` - The name of the topic to set up for publishing.
/// * `connection_network` - The `MultiBus` instance used for managing communication channels.
pub async fn setup_publishing(topic: String, connection_network: Arc<crate::core::communication::multibus::MultiBus>) {
    connection_network.clone().initialize(topic.clone()).await;
}

/// Dispatches a message to a specific topic. This sends a message without expecting any response.
///
/// # Arguments
/// * `src` - The source identifier of the message.
/// * `topic` - The topic to which the message should be dispatched.
/// * `message` - The message content to dispatch.
/// * `connection_network` - The `MultiBus` instance used for communication.
pub async fn dispatch(src: String, topic: String, message: String, connection_network: Arc<crate::core::communication::multibus::MultiBus>) {
    connection_network
        .clone()
        .send_data(message.clone(), topic.clone(), src.clone(), "dispatch".to_string())
        .await;
}

/// Publishes a message to a specific topic and waits for a response.
///
/// # Arguments
/// * `src` - The source identifier of the message.
/// * `topic` - The topic to which the message should be published.
/// * `message` - The message content to publish.
/// * `connection_network` - The `MultiBus` instance used for communication.
///
/// # Returns
/// * A `String` containing the response received from the published message.
/// * If no response is received, a default error message is returned.
pub async fn publish(src: String, topic: String, message: String, connection_network: Arc<crate::core::communication::multibus::MultiBus>) -> String {
    let current_id_i32 = id_pub_channel.load(Ordering::SeqCst);
    id_pub_channel.store((current_id_i32 + 1) % MOD, Ordering::SeqCst);
    let current_id_str = current_id_i32.to_string();
    connection_network.clone().initialize(src.clone() + "_pub" + &current_id_str).await;
    connection_network
        .clone()
        .send_data(message.clone(), topic.clone(), src.clone() + "_pub" + &current_id_str, "publish".to_string())
        .await;
    if let Some(response) = connection_network.clone().request_data(src.clone() + "_pub" + &current_id_str).await {
        connection_network.clone().remove(src.clone() + "_pub" + &current_id_str).await;
        return response.clone().to_owned();
    }
    connection_network.clone().remove(src.clone() + "_pub" + &current_id_str).await;
    "Something wrong happened while waiting for response".to_owned()
}
