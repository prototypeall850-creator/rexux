use super::*;
use rexux_protocol::error::RexuxErrorDetails;

pub(super) fn environment_selection_error(err: RexuxErr) -> JSONRPCErrorError {
    match err.details() {
        RexuxErrorDetails::InvalidRequest(message) => invalid_request(message.clone()),
        _ => internal_error(format!("failed to validate environment selections: {err}")),
    }
}
