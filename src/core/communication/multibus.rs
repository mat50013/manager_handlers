use serde_json::json;
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use tokio::sync::{Mutex, Notify, RwLock};
use tokio::time::{Duration, interval};

/// A `MultiBus` structure that facilitates asynchronous message passing between
/// different components. It maintains a memory of queued messages and workers
/// available to process them.
pub struct MultiBus {
    /// Tracks the number of available workers.
    pub number_of_workers: Mutex<i64>,
    /// Stores the messages for each address in a queue.
    pub memory: RwLock<HashMap<String, VecDeque<String>>>,
    /// Contains notification mechanisms for each address to notify when data is available.
    pub notifiers: RwLock<HashMap<String, Arc<Notify>>>,
}

impl MultiBus {
    /// Initializes the message queue and notifier for a given address.
    ///
    /// # Arguments
    /// * `address` - The address (or topic) to initialize in the memory and notifier map.
    pub async fn initialize(self: Arc<Self>, address: String) {
        let mut safety_time = interval(Duration::from_millis(70));
        let mut memory = self.memory.write().await;
        let mut notifiers = self.notifiers.write().await;

        notifiers.insert(address.clone(), Arc::new(Notify::new()));
        memory.insert(address.clone(), VecDeque::new());

        drop(memory);
        safety_time.tick().await;
    }

    /// Removes an address from the bus.
    ///
    /// # Arguments
    /// * `address` - The address (or topic) to be removed.
    pub async fn remove(self: Arc<Self>, address: String) {
        let mut memory = self.memory.write().await;
        let mut notifiers = self.notifiers.write().await;

        notifiers.remove(&*address.clone());
        memory.remove(&*address.clone());
        drop(memory);
        drop(notifiers);
    }

    /// Sends data to a given address. It first checks if workers are available
    /// to process the message and then pushes the message into the memory queue.
    /// Afterward, it notifies waiters that new data is available.
    ///
    /// # Arguments
    /// * `data` - The data/message to send.
    /// * `address` - The address/topic to which the message is sent.
    /// * `src` - The source of the message (typically the sender's identifier).
    /// * `kind` - The type of message (e.g., `publish`, `dispatch`).
    pub async fn send_data(self: Arc<Self>, data: String, address: String, src: String, kind: String) {
        let mut safety_time = interval(Duration::from_millis(90));

        // Wait until a worker becomes available
        while *self.number_of_workers.lock().await == 0 {
            safety_time.tick().await;
        }

        let data_to_insert = json!({
            "src": src.to_string(),
            "data": data.to_string(),
            "type": kind.to_owned()
        })
        .to_string();

        // Decrease the number of available workers
        let mut inf_nr = self.number_of_workers.lock().await;
        *inf_nr -= 1;

        // Push data to the memory queue for the given address
        let mut memory = self.memory.write().await;
        memory.entry(address.clone()).or_default().push_front(data_to_insert.clone());
        drop(memory);

        // Notify any waiters on this address
        let notifiers = self.notifiers.write().await;
        let notif_feature = notifiers.get(&address).unwrap().clone();
        drop(notifiers);
        notif_feature.notify_waiters();
    }

    /// Requests data from the queue for a given address. It checks if there is data
    /// in the queue and, if not, waits for a notification.
    ///
    /// # Arguments
    /// * `address` - The address/topic to request data from.
    ///
    /// # Returns
    /// * `Option<String>` - Returns the next available message, or `None` if no message is available.
    pub async fn request_data(self: Arc<Self>, address: String) -> Option<String> {
        let memory_data = self.memory.read().await;
        let nr_req = memory_data.get(&address).unwrap().len();
        drop(memory_data);

        // If there are messages, return one
        if nr_req > 0 {
            return self.get_one_request(&address).await;
        }

        // Wait for notification of new data
        let notifiers = self.notifiers.read().await;
        let notif_feature = notifiers.get(&address).unwrap().clone();
        drop(notifiers);
        notif_feature.notified().await;

        self.get_one_request(&address).await
    }

    /// Helper function to retrieve one message from the queue.
    ///
    /// # Arguments
    /// * `address` - The address/topic from which to retrieve the message.
    ///
    /// # Returns
    /// * `Option<String>` - The next message in the queue, or `None` if empty.
    async fn get_one_request(self: Arc<Self>, address: &str) -> Option<String> {
        let mut memory_data = self.memory.write().await;
        let result = memory_data.entry(address.to_string()).or_default().pop_back();
        drop(memory_data);

        // Increment the number of available workers
        let self_clone = self.clone();
        let mut inf_nr = self_clone.number_of_workers.lock().await;
        *inf_nr += 1;

        result
    }
}

/// Creates and initializes a new `MultiBus` instance with 10 workers, an empty memory
/// store, and no notifiers.
///
/// # Returns
/// * `Arc<MultiBus>` - A new `MultiBus` instance wrapped in an `Arc`.
pub fn create_bus() -> Arc<MultiBus> {
    let hsh_mp: RwLock<HashMap<String, VecDeque<String>>> = RwLock::new(HashMap::new());
    let nr_worker: Mutex<i64> = Mutex::new(10);
    let notifiers: RwLock<HashMap<String, Arc<Notify>>> = RwLock::new(HashMap::new());

    Arc::new(MultiBus { number_of_workers: nr_worker, memory: hsh_mp, notifiers })
}
