use serde_json::{Map, Value};

/// Helper macro for updating the status of a component.
///
/// ### Usage example
///
/// ```
/// use serde_json::{json, Value};
///
/// let status = Arc::new(Mutex::new(Value::Null));
///
/// // Set status as a whole
/// set_status!(status, json!({
///   "system": {
///     "cpu0": {
///       "freq": 2.4,
///       "temp": 47,
///     },
///   },
/// })).unwrap();
///
/// // Update existing properties
/// set_status!(status.system.cpu0.freq, json!(3.2)).unwrap();
///
/// // Add new properties
/// set_status!(status.system.cpu1, json!({
///   "freq": 2.4,
///   "temp": 38,
/// })).unwrap();
/// ```
#[macro_export]
macro_rules! set_status {
    // Case 1: Just a single identifier, e.g., root
    ($status:ident, $value:expr) => {
        if let Ok(mut s) = $status.lock() {
            *s = $value;
            Ok(())
        } else {
            Err(anyhow::anyhow!("Failed to acquire {} lock", stringify!($status)))
        }
    };

    // Case 2: A chain like root.key1.key2
    ($status:ident . $($path:ident).+, $value:expr) => {
        if let Ok(mut s) = $status.lock() {
            let keys = vec![$( stringify!($path) ),+];
            *$crate::macros::_get_or_create_json_path_mut(&mut *s, &keys) = $value;
            Ok(())
        } else {
            Err(anyhow::anyhow!("Failed to acquire {} lock", stringify!($status)))
        }
    };
}

#[doc(hidden)]
pub fn _get_or_create_json_path_mut<'a>(mut value: &'a mut Value, path: &[&str]) -> &'a mut Value {
    for key in path {
        // If value is not a Value::Object, overwrite it with a new object
        if !matches!(value, Value::Object(_)) {
            let mut map = Map::new();
            map.insert((*key).to_string(), Value::Object(Map::new()));
            *value = Value::Object(map);
        };

        // Get or create the new entry
        if let Value::Object(map) = value {
            value = map.entry(*key).or_insert_with(|| Value::Object(Map::new()))
        };
    }
    value
}

/// Helper macro for transparently operating on the [SystemPort] wrapped inside a [GenericSystemPort].
#[macro_export]
macro_rules! with_port {
    ($gen_port:expr, |$port_ident:ident| $body:block) => {
        match $gen_port {
            GenericSystemPort::Int($port_ident) => $body,
            GenericSystemPort::Float($port_ident) => $body,
            GenericSystemPort::Bool($port_ident) => $body,
            GenericSystemPort::Bytes($port_ident) => $body,
            GenericSystemPort::Text($port_ident) => $body,
            GenericSystemPort::Json($port_ident) => $body,
        }
    };
}
