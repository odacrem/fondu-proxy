//! Waiting on a pending backend request with a deadline.
//!
//! The fastly crate can only wait on a pending request indefinitely, so this calls the
//! `fastly_async_io` hostcalls on the raw handle directly, as fastly's own
//! `experimental::reusable_sessions::RequestPromise::wait_timeout` does.

use std::error::Error;
use std::fmt;
use std::time::Instant;

use fastly::handle::PendingRequestHandle;
use fastly::http::request::SendErrorCause;
use fastly::Response;
use fastly_sys::fastly_async_io;

/// Why [`wait_until`] returned without a response.
#[derive(Debug)]
pub enum WaitError {
    /// The response wasn't ready by the deadline.
    DeadlinePassed,
    /// The request failed.
    Send(SendErrorCause),
}

impl fmt::Display for WaitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            WaitError::DeadlinePassed => f.write_str("not ready within the time budget"),
            WaitError::Send(e) => write!(f, "request failed: {e}"),
        }
    }
}

impl Error for WaitError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            WaitError::DeadlinePassed => None,
            WaitError::Send(e) => Some(e),
        }
    }
}

/// Waits for the response to `pending`, giving up at `deadline` if there is one.
pub fn wait_until(
    pending: PendingRequestHandle,
    deadline: Option<Instant>,
) -> Result<Response, WaitError> {
    if deadline.is_some_and(|deadline| !is_ready_by(&pending, deadline)) {
        return Err(WaitError::DeadlinePassed);
    }
    let (resp_handle, body_handle) = pending.wait().map_err(WaitError::Send)?;
    Ok(Response::from_handles(resp_handle, body_handle))
}

/// Blocks until `pending` is ready or `deadline` passes, and returns whether it is ready.
fn is_ready_by(pending: &PendingRequestHandle, deadline: Instant) -> bool {
    let handle = pending.as_u32();
    let remaining = deadline.saturating_duration_since(Instant::now());
    if remaining.is_zero() {
        // select treats a zero timeout as "wait forever", so only check readiness
        let mut ready = 0;
        // SAFETY: `handle` belongs to `pending`, which outlives this call,
        // and `ready` is a valid pointer for the host to write to.
        let status = unsafe { fastly_async_io::is_ready(handle, &raw mut ready) };
        status.is_ok() && ready != 0
    } else {
        // select takes whole milliseconds; round sub-millisecond waits up
        let timeout_ms = u32::try_from(remaining.as_millis())
            .unwrap_or(u32::MAX)
            .max(1);
        // index of the ready handle; anything but 0 means the timeout expired
        let mut done_index = u32::MAX;
        // SAFETY: the handle list is a single handle belonging to `pending`, which outlives
        // this call, and `done_index` is a valid pointer for the host to write to.
        let status = unsafe {
            fastly_async_io::select(&raw const handle, 1, timeout_ms, &raw mut done_index)
        };
        status.is_ok() && done_index == 0
    }
}
