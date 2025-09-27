/// Creates a handler implementation with boilerplate code.
///
/// This macro generates a struct that implements the `Base` trait,
/// automatically providing the required boilerplate for handler creation.
///
/// # Syntax
///
/// ```ignore
/// handler! {
///     StructName, handler_name;
///     async fn run(&self, src: String, data: String) -> Result<String, Box<dyn Error + Send + Sync>> {
///         // Your handler logic here
///     },
///     // Additional methods (optional)
/// }
/// ```
///
/// # Example
///
/// ```ignore
/// handler! {
///     MyHandler, my_handler;
///     async fn run(&self, src: String, data: String) -> Result<String, Box<dyn Error + Send + Sync>> {
///         Ok(format!("Processed: {}", data))
///     }
/// }
/// ```
#[macro_export]
macro_rules! handler {
     { $struct_name: ident, $handler_name: ident; $method_mandatory: item, $($additional_methods: item)* } => {
            pub struct $struct_name {
                communication_line: Arc<MultiBus>,
                shared_state: Arc<SharedState>
            }
            #[async_trait]
            impl Base for $struct_name {
                fn new(communication_line: Arc<MultiBus>, shared_state: Arc<SharedState>) -> Self {
                    $struct_name {
                        communication_line,
                        shared_state
                    }
                }
                fn get_name(&self) -> String {
                    stringify!($handler_name).to_string()
                }
                fn get_communication_line(&self) -> Arc<MultiBus> {
                    self.communication_line.clone()
                }
                fn get_shared_state(&self) -> Arc<SharedState> {
                    self.shared_state.clone()
                }
                $method_mandatory
            }
            impl $struct_name {
                $($additional_methods)*
            }
     };
 }
