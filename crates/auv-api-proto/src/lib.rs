/// Encoded descriptor closure for every schema compiled by this crate.
pub const FILE_DESCRIPTOR_SET: &[u8] = tonic::include_file_descriptor_set!("auv.api");

/// Tonic requires a numeric message ceiling even when the application does not
/// impose one. Use the platform's representable maximum instead of an
/// AUV-specific capture or protobuf admission policy.
pub const GRPC_MESSAGE_SIZE_UNLIMITED: usize = usize::MAX;

fn is_false(value: &bool) -> bool {
  !*value
}

pub(crate) mod json {
  tonic_rest::define_enum_serde!(api_resource_operation, crate::auv::api::daemon::v1::ApiResourceOperation);
  tonic_rest::define_enum_serde!(device_platform, crate::auv::api::daemon::v1::DevicePlatform);
  tonic_rest::define_enum_serde!(run_phase, crate::auv::api::daemon::v1::RunPhase);
  tonic_rest::define_enum_serde!(run_outcome, crate::auv::api::daemon::v1::RunOutcome);
  tonic_rest::define_enum_serde!(runner_lifecycle, crate::auv::api::daemon::v1::RunnerLifecycle);
  tonic_rest::define_enum_serde!(runner_phase, crate::auv::api::daemon::v1::RunnerPhase);

  pub fn is_zero_i32(value: &i32) -> bool {
    *value == 0
  }

  pub fn is_zero_u64(value: &u64) -> bool {
    *value == 0
  }

  pub mod u64_string {
    use serde::de::{self, Visitor};
    use serde::{Deserializer, Serializer};

    pub fn serialize<S: Serializer>(value: &u64, serializer: S) -> Result<S::Ok, S::Error> {
      serializer.serialize_str(&value.to_string())
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<u64, D::Error> {
      struct U64Visitor;

      impl Visitor<'_> for U64Visitor {
        type Value = u64;

        fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
          formatter.write_str("an unsigned 64-bit integer or decimal string")
        }

        fn visit_u64<E: de::Error>(self, value: u64) -> Result<Self::Value, E> {
          Ok(value)
        }

        fn visit_i64<E: de::Error>(self, value: i64) -> Result<Self::Value, E> {
          u64::try_from(value).map_err(E::custom)
        }

        fn visit_str<E: de::Error>(self, value: &str) -> Result<Self::Value, E> {
          value.parse().map_err(E::custom)
        }
      }

      deserializer.deserialize_any(U64Visitor)
    }
  }
}

/// Builds the minimal descriptor closure that owns the named gRPC service.
///
/// Runner reflection must publish only services that the process actually
/// serves. Registering [`FILE_DESCRIPTOR_SET`] directly would advertise every
/// AUV service compiled into this crate.
pub fn descriptor_set_for_service(service_name: &str) -> Result<Vec<u8>, String> {
  descriptor_set_for_services(&[service_name])
}

/// Builds the minimal descriptor closure for an exact set of served services.
pub fn descriptor_set_for_services(service_names: &[&str]) -> Result<Vec<u8>, String> {
  let pool =
    prost_reflect::DescriptorPool::decode(FILE_DESCRIPTOR_SET).map_err(|error| format!("invalid embedded descriptor set: {error}"))?;
  let owners = service_names
    .iter()
    .map(|service_name| {
      pool
        .get_service_by_name(service_name)
        .map(|service| service.parent_file().name().to_string())
        .ok_or_else(|| format!("unknown gRPC service: {service_name}"))
    })
    .collect::<Result<Vec<_>, _>>()?;

  let mut required = std::collections::HashSet::new();
  let mut pending = owners;
  while let Some(name) = pending.pop() {
    if !required.insert(name.clone()) {
      continue;
    }
    let file = pool.get_file_by_name(&name).ok_or_else(|| format!("descriptor dependency is missing: {name}"))?;
    pending.extend(file.file_descriptor_proto().dependency.iter().cloned());
  }

  let mut names = required.into_iter().collect::<Vec<_>>();
  names.sort();
  let files = names
    .into_iter()
    .map(|name| {
      pool.get_file_by_name(&name).map(|file| file.encode_to_vec()).ok_or_else(|| format!("descriptor dependency is missing: {name}"))
    })
    .collect::<Result<Vec<_>, _>>()?;
  Ok(encode_file_descriptor_set(&files))
}

/// Encodes already-validated `FileDescriptorProto` messages without decoding
/// them through `prost_types`, which would discard custom option extensions.
fn encode_file_descriptor_set(files: &[Vec<u8>]) -> Vec<u8> {
  let mut encoded = Vec::new();
  for file in files {
    encoded.push(0x0a); // FileDescriptorSet.file, wire type length-delimited.
    encode_varint(file.len() as u64, &mut encoded);
    encoded.extend_from_slice(file);
  }
  encoded
}

fn encode_varint(mut value: u64, output: &mut Vec<u8>) {
  while value >= 0x80 {
    output.push((value as u8 & 0x7f) | 0x80);
    value >>= 7;
  }
  output.push(value as u8);
}

/// Package-shaped generated modules. Keeping the Rust module hierarchy aligned
/// with Protobuf packages lets generated cross-package references resolve
/// without extern-path rewrites.
pub mod auv {
  pub mod api {
    pub mod annotations {
      pub mod v1 {
        tonic::include_proto!("auv.api.annotations.v1");
      }
    }

    pub mod daemon {
      pub mod v1 {
        tonic::include_proto!("auv.api.daemon.v1");
      }
    }

    pub mod driver {
      pub mod v1 {
        tonic::include_proto!("auv.api.driver.v1");
      }

      pub mod macos {
        pub mod v1 {
          tonic::include_proto!("auv.api.driver.macos.v1");
        }
      }
    }

    pub mod image {
      pub mod v1 {
        tonic::include_proto!("auv.api.image.v1");
      }
    }

    pub mod transport {
      pub mod websocket {
        pub mod v1 {
          tonic::include_proto!("auv.api.transport.websocket.v1");
        }
      }
    }
  }
}

/// Short compatibility imports used by the existing Rust frontends.
pub mod v1 {
  pub use crate::auv::api::driver::v1 as driver;
  pub use crate::auv::api::image::v1 as image;
}

#[cfg(test)]
#[path = "lib_test.rs"]
mod tests;
