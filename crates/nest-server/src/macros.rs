use serde_json::{Map, Value};

/// Helper macro for updating the status of a component.
///
/// ### Usage example
///
/// ```
/// use serde_json::{json, Value};
/// use std::sync::{Arc, Mutex};
/// use nest_server::set_status;
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
            $any_type::Int($wrapper) => {
                #[allow(dead_code)]
                type M = $crate::ports::IntMessage;
                $body
            }
            $any_type::Float($wrapper) => {
                #[allow(dead_code)]
                type M = $crate::ports::FloatMessage;
                $body
            }
            $any_type::Bool($wrapper) => {
                #[allow(dead_code)]
                type M = $crate::ports::BoolMessage;
                $body
            }
            $any_type::Bytes($wrapper) => {
                #[allow(dead_code)]
                type M = $crate::ports::BytesMessage;
                $body
            }
            $any_type::Text($wrapper) => {
                #[allow(dead_code)]
                type M = $crate::ports::TextMessage;
                $body
            }
            $any_type::I2c($wrapper) => {
                #[allow(dead_code)]
                type M = $crate::ports::I2cMessage;
                $body
            }
        }
    };
}

/// Helper macro for transparently operating on the environment variable wrapper inside a type
/// erased struct.
#[macro_export]
macro_rules! with_env_var_wrapper {
    ($any_type:ident, $any_enum:expr, |$wrapper:ident| $body:block) => {
        match $any_enum {
            $any_type::Int($wrapper) => {
                #[allow(dead_code)]
                type M = $crate::environment::IntVar;
                $body
            }
            $any_type::Float($wrapper) => {
                #[allow(dead_code)]
                type M = $crate::environment::FloatVar;
                $body
            }
            $any_type::Bool($wrapper) => {
                #[allow(dead_code)]
                type M = $crate::environment::BoolVar;
                $body
            }
            $any_type::Vec2($wrapper) => {
                #[allow(dead_code)]
                type M = $crate::environment::Vec2Var;
                $body
            }
            $any_type::Vec3($wrapper) => {
                #[allow(dead_code)]
                type M = $crate::environment::Vec3Var;
                $body
            }
            $any_type::Quaternion($wrapper) => {
                #[allow(dead_code)]
                type M = $crate::environment::QuaternionVar;
                $body
            }
            $any_type::Tensor($wrapper) => {
                #[allow(dead_code)]
                type M = $crate::environment::TensorVar;
                $body
            }
        }
    };
}

#[doc(hidden)]
#[macro_export]
macro_rules! impl_as_into_methods {
    ($enum_name:ident, $wrapper:ident, $variant:ident, $msg:ty, $method_as:ident, $method_as_mut:ident, $method_into:ident) => {
        impl $enum_name {
            #[doc = concat!("Return a reference to the underlying [`", stringify!($wrapper), "<", stringify!($msg), ">`].\n\n",
                            "Fails if not a [`", stringify!($enum_name), "::", stringify!($variant), "`].")]
            #[allow(private_interfaces, private_bounds)]
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
            #[allow(private_interfaces, private_bounds)]
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
            #[allow(private_interfaces, private_bounds)]
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
        }

        // Automatically implement From to erase type
        impl ::core::convert::From<$wrapper<$msg>> for $enum_name {
            fn from(wrapper: $wrapper<$msg>) -> Self {
                $enum_name::$variant(wrapper)
            }
        }

        // Automatically implement TryFrom to restore type
        impl ::core::convert::TryFrom<$enum_name> for $wrapper<$msg> {
            type Error = anyhow::Error;

            fn try_from(value: $enum_name) -> Result<Self, Self::Error> {
                match value {
                    $enum_name::$variant(wrapper) => Ok(wrapper),
                    _ => Err(anyhow!(
                        "Can't convert {} into {}",
                        stringify!($enum_name::$variant),
                        stringify!($wrapper<$msg>)
                    )),
                }
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
        #[allow(private_interfaces, private_bounds)]
        #[derive(Clone, Debug)]
        pub enum $enum_name {
            Int($wrapper<$crate::ports::IntMessage>),
            Float($wrapper<$crate::ports::FloatMessage>),
            Bool($wrapper<$crate::ports::BoolMessage>),
            Bytes($wrapper<$crate::ports::BytesMessage>),
            Text($wrapper<$crate::ports::TextMessage>),
            I2c($wrapper<$crate::ports::I2cMessage>),
        }

        $crate::impl_as_into_methods!($enum_name, $wrapper, Int, $crate::ports::IntMessage, as_int, as_int_mut, into_int);
        $crate::impl_as_into_methods!($enum_name, $wrapper, Float, $crate::ports::FloatMessage, as_float, as_float_mut, into_float);
        $crate::impl_as_into_methods!($enum_name, $wrapper, Bool, $crate::ports::BoolMessage, as_bool, as_bool_mut, into_bool);
        $crate::impl_as_into_methods!($enum_name, $wrapper, Bytes, $crate::ports::BytesMessage, as_bytes, as_bytes_mut, into_bytes);
        $crate::impl_as_into_methods!($enum_name, $wrapper, Text, $crate::ports::TextMessage, as_text, as_text_mut, into_text);
        $crate::impl_as_into_methods!($enum_name, $wrapper, I2c, $crate::ports::I2cMessage, as_i2c, as_i2c_mut, into_i2c);
    };
}

/// Create a type erased enum for structs generic on environment variable types.
#[macro_export]
macro_rules! any_env_var_enum {
    ($enum_name:ident, $wrapper:ident) => {
        #[doc = concat!("A generic wrapper around [", stringify!($wrapper), "] to erase the underlying environment variable type.\n\n",
                        "Useful when working with heterogeneous [", stringify!($wrapper), "] collections, ",
                        "but not efficient since it requires looking up the underlying type for most operations.")]
        #[derive(Clone, Debug)]
        pub enum $enum_name {
            Int($wrapper<$crate::environment::IntVar>),
            Float($wrapper<$crate::environment::FloatVar>),
            Bool($wrapper<$crate::environment::BoolVar>),
            Vec2($wrapper<$crate::environment::Vec2Var>),
            Vec3($wrapper<$crate::environment::Vec3Var>),
            Quaternion($wrapper<$crate::environment::QuaternionVar>),
            Tensor($wrapper<$crate::environment::TensorVar>),
        }

        $crate::impl_as_into_methods!($enum_name, $wrapper, Int, $crate::environment::IntVar, as_int, as_int_mut, into_int);
        $crate::impl_as_into_methods!($enum_name, $wrapper, Float, $crate::environment::FloatVar, as_float, as_float_mut, into_float);
        $crate::impl_as_into_methods!($enum_name, $wrapper, Bool, $crate::environment::BoolVar, as_bool, as_bool_mut, into_bool);
        $crate::impl_as_into_methods!($enum_name, $wrapper, Vec2, $crate::environment::Vec2Var, as_vec2, as_vec2_mut, into_vec2);
        $crate::impl_as_into_methods!($enum_name, $wrapper, Vec3, $crate::environment::Vec3Var, as_vec3, as_vec3_mut, into_vec3);
        $crate::impl_as_into_methods!($enum_name, $wrapper, Quaternion, $crate::environment::QuaternionVar, as_quaternion, as_quaternion_mut, into_quaternion);
        $crate::impl_as_into_methods!($enum_name, $wrapper, Tensor, $crate::environment::TensorVar, as_tensor, as_tensor_mut, into_tensor);
    };
}
