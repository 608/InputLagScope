//! Report-aware target detection and the trigger guard of the tester path.
//!
//! The steps follow `measurement.rs` (release, settle, baseline, assert,
//! first hit after t0, release) and reuse its hit functions, with two
//! differences required by the tester:
//!
//! * Samples carry the report group they belong to (`TesterSample`). For
//!   generic HID with report IDs every report numbers its buttons and axes
//!   from 0, so baselines, neutral averages and hits are only compared
//!   within one report ID, and the target is `(report_id, index)`.
//! * The trigger is driven with TRIGGER_MANUAL through `TriggerPort`, which
//!   is armed before the assert is sent and releases on every path. A
//!   release whose outcome is unknown closes the connection, so that the
//!   next use starts a new session (HELLO releases every trigger).

use std::cell::Cell;
use std::collections::{BTreeMap, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, Result};
use crossbeam_channel::{Receiver, RecvTimeoutError};

use crate::measurement::{
    detect_axis_hit, detect_button_hit, peak_axis_delta, DetectionHit, RING_CAPACITY,
};
use crate::models::{MeasurementInputType, RawSample, ResolvedTarget};
use crate::report_layout::ReportDecoder;

use super::stats::TICK_NS;
use super::transport::TesterClient;

/// A decoded monitor report and the report group (report ID for generic
/// HID with report IDs, otherwise `None`) its logical indices belong to.
#[derive(Clone, Debug)]
pub struct TesterSample {
    pub report_id: Option<u8>,
    pub sample: RawSample,
}

/// Neutral axis values per report group.
pub type NeutralByReport = BTreeMap<Option<u8>, Vec<f32>>;

/// `restrict` = the report ID selected by the user (`None` = any).
pub fn report_matches(restrict: Option<u8>, report_id: Option<u8>) -> bool {
    restrict.is_none_or(|wanted| report_id == Some(wanted))
}

fn describe_report(report_id: Option<u8>) -> String {
    report_id.map_or_else(String::new, |id| format!(" in report ID {id}"))
}

// ---------------------------------------------------------------------------
// Sample ring

pub struct SampleRing {
    ring: VecDeque<TesterSample>,
}

impl Default for SampleRing {
    fn default() -> Self {
        Self::new()
    }
}

impl SampleRing {
    pub fn new() -> Self {
        Self {
            ring: VecDeque::with_capacity(RING_CAPACITY),
        }
    }

    pub fn push(&mut self, sample: TesterSample) {
        if self.ring.len() >= RING_CAPACITY {
            self.ring.pop_front();
        }
        self.ring.push_back(sample);
    }

    pub fn drain(&mut self, rx: &Receiver<TesterSample>) {
        while let Ok(sample) = rx.try_recv() {
            self.push(sample);
        }
    }

    /// Latest sample of the report group `report_id`.
    pub fn latest_of(&self, report_id: Option<u8>) -> Option<&TesterSample> {
        self.ring
            .iter()
            .rev()
            .find(|sample| sample.report_id == report_id)
    }

    /// Latest sample of every report group allowed by `restrict`.
    pub fn latest_by_report(&self, restrict: Option<u8>) -> BTreeMap<Option<u8>, RawSample> {
        let mut latest = BTreeMap::new();
        for sample in self.ring.iter().rev() {
            if report_matches(restrict, sample.report_id) {
                latest
                    .entry(sample.report_id)
                    .or_insert_with(|| sample.sample.clone());
            }
        }
        latest
    }
}

/// Waits for the first sample of the selected report (as
/// `measurement.rs::wait_for_first_sample`).
pub fn wait_for_first_sample(
    rx: &Receiver<TesterSample>,
    ring: &mut SampleRing,
    timeout: Duration,
    restrict: Option<u8>,
) -> Result<()> {
    let deadline = Instant::now() + timeout;
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            bail!("input sample did not arrive{}", describe_report(restrict));
        }
        match rx.recv_timeout(remaining) {
            Ok(sample) => {
                let wanted = report_matches(restrict, sample.report_id);
                ring.push(sample);
                if wanted {
                    break;
                }
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => bail!("the report monitor stopped"),
        }
    }
    ring.drain(rx);
    Ok(())
}

