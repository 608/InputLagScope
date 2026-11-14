//! CH32H417 gamepad latency tester support.
//!
//! This path is separate from the Windows-HID + RP2040 trigger path: the
//! tester is the USB host of the controller and timestamps everything
//! itself; the PC builds the watch spec, starts the runs and computes the
//! statistics.

pub mod app;
pub mod controller;
pub mod detect;
pub mod measure;
pub mod model;
pub mod objects;
pub mod poll;
pub mod protocol;
pub mod runctl;
pub mod settings;
pub mod stats;
pub mod transport;
