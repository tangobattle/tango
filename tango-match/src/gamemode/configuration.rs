//! Reproducible invocation settings, without player assignment or match seed.
use std::collections::BTreeMap;

use bincode::Options;
use serde::{Deserialize, Serialize};

use super::{identity::Identity, Context, Error, Value};

/// Explicit enum tags keep this portable across non-self-describing codecs.
/// Luau callbacks still receive ordinary booleans, numbers and strings.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum OptionValue {
    Boolean(bool),
    Number(f64),
    String(String),
}

impl PartialEq for OptionValue {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Boolean(a), Self::Boolean(b)) => a == b,
            (Self::Number(a), Self::Number(b)) => a.to_bits() == b.to_bits(),
            (Self::String(a), Self::String(b)) => a == b,
            _ => false,
        }
    }
}
impl Eq for OptionValue {}

impl From<Value> for OptionValue {
    fn from(value: Value) -> Self {
        match value {
            Value::Boolean(value) => Self::Boolean(value),
            Value::Number(value) => Self::Number(value),
            Value::String(value) => Self::String(value),
        }
    }
}
impl From<OptionValue> for Value {
    fn from(value: OptionValue) -> Self {
        match value {
            OptionValue::Boolean(value) => Self::Boolean(value),
            OptionValue::Number(value) => Self::Number(value),
            OptionValue::String(value) => Self::String(value),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Configuration {
    pub identity: Identity,
    pub disable_bgm: bool,
    pub options: BTreeMap<String, OptionValue>,
}

const HEADER: &[u8] = b"TGMC\x02";
pub const MAX_CONFIGURATION_BYTES: usize = 64 * 1024;

fn codec() -> impl Options {
    bincode::DefaultOptions::new()
        .with_varint_encoding()
        .with_limit(MAX_CONFIGURATION_BYTES as u64)
}

impl Configuration {
    pub fn new(identity: Identity, disable_bgm: bool, options: BTreeMap<String, Value>) -> Result<Self, Error> {
        let configuration = Self {
            identity,
            disable_bgm,
            options: options.into_iter().map(|(name, value)| (name, value.into())).collect(),
        };
        configuration.validate()?;
        Ok(configuration)
    }

    pub fn options(&self) -> BTreeMap<String, Value> {
        self.options
            .iter()
            .map(|(name, value)| (name.clone(), value.clone().into()))
            .collect()
    }

    pub fn validate(&self) -> Result<(), Error> {
        self.identity.validate()?;
        // Leaves room for the full dependency graph in a control-channel packet.
        // Validate before making the callback's copy of these values.
        if self.options.len() > 128
            || self
                .options
                .iter()
                .map(|(name, value)| {
                    name.len().saturating_add(match value {
                        OptionValue::String(value) => value.len(),
                        _ => 8,
                    })
                })
                .sum::<usize>()
                > 16 * 1024
        {
            return Err("gamemode configuration options exceed their size limit".into());
        }
        Context {
            player: 1,
            seed: [0; 16],
            disable_bgm: self.disable_bgm,
            options: self.options(),
        }
        .validate()
    }

    /// Versioned binary payload for replay metadata. Numbers preserve their
    /// exact bits, including signed zero. Both paths validate semantic limits.
    pub fn encode(&self) -> Result<Vec<u8>, Error> {
        self.validate()?;
        let mut output = HEADER.to_vec();
        codec().serialize_into(&mut output, self)?;
        if output.len() > MAX_CONFIGURATION_BYTES {
            return Err("gamemode configuration exceeds 64 KiB".into());
        }
        Ok(output)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, Error> {
        if bytes.len() > MAX_CONFIGURATION_BYTES {
            return Err("gamemode configuration exceeds 64 KiB".into());
        }
        let bytes = bytes
            .strip_prefix(HEADER)
            .ok_or("unsupported gamemode configuration format")?;
        let configuration: Self = codec().deserialize(bytes)?;
        configuration.validate()?;
        Ok(configuration)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gamemode::identity::{Package, Runtime};

    fn configuration() -> Configuration {
        Configuration::new(
            Identity {
                engine: Runtime {
                    name: "test".into(),
                    revision: 1,
                },
                script: Runtime {
                    name: "luau".into(),
                    revision: 1,
                },
                package: Package {
                    name: "game".into(),
                    version: "1.0.0".into(),
                    digest: [1; 32],
                },
                export: "main".into(),
                dependencies: vec![],
                rom: [2; 32],
                environment: [3; 32],
            },
            false,
            BTreeMap::from([
                ("zero".into(), Value::Number(-0.0)),
                ("precise".into(), Value::Number(f64::from_bits(0x3ff0000000000001))),
                ("toggle".into(), Value::Boolean(true)),
                ("choice".into(), Value::String("a\0é日本語".into())),
            ]),
        )
        .unwrap()
    }

    #[test]
    fn binary_configuration_preserves_option_types_and_number_bits() {
        let configuration = configuration();
        let decoded = Configuration::decode(&configuration.encode().unwrap()).unwrap();
        assert_eq!(decoded, configuration);
        let mut changed = decoded.clone();
        changed.options.insert("zero".into(), OptionValue::Number(0.0));
        assert_ne!(changed, decoded);
        let options = decoded.options();
        assert!(matches!(&options["zero"], Value::Number(value) if value.to_bits() == (-0.0f64).to_bits()));
        assert!(matches!(&options["choice"], Value::String(value) if value == "a\0é日本語"));
    }

    #[test]
    fn configuration_decode_rejects_invalid_payloads_and_semantic_limits() {
        let valid = configuration().encode().unwrap();
        for n in 0..valid.len() {
            assert!(Configuration::decode(&valid[..n]).is_err());
        }
        let mut trailing = valid.clone();
        trailing.push(0);
        assert!(Configuration::decode(&trailing).is_err());
        let mut newer = valid;
        newer[4] += 1;
        assert!(Configuration::decode(&newer).is_err());
        assert!(Configuration::decode(&vec![0; MAX_CONFIGURATION_BYTES + 1]).is_err());
        for mutate in [
            |c: &mut Configuration| {
                c.options.insert("n".into(), OptionValue::Number(f64::NAN));
            },
            |c: &mut Configuration| {
                c.options.insert("n".into(), OptionValue::Number(f64::INFINITY));
            },
            |c: &mut Configuration| {
                c.options.insert("".into(), OptionValue::Boolean(false));
            },
            |c: &mut Configuration| {
                c.options.insert("large".into(), OptionValue::String("x".repeat(4097)));
            },
            |c: &mut Configuration| {
                c.options = (0..5)
                    .map(|n| (n.to_string(), OptionValue::String("x".repeat(4096))))
                    .collect();
            },
            |c: &mut Configuration| {
                c.identity.export = "../mode".into();
            },
        ] {
            let mut invalid = configuration();
            mutate(&mut invalid);
            assert!(invalid.encode().is_err());
            // Bypass the producer validation to exercise an untrusted decoder.
            let mut bytes = HEADER.to_vec();
            codec().serialize_into(&mut bytes, &invalid).unwrap();
            assert!(Configuration::decode(&bytes).is_err());
        }
    }
}
