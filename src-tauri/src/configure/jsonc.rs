//! 修改允许注释的 JSON（JSONC）文件：用 jsonc-parser 只改要改的键，其余内容、注释和格式不动。

use jsonc_parser::ParseOptions;
use jsonc_parser::cst::{CstInputValue, CstObject, CstRootNode};
use serde_json::Value;

use super::ConfigError;

/// 解析（空内容当作空文档）。顶层不是对象时算格式有误：不去改它。
pub fn parse(text: Option<&str>) -> Result<CstRootNode, ConfigError> {
    let root = CstRootNode::parse(text.unwrap_or_default(), &ParseOptions::default())
        .map_err(|_| ConfigError::Invalid)?;
    if root.value().is_some() && root.object_value().is_none() {
        return Err(ConfigError::Invalid);
    }
    Ok(root)
}

/// JSON 值转成可以写进 JSONC 的值。
pub fn input_value(value: &Value) -> CstInputValue {
    match value {
        Value::Null => CstInputValue::Null,
        Value::Bool(value) => CstInputValue::Bool(*value),
        Value::Number(value) => CstInputValue::Number(value.to_string()),
        Value::String(value) => CstInputValue::String(value.clone()),
        Value::Array(items) => CstInputValue::Array(items.iter().map(input_value).collect()),
        Value::Object(map) => CstInputValue::Object(
            map.iter()
                .map(|(key, value)| (key.clone(), input_value(value)))
                .collect(),
        ),
    }
}

/// 对象里某个键的值（JSON 文本），用来记原值。
pub fn saved(object: &CstObject, key: &str) -> Option<String> {
    value_at(object, key).map(|value| value.to_string())
}

/// 对象里某个键的值。
pub fn value_at(object: &CstObject, key: &str) -> Option<Value> {
    object
        .get(key)
        .and_then(|prop| prop.value())
        .and_then(|value| value.to_serde_value())
}

pub fn string_at(object: &CstObject, key: &str) -> Option<String> {
    object
        .get(key)
        .and_then(|prop| prop.value())
        .and_then(|value| value.as_string_lit())
        .and_then(|value| value.decoded_value().ok())
}

/// 设置一个键：有就改，没有就加在最后。
pub fn set(object: &CstObject, key: &str, value: CstInputValue) {
    match object.get(key) {
        Some(prop) => prop.set_value(value),
        None => {
            object.append(key, value);
        }
    }
}

/// 放回原值（原来没有就删掉这个键）。
pub fn put_back(object: &CstObject, key: &str, original: Option<&str>) {
    match original.and_then(|text| serde_json::from_str::<Value>(text).ok()) {
        Some(value) => set(object, key, input_value(&value)),
        None => {
            if let Some(prop) = object.get(key) {
                prop.remove();
            }
        }
    }
}

/// 对象空了（没有键）时从父对象里删掉。
pub fn remove_if_empty(parent: &CstObject, key: &str) {
    if parent
        .object_value(key)
        .is_some_and(|object| object.properties().is_empty())
        && let Some(prop) = parent.get(key)
    {
        prop.remove();
    }
}

/// 整个文件只剩一个空对象（或者什么都没有）。
pub fn is_empty(root: &CstRootNode) -> bool {
    root.object_value()
        .is_none_or(|object| object.properties().is_empty())
}
