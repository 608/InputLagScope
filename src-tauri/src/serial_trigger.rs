use std::io::{Read, Write};
use std::time::Duration;

use anyhow::Result;
use serialport::{ClearBuffer, SerialPort};

use crate::clock::timestamp_ns;

const FAST_CMD_GPIO_ASSERT: u8 = 0x02;
const FAST_CMD_GPIO_RELEASE: u8 = 0x03;
const FAST_CMD_TIME_SYNC: u8 = 0x10;
const FAST_ACK_MAGIC: u8 = 0xA5;
const CLOCK_SYNC_SAMPLES: usize = 128;
const CLOCK_SYNC_REFRESH_SAMPLES: usize = 1;
const CLOCK_SYNC_KEEP_SAMPLES: usize = 512;

pub struct SerialTrigger {
    port: Box<dyn SerialPort>,
    clock_samples: Vec<ClockSyncSample>,
    clock_scale: f64,
    clock_offset_ns: f64,
}

impl SerialTrigger {
    pub fn open(port_name: &str, baud_rate: u32) -> Result<Self> {
        let port = serialport::new(port_name, baud_rate)
            .timeout(Duration::from_millis(50))
            .open()?;
        let mut trigger = Self {
            port,
            clock_samples: Vec::with_capacity(CLOCK_SYNC_KEEP_SAMPLES),
            clock_scale: 1.0,
            clock_offset_ns: 0.0,
        };
        let _ = trigger.port.clear(ClearBuffer::All);
        trigger.sync_device_clock()?;
        Ok(trigger)
    }

    pub fn assert_gpio2(&mut self) -> Result<TriggerTimestamp> {
        self.send_fast(FAST_CMD_GPIO_ASSERT)
    }

    pub fn release_gpio2(&mut self) -> Result<TriggerTimestamp> {
        self.send_fast(FAST_CMD_GPIO_RELEASE)
    }

    pub fn refresh_device_clock(&mut self) -> Result<()> {
        self.collect_clock_samples(CLOCK_SYNC_REFRESH_SAMPLES)?;
        self.fit_device_clock()
    }

    fn send_fast(&mut self, byte: u8) -> Result<TriggerTimestamp> {
        let _ = self.port.clear(ClearBuffer::Input);
        self.port.write_all(&[byte])?;
        self.port.flush()?;
        let ack = self.read_fast_ack(byte)?;
        Ok(TriggerTimestamp {
            pc_ns: self.device_time_to_pc_ns(ack.timestamp_us),
            device_us: ack.timestamp_us,
        })
    }

    fn sync_device_clock(&mut self) -> Result<()> {
        self.collect_clock_samples(CLOCK_SYNC_SAMPLES)?;
        self.fit_device_clock()
    }

    fn collect_clock_samples(&mut self, count: usize) -> Result<()> {
        for _ in 0..count {
            let _ = self.port.clear(ClearBuffer::Input);
            let pc_start_ns = timestamp_ns();
            self.port.write_all(&[FAST_CMD_TIME_SYNC])?;
            self.port.flush()?;
            let ack = self.read_fast_ack(FAST_CMD_TIME_SYNC)?;
            let pc_end_ns = timestamp_ns();

            if pc_end_ns >= pc_start_ns {
                let rtt_ns = pc_end_ns - pc_start_ns;
                let pc_mid_ns = pc_start_ns + (rtt_ns / 2);
                self.clock_samples.push(ClockSyncSample {
                    rtt_ns,
                    device_ns: ack.timestamp_us.saturating_mul(1_000),
                    pc_mid_ns,
                });
                if self.clock_samples.len() > CLOCK_SYNC_KEEP_SAMPLES {
                    let excess = self.clock_samples.len() - CLOCK_SYNC_KEEP_SAMPLES;
                    self.clock_samples.drain(0..excess);
                }
            }
        }

        Ok(())
    }

    fn fit_device_clock(&mut self) -> Result<()> {
        if self.clock_samples.is_empty() {
            anyhow::bail!("failed to synchronize RP2040 trigger clock");
        }

        let mut by_rtt = self.clock_samples.clone();
        by_rtt.sort_by_key(|sample| sample.rtt_ns);
        let threshold_index = ((by_rtt.len() - 1) * 3) / 4;
        let rtt_threshold = by_rtt[threshold_index].rtt_ns;
        let selected = self
            .clock_samples
            .iter()
            .filter(|sample| sample.rtt_ns <= rtt_threshold)
            .cloned()
            .collect::<Vec<_>>();

        if selected.len() < 2 {
            let sample = by_rtt[0].clone();
            self.clock_scale = 1.0;
            self.clock_offset_ns = sample.pc_mid_ns as f64 - sample.device_ns as f64;
            return Ok(());
        }

        let mean_device = selected
            .iter()
            .map(|sample| sample.device_ns as f64)
            .sum::<f64>()
            / selected.len() as f64;
        let mean_pc = selected
            .iter()
            .map(|sample| sample.pc_mid_ns as f64)
            .sum::<f64>()
            / selected.len() as f64;

        let mut covariance = 0.0;
        let mut variance = 0.0;
        for sample in &selected {
            let device_delta = sample.device_ns as f64 - mean_device;
            let pc_delta = sample.pc_mid_ns as f64 - mean_pc;
            covariance += device_delta * pc_delta;
            variance += device_delta * device_delta;
        }

        if variance <= f64::EPSILON {
            let sample = by_rtt[0].clone();
            self.clock_scale = 1.0;
            self.clock_offset_ns = sample.pc_mid_ns as f64 - sample.device_ns as f64;
            return Ok(());
        }

        self.clock_scale = covariance / variance;
        self.clock_offset_ns = mean_pc - (self.clock_scale * mean_device);
        Ok(())
    }

    fn read_fast_ack(&mut self, expected_kind: u8) -> Result<FastAck> {
        let mut byte = [0_u8; 1];
        loop {
            self.port.read_exact(&mut byte)?;
            if byte[0] == FAST_ACK_MAGIC {
                break;
            }
        }

        let mut rest = [0_u8; 9];
        self.port.read_exact(&mut rest)?;
        let kind = rest[0];
        if kind != expected_kind {
            anyhow::bail!(
                "unexpected trigger ack: expected 0x{expected_kind:02x}, got 0x{kind:02x}"
            );
        }

        let mut timestamp_bytes = [0_u8; 8];
        timestamp_bytes.copy_from_slice(&rest[1..]);
        Ok(FastAck {
            timestamp_us: u64::from_le_bytes(timestamp_bytes),
        })
    }

    pub fn device_time_to_pc_ns(&self, timestamp_us: u64) -> u64 {
        let device_ns = timestamp_us as f64 * 1_000.0;
        let pc_ns = (self.clock_scale * device_ns) + self.clock_offset_ns;
        if !pc_ns.is_finite() {
            return 0;
        }
        pc_ns.round().clamp(0.0, u64::MAX as f64) as u64
    }
}

pub struct TriggerTimestamp {
    pub pc_ns: u64,
    pub device_us: u64,
}

struct FastAck {
    timestamp_us: u64,
}

#[derive(Clone)]
struct ClockSyncSample {
    rtt_ns: u64,
    device_ns: u64,
    pc_mid_ns: u64,
}
