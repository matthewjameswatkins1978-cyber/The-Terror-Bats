//! Strict YAML → JSON conversion.
//!
//! Behaves like deserialising into `serde_json::Value`, except every mapping
//! key must be a string. Non-string keys are rejected with a human-readable
//! error instead of being silently stringified by the JSON layer.
//!
//! This runs entirely through `serde-saphyr`'s own deserializer (no second
//! YAML library): duplicate-key and multi-document rejection, anchor
//! resolution, and scalar handling are inherited unchanged.

use std::fmt;

use serde::de::Error as _;
use serde::de::{Deserialize, Deserializer, MapAccess, SeqAccess, Visitor};
use serde_json::{Map, Value};

/// A YAML document in which all mapping keys are strings.
pub struct StrictValue(pub Value);

impl<'de> Deserialize<'de> for StrictValue {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_any(ValueVisitor).map(StrictValue)
    }
}

struct ValueVisitor;

impl<'de> Visitor<'de> for ValueVisitor {
    type Value = Value;

    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("a YAML scalar, sequence, or mapping with string keys")
    }

    fn visit_bool<E>(self, v: bool) -> Result<Value, E>
    where
        E: serde::de::Error,
    {
        Ok(Value::Bool(v))
    }

    fn visit_i64<E>(self, v: i64) -> Result<Value, E>
    where
        E: serde::de::Error,
    {
        Ok(Value::Number(v.into()))
    }

    fn visit_u64<E>(self, v: u64) -> Result<Value, E>
    where
        E: serde::de::Error,
    {
        Ok(Value::Number(v.into()))
    }

    fn visit_f64<E>(self, v: f64) -> Result<Value, E>
    where
        E: serde::de::Error,
    {
        match serde_json::Number::from_f64(v) {
            Some(n) => Ok(Value::Number(n)),
            None => Err(E::custom("non-finite numbers cannot be represented")),
        }
    }

    fn visit_str<E>(self, v: &str) -> Result<Value, E>
    where
        E: serde::de::Error,
    {
        Ok(Value::String(v.to_owned()))
    }

    fn visit_string<E>(self, v: String) -> Result<Value, E>
    where
        E: serde::de::Error,
    {
        Ok(Value::String(v))
    }

    fn visit_borrowed_str<E>(self, v: &'de str) -> Result<Value, E>
    where
        E: serde::de::Error,
    {
        Ok(Value::String(v.to_owned()))
    }

    fn visit_none<E>(self) -> Result<Value, E>
    where
        E: serde::de::Error,
    {
        Ok(Value::Null)
    }

    fn visit_unit<E>(self) -> Result<Value, E>
    where
        E: serde::de::Error,
    {
        Ok(Value::Null)
    }

    fn visit_some<D>(self, deserializer: D) -> Result<Value, D::Error>
    where
        D: Deserializer<'de>,
    {
        StrictValue::deserialize(deserializer).map(|v| v.0)
    }

    fn visit_seq<A>(self, mut seq: A) -> Result<Value, A::Error>
    where
        A: SeqAccess<'de>,
    {
        let mut items = Vec::new();
        while let Some(item) = seq.next_element::<StrictValue>()? {
            items.push(item.0);
        }
        Ok(Value::Array(items))
    }

    fn visit_map<A>(self, mut map: A) -> Result<Value, A::Error>
    where
        A: MapAccess<'de>,
    {
        let mut object = Map::new();
        while let Some((key, value)) = map.next_entry::<StrictKey, StrictValue>()? {
            object.insert(key.0, value.0);
        }
        Ok(Value::Object(object))
    }
}

/// A YAML mapping key. Only strings are accepted.
struct StrictKey(String);

impl<'de> Deserialize<'de> for StrictKey {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_any(KeyVisitor).map(StrictKey)
    }
}

struct KeyVisitor;

impl<'de> Visitor<'de> for KeyVisitor {
    type Value = String;

    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("a string mapping key")
    }

    fn visit_str<E>(self, v: &str) -> Result<String, E>
    where
        E: serde::de::Error,
    {
        Ok(v.to_owned())
    }

    fn visit_string<E>(self, v: String) -> Result<String, E>
    where
        E: serde::de::Error,
    {
        Ok(v)
    }

    fn visit_borrowed_str<E>(self, v: &'de str) -> Result<String, E>
    where
        E: serde::de::Error,
    {
        Ok(v.to_owned())
    }

    fn visit_bool<E>(self, v: bool) -> Result<String, E>
    where
        E: serde::de::Error,
    {
        Err(E::custom(format!(
            "YAML mapping keys must be strings; found boolean key `{v}` (quote it to use it as a string)"
        )))
    }

    fn visit_i64<E>(self, v: i64) -> Result<String, E>
    where
        E: serde::de::Error,
    {
        Err(E::custom(format!(
            "YAML mapping keys must be strings; found integer key `{v}` (quote it to use it as a string)"
        )))
    }

    fn visit_u64<E>(self, v: u64) -> Result<String, E>
    where
        E: serde::de::Error,
    {
        Err(E::custom(format!(
            "YAML mapping keys must be strings; found integer key `{v}` (quote it to use it as a string)"
        )))
    }

    fn visit_f64<E>(self, v: f64) -> Result<String, E>
    where
        E: serde::de::Error,
    {
        Err(E::custom(format!(
            "YAML mapping keys must be strings; found float key `{v}` (quote it to use it as a string)"
        )))
    }

    fn visit_none<E>(self) -> Result<String, E>
    where
        E: serde::de::Error,
    {
        Err(E::custom(
            "YAML mapping keys must be strings; found null key (quote it to use it as a string)",
        ))
    }

    fn visit_unit<E>(self) -> Result<String, E>
    where
        E: serde::de::Error,
    {
        Err(E::custom(
            "YAML mapping keys must be strings; found null key (quote it to use it as a string)",
        ))
    }

    fn visit_some<D>(self, deserializer: D) -> Result<String, D::Error>
    where
        D: Deserializer<'de>,
    {
        StrictKey::deserialize(deserializer).map(|k| k.0)
    }

    fn visit_seq<A>(self, _seq: A) -> Result<String, A::Error>
    where
        A: SeqAccess<'de>,
    {
        Err(A::Error::custom(
            "YAML mapping keys must be strings; found a sequence key (compound keys are not allowed)",
        ))
    }

    fn visit_map<A>(self, _map: A) -> Result<String, A::Error>
    where
        A: MapAccess<'de>,
    {
        Err(A::Error::custom(
            "YAML mapping keys must be strings; found a mapping key (compound keys are not allowed)",
        ))
    }
}
