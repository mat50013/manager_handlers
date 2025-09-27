use futures_util::future::BoxFuture;
use redis::ToRedisArgs;
use serde::{Deserialize, Serialize};
use std::any::Any;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Serialize, Deserialize, Debug)]
pub struct RequestRedis {
    pub(crate) id: String,
    pub(crate) message: String,
    pub(crate) reply_channel: String,
}
impl ToRedisArgs for RequestRedis {
    fn write_redis_args<W>(&self, out: &mut W)
    where
        W: ?Sized + redis::RedisWrite,
    {
        let serialized = serde_json::to_string(self).unwrap();
        serialized.write_redis_args(out);
    }
}

/// Represents the different types of states that can be stored.
/// It can hold primitive types, strings, or even a function wrapped in `Arc` for thread-safe access.
#[derive(Clone)]
pub enum StateType {
    /// Represents an empty or uninitialized state.
    None,
    /// Holds a sync function that takes a `String` as input and returns a `String`.
    FunctionSync(Arc<dyn Fn(String) -> String + Sync + Send>),
    /// Holds an async function that takes a `String` as input and returns a `String`
    FunctionAsync(Arc<dyn Fn(String) -> BoxFuture<'static, String> + Sync + Send>),
    /// Holds a `String` value.
    String(String),
    /// Holds a 32-bit integer.
    Int(i32),
    /// Holds a 32-bit floating-point number.
    Float(f32),
    /// Holds a 64-bit integer.
    Long(i64),
    /// Holds a 64-bit floating-point number.
    Double(f64),
    /// Holds any type which is safe
    AnyType(Arc<dyn Any + Sync + Send>),
}

macro_rules! impl_from_state_type {
    ($type: ty, $variant:ident) => {
        impl From<StateType> for $type {
            fn from(state: StateType) -> Self {
                match state {
                    StateType::$variant(value) => value,
                    _ => panic!("Invalid conversion"),
                }
            }
        }
    };
}
impl_from_state_type!(i32, Int);
impl_from_state_type!(f32, Float);
impl_from_state_type!(i64, Long);
impl_from_state_type!(f64, Double);
impl_from_state_type!(String, String);
impl_from_state_type!(Arc<dyn Fn(String) -> BoxFuture<'static, String> + Sync + Send>, FunctionAsync);
impl_from_state_type!(Arc<dyn Fn(String) -> String + Sync + Send>, FunctionSync);

/// `SharedState` is a thread-safe structure for storing and managing different types of states.
/// It uses an `RwLock` to allow safe concurrent access across threads.
pub struct SharedState {
    /// A `HashMap` that stores key-value pairs, where the key is a `String`
    /// and the value is of type `StateType`.
    pub(crate) elements: RwLock<HashMap<String, StateType>>,
}
impl SharedState {
    /// Retrieves a value from the shared state given a key.
    ///
    /// # Arguments
    /// * `key` - A reference to the key (`String`) used to lookup the value.
    ///
    /// # Returns
    /// * `StateType` - The value associated with the key, or `StateType::None` if the key is not found.
    ///
    /// ```
    pub async fn get(&self, key: &String) -> StateType {
        let elem_lock = self.elements.read().await;
        let result = elem_lock.get(key);
        match result {
            None => StateType::None,
            Some(value) => value.clone(),
        }
    }

    /// Inserts a key-value pair into the shared state.
    ///
    /// # Arguments
    /// * `key` - A reference to the key (`String`) for the value.
    /// * `data` - The `StateType` value to be stored in the shared state.
    ///
    /// ```
    pub async fn insert(&self, key: &String, data: StateType) {
        let mut elem_lock = self.elements.write().await;
        elem_lock.insert(key.clone(), data.clone());
        drop(elem_lock);
    }

    /// Deletes a key-value pair from the shared state.
    ///
    /// # Arguments
    /// * `key` - A reference to the key (`String`) to remove from the shared state.
    ///
    /// ```
    pub async fn delete(&self, key: &String) {
        let mut elem_lock = self.elements.write().await;
        elem_lock.remove(key);
        drop(elem_lock);
    }

    /// Attempts to downcast the stored `AnyType` value to the specified type.
    ///
    /// # Arguments
    /// * `key` - A reference to the key (`String`) used to lookup the value.
    ///
    /// # Returns
    /// * `Option<Arc<T>>` - The downcast value if successful, or `None` if downcast fails.
    pub async fn get_any<T: 'static + Send + Sync>(&self, key: &String) -> Option<Arc<T>> {
        let elem_lock = self.elements.read().await;
        match elem_lock.get(key) {
            Some(StateType::AnyType(value)) => value.clone().downcast::<T>().ok(),
            _ => None,
        }
    }
}
