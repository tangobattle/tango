//! Serde adapter for owned binary buffers. Text is never coerced to bytes.
use serde::{Deserialize, Serialize};

pub fn serialize<S: serde::Serializer, T: AsRef<[u8]>>(bytes: &T, serializer: S) -> Result<S::Ok, S::Error> {
    bytes.as_ref().serialize(serializer)
}
pub fn deserialize<'de, D: serde::Deserializer<'de>, T: From<Vec<u8>>>(deserializer: D) -> Result<T, D::Error> {
    struct Bytes;
    impl<'de> serde::de::Visitor<'de> for Bytes {
        type Value = Vec<u8>;
        fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("a byte buffer")
        }
        fn visit_bytes<E: serde::de::Error>(self, bytes: &[u8]) -> Result<Self::Value, E> {
            Ok(bytes.to_vec())
        }
        fn visit_byte_buf<E: serde::de::Error>(self, bytes: Vec<u8>) -> Result<Self::Value, E> {
            Ok(bytes)
        }
        fn visit_seq<A: serde::de::SeqAccess<'de>>(self, seq: A) -> Result<Self::Value, A::Error> {
            Vec::<u8>::deserialize(serde::de::value::SeqAccessDeserializer::new(seq))
        }
    }
    deserializer.deserialize_any(Bytes).map(Into::into)
}