/// Mean of every axis over `duration`, per report group (as
/// `measurement.rs::collect_axis_means`, which averages all samples
/// together).
pub fn collect_axis_means(
    rx: &Receiver<TesterSample>,
    ring: &mut SampleRing,
    duration: Duration,
    restrict: Option<u8>,
) -> NeutralByReport {
    let deadline = Instant::now() + duration;
    let mut sums = BTreeMap::<Option<u8>, (Vec<f64>, Vec<usize>)>::new();

    while Instant::now() < deadline {
        if let Ok(sample) = rx.recv_timeout(Duration::from_millis(10)) {
            if report_matches(restrict, sample.report_id) {
                let (sum, count) = sums.entry(sample.report_id).or_default();
                for (index, value) in sample.sample.state.axes.iter().enumerate() {
                    if sum.len() <= index {
                        sum.resize(index + 1, 0.0);
                        count.resize(index + 1, 0);
                    }
                    sum[index] += f64::from(*value);
                    count[index] += 1;
                }
            }
            ring.push(sample);
        }
    }

    sums.into_iter()
        .map(|(report_id, (sum, count))| {
            let means = sum
                .iter()
                .zip(count.iter())
                .map(|(sum, count)| {
                    if *count == 0 {
                        0.0
                    } else {
                        (sum / *count as f64) as f32
                    }
                })
                .collect();
            (report_id, means)
        })
        .collect()
}

/// Largest axis deviation from the neutral of the sample's own report
/// group (as `measurement.rs::collect_peak_axis_delta`). Samples of a group
/// without a neutral are ignored.
pub fn collect_peak_axis_delta(
    rx: &Receiver<TesterSample>,
    ring: &mut SampleRing,
    neutral: &NeutralByReport,
    axis_index: i32,
    restrict: Option<u8>,
    min_timestamp_ns: Option<u64>,
    duration: Duration,
) -> Option<(Option<u8>, DetectionHit)> {
    let deadline = Instant::now() + duration;
    let mut best = None::<(Option<u8>, DetectionHit)>;

    while Instant::now() < deadline {
        let Ok(sample) = rx.recv_timeout(Duration::from_millis(1)) else {
            continue;
        };
        let hit = (report_matches(restrict, sample.report_id)
            && min_timestamp_ns.is_none_or(|timestamp| sample.sample.timestamp_ns >= timestamp))
        .then(|| {
            neutral
                .get(&sample.report_id)
                .and_then(|axes| peak_axis_delta(&sample.sample, axes, axis_index))
        })
        .flatten();
        let report_id = sample.report_id;
        ring.push(sample);
        if let Some(hit) = hit {
            if best
                .as_ref()
                .is_none_or(|(_, current)| hit.score > current.score)
            {
                best = Some((report_id, hit));
            }
        }
    }

    best
}

// ---------------------------------------------------------------------------
// Trigger

/// TRIGGER_MANUAL with a release guarantee. `engaged` is set before the
/// assert is sent (a lost response may still have asserted the pins) and
/// cleared only by a confirmed release or by closing the connection.
pub struct TriggerPort<'a> {
    client: &'a TesterClient,
    mask: u8,
    engaged: Cell<bool>,
}

impl<'a> TriggerPort<'a> {
    pub fn new(client: &'a TesterClient, mask: u8) -> Self {
        Self {
            client,
            mask,
            engaged: Cell::new(false),
        }
    }

    /// Asserts the trigger pins and returns a lower bound of the assertion
    /// time in ns of the tester clock: GET_STATUS `now` read right before
    /// TRIGGER_MANUAL (the published time lags by up to 1 ms, so it is
    /// never later than the assertion).
    pub fn assert(&self) -> Result<u64> {
        let now = self.client.get_status()?.now;
        self.engaged.set(true);
        self.client
            .trigger_manual(self.mask, 0)
            .map_err(|error| anyhow!(error).context("asserting the trigger failed"))?;
        Ok(now.saturating_mul(TICK_NS))
    }

