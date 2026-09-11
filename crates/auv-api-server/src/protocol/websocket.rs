//! WebSocket application messages for one browser-driven routed Runner operation.

use std::convert::Infallible;
use std::sync::Arc;

use auv_api_proto::auv::api::transport::websocket::v1 as proto;
use axum::body::Body;
use axum::extract::ws::{Message, WebSocket};
use axum::http::{HeaderMap, HeaderValue, Request, header};
use bytes::{Buf, Bytes, BytesMut};
use futures_util::{SinkExt as _, StreamExt as _};
use http_body_util::BodyExt as _;
use prost::Message as _;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tonic::{Code, Status};
use tower::ServiceExt as _;

use crate::authentication::Authenticator;
use crate::control::{Control, RunnerRoute};
use crate::protocol::grpc::status::map_control_error;

/// Serves exactly one operation on an upgraded browser connection.
pub(crate) async fn serve(socket: WebSocket, daemon: Arc<dyn Control>, authenticator: Authenticator) {
  let (mut outgoing, mut incoming) = socket.split();

  let result = async {
    let open = receive_open(&mut incoming).await?;
    // NOTICE(websocket-authentication): The browser credential is part of the
    // Open message, so HTTP request middleware cannot authenticate this session.
    let caller = authenticator.authenticate_websocket(&open.credential)?;
    let operation = daemon
      .admit_routed_channel(
        &caller,
        RunnerRoute {
          device_id: open.device_id.clone(),
          run_id: open.run_id.clone(),
          runner_class: open.runner_class.clone(),
        },
        &open.service,
        &open.method,
      )
      .await
      .map_err(map_control_error)?;

    send_message(&mut outgoing, proto::server_message::Message::Ready(proto::Ready {})).await?;

    let (input, body) = grpc_input_body();
    let mut input_task = tokio::spawn(forward_input(incoming, input));
    let request = grpc_request(&open.service, &open.method, body)?;
    let response = tokio::select! {
      response = operation.channel.oneshot(request) => {
        response.map_err(|error| Status::unavailable(format!("Runner transport failed: {error}")))?
      }

      input = &mut input_task => return input_status(input),
    };

    let output = forward_output(&mut outgoing, response);
    tokio::pin!(output);

    let result = tokio::select! {
      result = &mut output => result,
      input = &mut input_task => input_status(input),
    };

    input_task.abort();
    result
  }
  .await;

  let status = match result {
    Ok(()) => Status::new(Code::Ok, ""),
    Err(status) => status,
  };

  let _ = send_message(
    &mut outgoing,
    proto::server_message::Message::End(proto::End {
      grpc_status: status.code() as i32,
      message: status.message().to_string(),
    }),
  )
  .await;
  let _ = outgoing.close().await;
}

async fn forward_output(
  outgoing: &mut futures_util::stream::SplitSink<WebSocket, Message>,
  response: axum::http::Response<tonic::body::Body>,
) -> Result<(), Status> {
  let (parts, mut body) = response.into_parts();
  let mut trailers = None;
  let mut buffered = BytesMut::new();

  while let Some(frame) = body.frame().await {
    let frame = frame?;

    if let Some(data) = frame.data_ref() {
      buffered.extend_from_slice(data);
      while let Some(output) = decode_grpc_frame(&mut buffered)? {
        send_message(outgoing, proto::server_message::Message::Output(proto::Output { payload: output })).await?;
      }
    }

    if let Some(value) = frame.trailers_ref() {
      trailers = Some(value.clone());
    }
  }
  if !buffered.is_empty() {
    return Err(Status::internal("Runner returned an incomplete gRPC frame"));
  }

  grpc_status(&parts.headers, trailers.as_ref())
}

fn input_status(result: Result<Result<(), Status>, tokio::task::JoinError>) -> Result<(), Status> {
  result.map_err(|error| Status::internal(format!("WebSocket input task failed: {error}")))?
}

async fn receive_open(incoming: &mut futures_util::stream::SplitStream<WebSocket>) -> Result<proto::Open, Status> {
  let message = incoming.next().await.ok_or_else(|| Status::cancelled("WebSocket closed before Open"))?;
  let Message::Binary(bytes) = message.map_err(websocket_status)? else {
    return Err(Status::invalid_argument("first WebSocket message must be a binary ClientMessage"));
  };

  let message = proto::ClientMessage::decode(bytes).map_err(|error| Status::invalid_argument(error.to_string()))?;
  match message.message {
    Some(proto::client_message::Message::Open(open)) => Ok(open),
    _ => Err(Status::invalid_argument("first ClientMessage must contain Open")),
  }
}

fn grpc_input_body() -> (mpsc::Sender<Result<Bytes, Infallible>>, Body) {
  let (sender, receiver) = mpsc::channel(8);
  (sender, Body::from_stream(ReceiverStream::new(receiver)))
}

