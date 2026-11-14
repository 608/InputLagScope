//! Ending runs (trials and polling tests) whose state is uncertain.
//!
//! From the moment TRIAL_START / POLL_TEST_START is sent the run may be
//! active on the tester, even when the response is lost. The PC then sends
//! the STOP command with the same run_id, keeps waiting for the terminal
//! event (RUN_DONE / POLL_RESULT) that the tester always sends once, and,
//! if that event does not arrive in time, closes the connection: the next
//! session (HELLO) stops every run and releases every trigger.

use std::time::Duration;

use anyhow::anyhow;

use super::protocol::STATUS_BAD_PARAM;
use super::transport::{TesterClient, TesterError};

/// Time to wait for the terminal event after a STOP.
pub const STOP_WAIT: Duration = Duration::from_secs(5);
/// Time to wait when the STOP was answered BAD_PARAM (the run is not
/// running: it already ended, and its terminal event may be in flight, or
/// it never started).
pub const NOT_RUNNING_WAIT: Duration = Duration::from_secs(1);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RunKind {
    Trial,
    Poll,
}

#[derive(Debug)]
pub enum StopOutcome {
    /// The tester accepted the STOP; the terminal event follows.
    Accepted,
    /// BAD_PARAM: the run_id is not the running one.
    NotRunning,
    /// No usable answer (lost response, link error) or another status.
    Unconfirmed(TesterError),
}

impl StopOutcome {
    /// How long to wait for the terminal event after this outcome.
    pub fn wait(&self) -> Duration {
        match self {
            Self::NotRunning => NOT_RUNNING_WAIT,
            _ => STOP_WAIT,
        }
    }
}

pub fn send_stop(client: &TesterClient, kind: RunKind, run_id: u32) -> StopOutcome {
    let result = match kind {
        RunKind::Trial => client.trial_stop(run_id),
        RunKind::Poll => client.poll_test_stop(run_id),
    };
    match result {
        Ok(()) => StopOutcome::Accepted,
        Err(error) if error.is_status(STATUS_BAD_PARAM) => StopOutcome::NotRunning,
        Err(error) => StopOutcome::Unconfirmed(error),
    }
}

/// Called when the terminal event did not arrive in time. The connection
/// is always closed: even when GET_STATUS shows the run as inactive, the
/// tester may have ended it by an event-queue overflow and then refuses new runs until a new session. The next use opens a
/// new session (HELLO), which stops every run, releases the triggers and
/// clears that state. GET_STATUS only words the error. Returns the error to
/// report.
pub fn unconfirmed_end(client: &TesterClient, run_id: u32, what: &str) -> anyhow::Error {
    if !client.is_alive() {
        return anyhow!("{what}; the tester connection is closed");
    }
    let state = match client.get_status() {
        Ok(status) if status.active_run_id != run_id => format!(
            "the tester reports that run {run_id} is not active (it may have been ended by an \
             event-queue overflow)"
        ),
        Ok(_) => format!("run {run_id} is still active on the tester"),
        Err(error) => format!("the run state could not be read ({error})"),
    };
    client.close();
    anyhow!(
        "{what}; {state}; the connection was closed so that the next session (HELLO) stops \
         every run, releases the triggers and accepts new runs"
    )
}
