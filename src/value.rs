use minicbor::Encode;
use serde_json::Value as JsonValue;
use std::collections::BTreeMap;

pub struct Value(pub JsonValue);

impl Encode for Value {
    fn encode<W: minicbor::encode::Write>(
        &self,
        e: &mut minicbor::Encoder<W>,
    ) -> Result<(), minicbor::encode::Error<W::Error>> {
        match &self.0 {
            JsonValue::Null => e.null().map(|_| ()),
            JsonValue::Bool(x) => x.encode(e),
            JsonValue::Number(x) => {
                if x.is_u64() {
                    x.as_u64().unwrap().encode(e)
                } else if x.is_i64() {
                    x.as_i64().unwrap().encode(e)
                } else {
                    x.as_f64().unwrap().encode(e)
                }
            }
            JsonValue::String(x) => x.encode(e),
            JsonValue::Array(x) => x
                .iter()
                .map(|x| Value(x.clone()))
                .collect::<Vec<Value>>()
                .encode(e),
            JsonValue::Object(x) => x
                .iter()
                .map(|(s, v)| (s.clone(), Value(v.clone())))
                .collect::<BTreeMap<String, Value>>()
                .encode(e),
        }
    }

    fn is_nil(&self) -> bool {
        self.0 == JsonValue::Null
    }
}