async fn forward_input(
  mut incoming: futures_util::stream::SplitStream<WebSocket>,
  sender: mpsc::Sender<Result<Bytes, Infallible>>,
) -> Result<(), Status> {
  let mut sender = Some(sender);
  while let Some(message) = incoming.next().await {
    let bytes = match message.map_err(websocket_status)? {
      Message::Binary(bytes) => bytes,
      Message::Ping(_) | Message::Pong(_) => continue,
      Message::Close(_) => return Err(Status::cancelled("WebSocket closed before the operation ended")),
      Message::Text(_) => return Err(Status::invalid_argument("WebSocket operation messages must be binary ClientMessage values")),
    };

    let message = proto::ClientMessage::decode(bytes).map_err(|error| Status::invalid_argument(error.to_string()))?;
    match message.message {
      Some(proto::client_message::Message::Input(input)) => sender
        .as_ref()
        .ok_or_else(|| Status::failed_precondition("input received after HalfClose"))?
        .send(Ok(encode_grpc_frame(input.payload)?))
        .await
        .map_err(|_| Status::cancelled("Runner stopped receiving input"))?,
      Some(proto::client_message::Message::HalfClose(_)) => {
        if sender.take().is_none() {
          return Err(Status::failed_precondition("HalfClose may appear only once"));
        }
      }
      Some(proto::client_message::Message::Cancel(cancel)) => return Err(Status::cancelled(cancel.reason)),
      Some(proto::client_message::Message::Open(_)) => return Err(Status::invalid_argument("Open may appear only once")),
      None => return Err(Status::invalid_argument("ClientMessage has no message")),
    }
  }
  Err(Status::cancelled("WebSocket closed before the operation ended"))
}

fn grpc_request(service: &str, method: &str, body: Body) -> Result<Request<tonic::body::Body>, Status> {
  let mut request = Request::new(tonic::body::Body::new(body));

  *request.method_mut() = axum::http::Method::POST;
  *request.uri_mut() =
    format!("/{service}/{method}").parse().map_err(|error| Status::invalid_argument(format!("invalid gRPC path: {error}")))?;
  request.headers_mut().insert(header::CONTENT_TYPE, HeaderValue::from_static("application/grpc"));
  request.headers_mut().insert(header::TE, HeaderValue::from_static("trailers"));

  Ok(request)
}

fn encode_grpc_frame(payload: Vec<u8>) -> Result<Bytes, Status> {
  let length = u32::try_from(payload.len()).map_err(|_| Status::out_of_range("Protobuf input exceeds the gRPC frame length"))?;
  let mut frame = Vec::with_capacity(5 + payload.len());

  frame.push(0);
  frame.extend_from_slice(&length.to_be_bytes());
  frame.extend_from_slice(&payload);

  Ok(frame.into())
}

fn decode_grpc_frame(buffer: &mut BytesMut) -> Result<Option<Vec<u8>>, Status> {
  if buffer.len() < 5 {
    return Ok(None);
  }
  if buffer[0] != 0 {
    return Err(Status::unimplemented("compressed Runner responses are not supported"));
  }
  let length = u32::from_be_bytes(buffer[1..5].try_into().expect("five-byte prefix was checked")) as usize;
  if buffer.len() < 5 + length {
    return Ok(None);
  }
  buffer.advance(5);

  Ok(Some(buffer.split_to(length).to_vec()))
}

fn grpc_status(headers: &HeaderMap, trailers: Option<&HeaderMap>) -> Result<(), Status> {
  let status = headers
    .get("grpc-status")
    .or_else(|| trailers.and_then(|trailers| trailers.get("grpc-status")))
    .ok_or_else(|| Status::internal("Runner response omitted grpc-status"))?
    .to_str()
    .map_err(|error| Status::internal(error.to_string()))?
    .parse::<i32>()
    .map_err(|error| Status::internal(error.to_string()))?;
  if status == 0 {
    return Ok(());
  }

  let message = match headers.get("grpc-message").or_else(|| trailers.and_then(|trailers| trailers.get("grpc-message"))) {
    Some(value) => value.to_str().map_err(|error| Status::internal(error.to_string()))?,
    None => "Runner operation failed",
  };

  Err(Status::new(Code::from_i32(status), message))
}

async fn send_message(
  outgoing: &mut futures_util::stream::SplitSink<WebSocket, Message>,
  message: proto::server_message::Message,
) -> Result<(), Status> {
  outgoing
    .send(Message::Binary(
      proto::ServerMessage {
        message: Some(message),
      }
      .encode_to_vec()
      .into(),
    ))
    .await
    .map_err(websocket_status)
}

fn websocket_status(error: axum::Error) -> Status {
  Status::unavailable(error.to_string())
}
