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

/// Helper macro for transparently operating on the message wrapper inside a type erased
/// struct.
#[macro_export]
macro_rules! with_message_wrapper {
    ($any_type:ident, $any_enum:expr, |$wrapper:ident| $body:block) => {
        match $any_enum {
            $any_type::Int($wrapper) => $body,
            $any_type::Float($wrapper) => $body,
            $any_type::Bool($wrapper) => $body,
            $any_type::Bytes($wrapper) => $body,
            $any_type::Text($wrapper) => $body,
            $any_type::Json($wrapper) => $body,
        }
    };
}

#[doc(hidden)]
#[macro_export]
macro_rules! impl_as_into_methods {
    ($enum_name:ident, $wrapper:ident, $variant:ident, $msg:ty, $method_as:ident, $method_as_mut:ident, $method_into:ident) => {
        #[doc = concat!("Return a reference to the underlying [`", stringify!($wrapper), "<", stringify!($msg), ">`].\n\n",
                        "Fails if not a [`", stringify!($enum_name), "::", stringify!($variant), "`].")]
        pub fn $method_as(&self) -> Result<&$wrapper<$msg>> {
            match self {
                $enum_name::$variant(wrapper) => Ok(wrapper),
                _ => Err(anyhow!(
                    "Can't convert {} into {}",
                    stringify!($enum_name::$variant),
                    stringify!($wrapper<$msg>)
                )),
            }
        }

        #[doc = concat!("Return a mutable reference to the underlying [`", stringify!($wrapper), "<", stringify!($msg), ">`].\n\n",
                        "Fails if not a [`", stringify!($enum_name), "::", stringify!($variant), "`].")]
        pub fn $method_as_mut(&mut self) -> Result<&mut $wrapper<$msg>> {
            match self {
                $enum_name::$variant(wrapper) => Ok(wrapper),
                _ => Err(anyhow!(
                    "Can't convert {} into {}",
                    stringify!($enum_name::$variant),
                    stringify!($wrapper<$msg>)
                )),
            }
        }

        #[doc = concat!("Convert into the underlying [`", stringify!($wrapper), "<", stringify!($msg), ">`].\n\n",
                        "Fails if not a [`", stringify!($enum_name), "::", stringify!($variant), "`].")]
        pub fn $method_into(self) -> Result<$wrapper<$msg>> {
            match self {
                $enum_name::$variant(wrapper) => Ok(wrapper),
                _ => Err(anyhow!(
                    "Can't convert {} into {}",
                    stringify!($enum_name::$variant),
                    stringify!($wrapper<$msg>)
                )),
            }
        }
    };
}

/// Create a type erased enum for structs generic on message types.
#[macro_export]
macro_rules! any_message_enum {
    ($enum_name:ident, $wrapper:ident) => {
        #[doc = concat!("A generic wrapper around [", stringify!($wrapper), "] to erase the underlying message type.\n\n",
                        "Useful when working with heterogeneous [", stringify!($wrapper), "] collections, ",
                        "but not efficient since it requires looking up the underlying type for most operations.")]
        #[derive(Clone, Debug)]
        pub enum $enum_name {
            Int($wrapper<IntMessage>),
            Float($wrapper<FloatMessage>),
            Bool($wrapper<BoolMessage>),
            Bytes($wrapper<BytesMessage>),
            Text($wrapper<TextMessage>),
            Json($wrapper<JsonMessage>),
        }

        impl $enum_name {
            $crate::impl_as_into_methods!($enum_name, $wrapper, Int, IntMessage, as_int, as_int_mut, into_int);
            $crate::impl_as_into_methods!($enum_name, $wrapper, Float, FloatMessage, as_float, as_float_mut, into_float);
            $crate::impl_as_into_methods!($enum_name, $wrapper, Bool, BoolMessage, as_bool, as_bool_mut, into_bool);
            $crate::impl_as_into_methods!($enum_name, $wrapper, Bytes, BytesMessage, as_bytes, as_bytes_mut, into_bytes);
            $crate::impl_as_into_methods!($enum_name, $wrapper, Text, TextMessage, as_text, as_text_mut, into_text);
            $crate::impl_as_into_methods!($enum_name, $wrapper, Json, JsonMessage, as_json, as_json_mut, into_json);
        }
    };
}
