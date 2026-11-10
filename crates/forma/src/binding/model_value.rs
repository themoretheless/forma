//! Conversions between typed Rust model values and document values.
//!
//! Generated forms use these to move state between Rust structs and Forma expressions.

use std::collections::BTreeMap;

/// Values available to generated expressions. Numeric fields remain typed in Rust.
pub trait ModelValue: Clone + PartialEq + Default + 'static {
    fn to_value(&self) -> crate::form_document::Value;
    fn from_value(value: &crate::form_document::Value) -> Result<Self, String> {
        Err(format!(
            "Cannot decode {value:?} as {}",
            std::any::type_name::<Self>()
        ))
    }
}
pub fn decode<T: ModelValue>(value: &crate::form_document::Value) -> Result<T, String> {
    T::from_value(value)
}

impl ModelValue for String {
    fn to_value(&self) -> crate::form_document::Value {
        crate::form_document::Value::Text(self.clone())
    }
    fn from_value(value: &crate::form_document::Value) -> Result<Self, String> {
        if let crate::form_document::Value::Text(value) = value {
            Ok(value.clone())
        } else {
            Err("Expected String event argument".into())
        }
    }
}
impl<T: ModelValue> ModelValue for Option<T> {
    fn to_value(&self) -> crate::form_document::Value {
        self.as_ref()
            .map(ModelValue::to_value)
            .unwrap_or(crate::form_document::Value::Null)
    }
    fn from_value(value: &crate::form_document::Value) -> Result<Self, String> {
        if matches!(value, crate::form_document::Value::Null) {
            Ok(None)
        } else {
            T::from_value(value).map(Some)
        }
    }
}
impl<T: ModelValue> ModelValue for BTreeMap<String, T> {
    fn to_value(&self) -> crate::form_document::Value {
        crate::form_document::Value::Object(
            self.iter()
                .map(|(k, v)| (k.clone(), v.to_value()))
                .collect(),
        )
    }
}
impl<T: ModelValue> ModelValue for Vec<T> {
    fn to_value(&self) -> crate::form_document::Value {
        crate::form_document::Value::List(self.iter().map(ModelValue::to_value).collect())
    }
}
impl ModelValue for bool {
    fn to_value(&self) -> crate::form_document::Value {
        crate::form_document::Value::Bool(*self)
    }
    fn from_value(value: &crate::form_document::Value) -> Result<Self, String> {
        value.boolean()
    }
}
macro_rules! numbers { ($($t:ty),*) => {$ (impl ModelValue for $t {
    fn to_value(&self)->crate::form_document::Value { crate::form_document::Value::Number(*self as f64) }
    fn from_value(value: &crate::form_document::Value) -> Result<Self, String> {
        let n=value.number()?; let result=n as $t;
        if result as f64 != n { return Err(format!("Number {n} cannot be represented as {}", stringify!($t))); }
        Ok(result)
    }
})*}; }
numbers!(f64, f32, i32, u32, i64, u64, usize);
pub trait ModelNumber: ModelValue {}
macro_rules! numeric {($($t:ty),*)=>{$(impl ModelNumber for $t {})*};}
numeric!(f64, f32, i32, u32, i64, u64, usize);
