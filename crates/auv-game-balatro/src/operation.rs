// TODO(balatro-operations-v1): Target resolution, driver delivery, operation
// request/result fields, and semantic verification are deferred until the
// owner-approved operation slice. These opaque placeholders keep Task 2 focused
// on model/output contracts without stabilizing an operation wire shape.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OperationRequest {
  _private: (),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OperationResult {
  _private: (),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerificationMode {
  _private: (),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerificationProfile {
  _private: (),
}
