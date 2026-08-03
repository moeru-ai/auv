/// Encoded descriptor closure for every schema compiled by this crate.
pub const FILE_DESCRIPTOR_SET: &[u8] = tonic::include_file_descriptor_set!("auv.api");

/// Tonic requires a numeric message ceiling even when the application does not
/// impose one. Use the platform's representable maximum instead of an
/// AUV-specific capture or protobuf admission policy.
pub const GRPC_MESSAGE_SIZE_UNLIMITED: usize = usize::MAX;

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

    pub mod inference {
      pub mod v1 {
        tonic::include_proto!("auv.api.inference.v1");
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
  }
}

/// Short compatibility imports used by the existing Rust frontends.
pub mod v1 {
  pub use crate::auv::api::driver::v1 as driver;
  pub use crate::auv::api::image::v1 as image;
  pub use crate::auv::api::inference::v1 as inference;
}

#[cfg(test)]
#[path = "lib_test.rs"]
mod tests;