    /// Releases the trigger pins. A refused release is an error; a release
    /// whose outcome is unknown closes the connection (the next session
    /// releases every trigger) and is an error as well.
    pub fn release(&self) -> Result<()> {
        match self.client.trigger_manual(0, self.mask) {
            Ok(()) => {
                self.engaged.set(false);
                Ok(())
            }
            Err(error) if error.outcome_unknown() => {
                self.client.close();
                self.engaged.set(false);
                Err(anyhow!(error).context(
                    "releasing the trigger could not be confirmed; the tester connection was \
                     closed so that the next session (HELLO) releases every trigger",
                ))
            }
            Err(error) => Err(anyhow!(error).context("the tester did not release the trigger")),
        }
    }

    /// Releases after `result`; a failed release takes precedence and
    /// keeps the original error as context.
    pub fn release_after<T>(&self, result: Result<T>) -> Result<T> {
        let released = self.release();
        match (result, released) {
            (Ok(value), Ok(())) => Ok(value),
            (Ok(_), Err(release)) => Err(release),
            (Err(error), Ok(())) => Err(error),
            (Err(error), Err(release)) => Err(release.context(format!("{error:#}"))),
        }
    }

    pub fn is_engaged(&self) -> bool {
        self.engaged.get()
    }
}

impl Drop for TriggerPort<'_> {
    fn drop(&mut self) {
        // Early return, `?` or panic while the pins may be asserted.
        if self.engaged.get() && self.release().is_err() && self.client.is_alive() {
            // The tester refused: a new session releases every trigger.
            self.client.close();
        }
    }
}

// ---------------------------------------------------------------------------
// Target detection

/// Button detection (`measurement.rs::detect_button_target`) with one
/// baseline per report group.
#[allow(clippy::too_many_arguments)]
pub fn detect_button_target(
    trigger: &TriggerPort<'_>,
    rx: &Receiver<TesterSample>,
    ring: &mut SampleRing,
    timeout_ms: u64,
    retry_delay_ms: u64,
    restrict: Option<u8>,
    log: &dyn Fn(&str),
    stop: &AtomicBool,
) -> Result<(ResolvedTarget, TesterSample)> {
    log("detecting target button");

    trigger.release()?;
    thread::sleep(Duration::from_millis(retry_delay_ms));
    ring.drain(rx);
    // Reports without buttons cannot give a target.
    let baselines: BTreeMap<Option<u8>, RawSample> = ring
        .latest_by_report(restrict)
        .into_iter()
        .filter(|(_, sample)| !sample.state.buttons.is_empty())
        .collect();
    if baselines.is_empty() {
        bail!(
            "no baseline sample for button detection{}",
            describe_report(restrict)
        );
    }

    // A failed assert may still have asserted the pins: release first.
    let t0_ns = match trigger.assert() {
        Ok(t0_ns) => t0_ns,
        Err(error) => return trigger.release_after(Err(error)),
    };
    let deadline = Instant::now() + Duration::from_millis(timeout_ms);
    let mut found = None;
    while Instant::now() < deadline && !stop.load(Ordering::Relaxed) {
        if let Ok(sample) = rx.recv_timeout(Duration::from_millis(1)) {
            let hit = baselines
                .get(&sample.report_id)
                .filter(|_| report_matches(restrict, sample.report_id))
                .filter(|_| !sample.sample.state.buttons.is_empty())
                .and_then(|baseline| detect_button_hit(&sample.sample, baseline, None, None));
            ring.push(sample.clone());
            if sample.sample.timestamp_ns >= t0_ns {
                if let Some(hit) = hit {
                    found = Some((hit, sample));
                    break;
                }
            }
        }
    }

    match found {
        Some((hit, sample)) => {
            trigger.release()?;
            thread::sleep(Duration::from_millis(retry_delay_ms));
            Ok((
                ResolvedTarget {
                    input_type: Some(MeasurementInputType::Button),
                    button_index: Some(hit.index),
                    button_assert_state: Some(hit.button_state.unwrap_or(true)),
                    report_id: sample.report_id,
                    ..ResolvedTarget::default()
                },
                sample,
            ))
        }
        None => {
            trigger.release()?;
            if stop.load(Ordering::Relaxed) {
                bail!("target button detection was stopped");
            }
            bail!("target button detection timed out");
        }
    }
}

