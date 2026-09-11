//! API discovery service adapter.

use std::sync::Arc;

use auv_api_proto::auv::api::daemon::v1 as proto;
use auv_api_proto::auv::api::daemon::v1::discovery_service_server::DiscoveryService;
use tonic::{Request, Response, Status};

use crate::control::Control;
use crate::protocol::grpc::status::map_control_error;

#[derive(Clone)]
pub(crate) struct DiscoveryServiceGrpc {
  daemon: Arc<dyn Control>,
}

impl DiscoveryServiceGrpc {
  pub(crate) fn new(daemon: Arc<dyn Control>) -> Self {
    Self { daemon }
  }
}

#[tonic::async_trait]
impl DiscoveryService for DiscoveryServiceGrpc {
  async fn list_api_namespaces(
    &self,
    _request: Request<proto::ListApiNamespacesRequest>,
  ) -> Result<Response<proto::ListApiNamespacesResponse>, Status> {
    Ok(Response::new(proto::ListApiNamespacesResponse {
      namespaces: vec![proto::ApiNamespace {
        name: "auv".to_string(),
      }],
    }))
  }

  async fn get_api_namespace(
    &self,
    request: Request<proto::GetApiNamespaceRequest>,
  ) -> Result<Response<proto::GetApiNamespaceResponse>, Status> {
    if request.get_ref().namespace != "auv" {
      return Err(Status::not_found("unknown API namespace"));
    }
    Ok(Response::new(proto::GetApiNamespaceResponse {
      namespace: "auv".to_string(),
      groups: vec![
        proto::ApiGroup {
          name: "daemon".to_string(),
          versions: vec!["v1".to_string()],
        },
        proto::ApiGroup {
          name: "runtime".to_string(),
          versions: vec!["v1".to_string()],
        },
      ],
    }))
  }

  async fn get_api_group_version(
    &self,
    request: Request<proto::GetApiGroupVersionRequest>,
  ) -> Result<Response<proto::GetApiGroupVersionResponse>, Status> {
    let request = request.into_inner();
    if request.namespace != "auv" {
      return Err(Status::not_found("unknown API namespace"));
    }
    let resources = match (request.group.as_str(), request.version.as_str()) {
      ("daemon", "v1") => vec![api_resource(
        "devices",
        "Device",
        &[
          proto::ApiResourceOperation::List,
          proto::ApiResourceOperation::Get,
        ],
      )],
      ("runtime", "v1") => {
        let read = [
          proto::ApiResourceOperation::List,
          proto::ApiResourceOperation::Get,
        ];
        let mut runners = read.to_vec();
        if !self.daemon.list_runner_classes(None).map_err(map_control_error)?.is_empty() {
          runners.extend([
            proto::ApiResourceOperation::Create,
            proto::ApiResourceOperation::Delete,
          ]);
        }
        let mut runs = read.to_vec();
        runs.extend([
          proto::ApiResourceOperation::Create,
          proto::ApiResourceOperation::Delete,
        ]);
        vec![
          api_resource("runners", "Runner", &runners),
          api_resource("runnerclasses", "RunnerClass", &read),
          api_resource("runs", "Run", &runs),
        ]
      }
      _ => return Err(Status::not_found("unknown AUV API group or version")),
    };
    Ok(Response::new(proto::GetApiGroupVersionResponse {
      namespace: request.namespace,
      group: request.group,
      version: request.version,
      resources,
    }))
  }
}

fn api_resource(name: &str, kind: &str, operations: &[proto::ApiResourceOperation]) -> proto::ApiResource {
  proto::ApiResource {
    name: name.to_string(),
    kind: kind.to_string(),
    operations: operations.iter().map(|operation| *operation as i32).collect(),
  }
}