/// Axis detection (`measurement.rs::detect_axis_target`) against the
/// neutral of the sample's own report group.
#[allow(clippy::too_many_arguments)]
pub fn detect_axis_target(
    trigger: &TriggerPort<'_>,
    rx: &Receiver<TesterSample>,
    ring: &mut SampleRing,
    neutral: &NeutralByReport,
    axis_index: i32,
    threshold: f32,
    timeout_ms: u64,
    retry_delay_ms: u64,
    restrict: Option<u8>,
    log: &dyn Fn(&str),
    stop: &AtomicBool,
) -> Result<(ResolvedTarget, TesterSample)> {
    log("detecting target axis");

    trigger.release()?;
    thread::sleep(Duration::from_millis(retry_delay_ms));
    ring.drain(rx);

    // A failed assert may still have asserted the pins: release first.
    let t0_ns = match trigger.assert() {
        Ok(t0_ns) => t0_ns,
        Err(error) => return trigger.release_after(Err(error)),
    };
    let deadline = Instant::now() + Duration::from_millis(timeout_ms);
    let mut found = None;
    while Instant::now() < deadline && !stop.load(Ordering::Relaxed) {
        if let Ok(sample) = rx.recv_timeout(Duration::from_millis(1)) {
            let hit = neutral
                .get(&sample.report_id)
                .filter(|_| report_matches(restrict, sample.report_id))
                .and_then(|axes| {
                    detect_axis_hit(&sample.sample, axes, axis_index, None, threshold)
                });
            ring.push(sample.clone());
            if sample.sample.timestamp_ns >= t0_ns {
                if let Some(hit) = hit {
                    found = Some((hit, sample));
                    break;
                }
            }
        }
    }

    match found {
        Some((hit, sample)) => {
            trigger.release()?;
            thread::sleep(Duration::from_millis(retry_delay_ms));
            Ok((
                ResolvedTarget {
                    input_type: Some(MeasurementInputType::Axis),
                    axis_index: Some(hit.index),
                    axis_direction: hit.axis_direction,
                    axis_threshold: Some(threshold),
                    report_id: sample.report_id,
                    ..ResolvedTarget::default()
                },
                sample,
            ))
        }
        None => {
            trigger.release()?;
            if stop.load(Ordering::Relaxed) {
                bail!("target axis detection was stopped");
            }
            bail!("target axis detection timed out");
        }
    }
}

/// Report group of a fixed button index. With report IDs the index must
/// name a single report: either `restrict` selects it, or exactly one
/// report has that many buttons.
pub fn resolve_fixed_button_report(
    decoder: &ReportDecoder,
    index: usize,
    restrict: Option<u8>,
) -> Result<Option<u8>> {
    let keys = match restrict {
        Some(report_id) => vec![Some(report_id)],
        None => decoder.input_report_keys(),
    };
    let candidates: Vec<Option<u8>> = keys
        .into_iter()
        .filter(|key| {
            decoder
                .layout_for_key(*key)
                .buttons
                .get(index)
                .is_some_and(Option::is_some)
        })
        .collect();
    match candidates.as_slice() {
        [key] => Ok(*key),
        [] => bail!(
            "button index {index} does not exist{}",
            describe_report(restrict)
        ),
        several => bail!(
            "button index {index} exists in report IDs {}; select one with report_id (\"<report_id>:{index}\")",
            several
                .iter()
                .map(|key| key.map_or_else(|| "-".to_string(), |id| id.to_string()))
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}

/// Validates a user-selected report ID against the decoder.
pub fn validate_report_selection(decoder: &ReportDecoder, restrict: Option<u8>) -> Result<()> {
    let Some(report_id) = restrict else {
        return Ok(());
    };
    if !decoder.uses_report_ids() {
        bail!(
            "report ID {report_id} was selected, but the controller's reports have no report IDs"
        );
    }
    if !decoder.input_report_keys().contains(&Some(report_id)) {
        bail!("report ID {report_id} has no buttons or axes in the HID report descriptor");
    }
    Ok(())
}
