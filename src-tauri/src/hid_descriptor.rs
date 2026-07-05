use std::collections::BTreeSet;

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};

use crate::models::InputState;

const ITEM_TYPE_MAIN: u8 = 0;
const ITEM_TYPE_GLOBAL: u8 = 1;
const ITEM_TYPE_LOCAL: u8 = 2;
const MAIN_INPUT: u8 = 8;
const MAIN_COLLECTION: u8 = 10;
const MAIN_END_COLLECTION: u8 = 12;
const GLOBAL_USAGE_PAGE: u8 = 0;
const GLOBAL_LOGICAL_MINIMUM: u8 = 1;
const GLOBAL_LOGICAL_MAXIMUM: u8 = 2;
const GLOBAL_REPORT_SIZE: u8 = 7;
const GLOBAL_REPORT_ID: u8 = 8;
const GLOBAL_REPORT_COUNT: u8 = 9;
const GLOBAL_PUSH: u8 = 10;
const GLOBAL_POP: u8 = 11;
const LOCAL_USAGE: u8 = 0;
const LOCAL_USAGE_MINIMUM: u8 = 1;
const LOCAL_USAGE_MAXIMUM: u8 = 2;
const INPUT_CONSTANT: u32 = 0x01;
const INPUT_VARIABLE: u32 = 0x02;
const USAGE_PAGE_BUTTON: u16 = 0x09;
const USAGE_PAGE_GENERIC_DESKTOP: u16 = 0x01;
const USAGE_PAGE_SIMULATION: u16 = 0x02;

#[cfg_attr(not(test), allow(dead_code))]
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct HidReportShape {
    pub input_report_bytes: usize,
    pub report_ids: Vec<u8>,
    pub button_count: usize,
    pub axis_count: usize,
}

#[derive(Clone, Debug)]
pub struct HidReportParser {
    has_report_ids: bool,
    #[cfg_attr(not(test), allow(dead_code))]
    input_report_bytes: usize,
    #[cfg_attr(not(test), allow(dead_code))]
    report_ids: BTreeSet<u8>,
    buttons: Vec<ButtonField>,
    axes: Vec<ValueField>,
}

#[derive(Clone, Debug)]
struct ButtonField {
    report_id: u8,
    bit_offset: u32,
    bit_size: u32,
}

#[derive(Clone, Debug)]
struct ValueField {
    report_id: u8,
    bit_offset: u32,
    bit_size: u32,
    logical_min: i32,
    logical_max: i32,
}

#[derive(Clone, Debug)]
struct GlobalState {
    usage_page: u16,
    logical_min: i32,
    logical_max: i32,
    report_size: u32,
    report_count: u32,
    report_id: u8,
}

impl Default for GlobalState {
    fn default() -> Self {
        Self {
            usage_page: 0,
            logical_min: 0,
            logical_max: 1,
            report_size: 0,
            report_count: 0,
            report_id: 0,
        }
    }
}

#[derive(Clone, Debug, Default)]
struct LocalState {
    usages: Vec<Usage>,
    usage_min: Option<Usage>,
    usage_max: Option<Usage>,
}

#[derive(Clone, Copy, Debug)]
struct Usage {
    page: Option<u16>,
    id: u16,
}

impl HidReportParser {
    pub fn from_descriptor(descriptor: &[u8]) -> Result<Self> {
        let mut parser = DescriptorBuilder::default();
        parser.parse(descriptor)?;
        Ok(parser.finish())
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn shape(&self) -> HidReportShape {
        HidReportShape {
            input_report_bytes: self.input_report_bytes,
            report_ids: self.report_ids.iter().copied().collect(),
            button_count: self.buttons.len(),
            axis_count: self.axes.len(),
        }
    }

    pub fn parse_input_report(&self, raw: &[u8]) -> Option<InputState> {
        if raw.is_empty() {
            return None;
        }

        let report_id = if self.has_report_ids { raw[0] } else { 0 };
        let report_prefix_bits = if self.has_report_ids { 8 } else { 0 };
        let mut buttons = Vec::with_capacity(self.buttons.len());
        for field in self
            .buttons
            .iter()
            .filter(|field| field.report_id == report_id)
        {
            let value = read_bits(raw, report_prefix_bits + field.bit_offset, field.bit_size)?;
            buttons.push(value != 0);
        }

        let mut axes = Vec::with_capacity(self.axes.len());
        for field in self
            .axes
            .iter()
            .filter(|field| field.report_id == report_id)
        {
            let raw_value = read_bits(raw, report_prefix_bits + field.bit_offset, field.bit_size)?;
            let value = if field.logical_min < 0 {
                sign_extend(raw_value, field.bit_size)
            } else {
                raw_value as i32
            };
            axes.push(normalize_logical(
                value,
                field.logical_min,
                field.logical_max,
            ));
        }

        if buttons.is_empty() && axes.is_empty() {
            None
        } else {
            Some(InputState { buttons, axes })
        }
    }
}

#[derive(Default)]
struct DescriptorBuilder {
    global: GlobalState,
    global_stack: Vec<GlobalState>,
    local: LocalState,
    report_bit_offsets: Vec<(u8, u32)>,
    report_ids: BTreeSet<u8>,
    has_report_ids: bool,
    buttons: Vec<ButtonField>,
    axes: Vec<ValueField>,
}

impl DescriptorBuilder {
    fn parse(&mut self, descriptor: &[u8]) -> Result<()> {
        let mut index = 0;
        while index < descriptor.len() {
            let prefix = descriptor[index];
            index += 1;

            if prefix == 0xFE {
                if index + 2 > descriptor.len() {
                    return Err(anyhow!("truncated HID long item"));
                }
                let data_size = descriptor[index] as usize;
                index += 2;
                index = index.saturating_add(data_size).min(descriptor.len());
                continue;
            }

            let data_size = match prefix & 0x03 {
                0 => 0,
                1 => 1,
                2 => 2,
                _ => 4,
            };
            if index + data_size > descriptor.len() {
                return Err(anyhow!("truncated HID item"));
            }
            let data = &descriptor[index..index + data_size];
            index += data_size;

            let item_type = (prefix >> 2) & 0x03;
            let tag = (prefix >> 4) & 0x0F;
            match item_type {
                ITEM_TYPE_MAIN => self.handle_main(tag, data),
                ITEM_TYPE_GLOBAL => self.handle_global(tag, data)?,
                ITEM_TYPE_LOCAL => self.handle_local(tag, data),
                _ => {}
            }
        }
        Ok(())
    }

    fn finish(self) -> HidReportParser {
        let max_payload_bits = self
            .report_bit_offsets
            .iter()
            .map(|(_, bits)| *bits)
            .max()
            .unwrap_or(0);
        let payload_bytes = max_payload_bits.div_ceil(8) as usize;

        HidReportParser {
            has_report_ids: self.has_report_ids,
            input_report_bytes: payload_bytes + usize::from(self.has_report_ids),
            report_ids: self.report_ids,
            buttons: self.buttons,
            axes: self.axes,
        }
    }

    fn handle_main(&mut self, tag: u8, data: &[u8]) {
        match tag {
            MAIN_INPUT => self.handle_input(unsigned_value(data)),
            MAIN_COLLECTION | MAIN_END_COLLECTION => self.local = LocalState::default(),
            _ => self.local = LocalState::default(),
        }
    }

    fn handle_input(&mut self, flags: u32) {
        let total_bits = self
            .global
            .report_size
            .saturating_mul(self.global.report_count);
        let start_offset = self.current_offset();
        self.set_current_offset(start_offset.saturating_add(total_bits));

        if flags & INPUT_CONSTANT != 0 {
            self.local = LocalState::default();
            return;
        }

        let usages = self.expand_usages(self.global.report_count as usize);
        let variable = flags & INPUT_VARIABLE != 0;

        if variable {
            for index in 0..self.global.report_count {
                let usage = usages
                    .get(index as usize)
                    .copied()
                    .unwrap_or(Usage { page: None, id: 0 });
                let usage_page = usage.page.unwrap_or(self.global.usage_page);
                let bit_offset = start_offset + index.saturating_mul(self.global.report_size);

                if usage_page == USAGE_PAGE_BUTTON {
                    self.buttons.push(ButtonField {
                        report_id: self.global.report_id,
                        bit_offset,
                        bit_size: self.global.report_size.max(1),
                    });
                } else if is_axis_usage(usage_page, usage.id) {
                    self.axes.push(ValueField {
                        report_id: self.global.report_id,
                        bit_offset,
                        bit_size: self.global.report_size.max(1),
                        logical_min: self.global.logical_min,
                        logical_max: self.global.logical_max,
                    });
                }
            }
        }

        self.local = LocalState::default();
    }

    fn handle_global(&mut self, tag: u8, data: &[u8]) -> Result<()> {
        match tag {
            GLOBAL_USAGE_PAGE => self.global.usage_page = unsigned_value(data) as u16,
            GLOBAL_LOGICAL_MINIMUM => self.global.logical_min = signed_value(data),
            GLOBAL_LOGICAL_MAXIMUM => self.global.logical_max = signed_value(data),
            GLOBAL_REPORT_SIZE => self.global.report_size = unsigned_value(data),
            GLOBAL_REPORT_ID => {
                self.global.report_id = unsigned_value(data) as u8;
                self.has_report_ids = true;
                self.report_ids.insert(self.global.report_id);
            }
            GLOBAL_REPORT_COUNT => self.global.report_count = unsigned_value(data),
            GLOBAL_PUSH => self.global_stack.push(self.global.clone()),
            GLOBAL_POP => {
                self.global = self
                    .global_stack
                    .pop()
                    .ok_or_else(|| anyhow!("HID global pop without push"))?;
            }
            _ => {}
        }
        Ok(())
    }

    fn handle_local(&mut self, tag: u8, data: &[u8]) {
        match tag {
            LOCAL_USAGE => self.local.usages.push(decode_usage(data)),
            LOCAL_USAGE_MINIMUM => self.local.usage_min = Some(decode_usage(data)),
            LOCAL_USAGE_MAXIMUM => self.local.usage_max = Some(decode_usage(data)),
            _ => {}
        }
    }

    fn expand_usages(&self, count: usize) -> Vec<Usage> {
        if !self.local.usages.is_empty() {
            let mut usages = self.local.usages.clone();
            if let Some(last) = usages.last().copied() {
                usages.resize(count, last);
            }
            return usages;
        }

        if let (Some(min), Some(max)) = (self.local.usage_min, self.local.usage_max) {
            let page = min.page.or(max.page);
            let mut usages = (min.id..=max.id)
                .map(|id| Usage { page, id })
                .collect::<Vec<_>>();
            if let Some(last) = usages.last().copied() {
                usages.resize(count, last);
            }
            return usages;
        }

        Vec::new()
    }

    fn current_offset(&self) -> u32 {
        self.report_bit_offsets
            .iter()
            .find(|(report_id, _)| *report_id == self.global.report_id)
            .map(|(_, bits)| *bits)
            .unwrap_or(0)
    }

    fn set_current_offset(&mut self, bit_offset: u32) {
        if let Some((_, bits)) = self
            .report_bit_offsets
            .iter_mut()
            .find(|(report_id, _)| *report_id == self.global.report_id)
        {
            *bits = bit_offset;
        } else {
            self.report_bit_offsets
                .push((self.global.report_id, bit_offset));
        }
    }
}

fn is_axis_usage(usage_page: u16, usage: u16) -> bool {
    matches!(
        (usage_page, usage),
        (USAGE_PAGE_GENERIC_DESKTOP, 0x30..=0x39)
            | (USAGE_PAGE_GENERIC_DESKTOP, 0x40..=0x48)
            | (USAGE_PAGE_SIMULATION, 0xBB..=0xC5)
    )
}

fn read_bits(data: &[u8], bit_offset: u32, bit_size: u32) -> Option<u32> {
    if bit_size == 0 || bit_size > 32 {
        return None;
    }

    let end_bit = bit_offset.checked_add(bit_size)?;
    if end_bit > (data.len() as u32).saturating_mul(8) {
        return None;
    }

    let mut value = 0_u32;
    for bit_index in 0..bit_size {
        let absolute_bit = bit_offset + bit_index;
        let byte = data[(absolute_bit / 8) as usize];
        let bit = (byte >> (absolute_bit % 8)) & 1;
        value |= u32::from(bit) << bit_index;
    }
    Some(value)
}

fn normalize_logical(value: i32, logical_min: i32, logical_max: i32) -> f32 {
    if logical_max <= logical_min {
        return 0.0;
    }

    if logical_min < 0 && logical_max > 0 {
        if value >= 0 {
            (value as f32 / logical_max as f32).clamp(-1.0, 1.0)
        } else {
            (value as f32 / logical_min.unsigned_abs() as f32).clamp(-1.0, 1.0)
        }
    } else {
        (((value - logical_min) as f32 / (logical_max - logical_min) as f32) * 2.0 - 1.0)
            .clamp(-1.0, 1.0)
    }
}

fn sign_extend(value: u32, bit_size: u32) -> i32 {
    if bit_size == 0 || bit_size >= 32 {
        value as i32
    } else {
        let shift = 32 - bit_size;
        ((value << shift) as i32) >> shift
    }
}

fn decode_usage(data: &[u8]) -> Usage {
    let raw = unsigned_value(data);
    if raw > 0xFFFF {
        Usage {
            page: Some((raw >> 16) as u16),
            id: raw as u16,
        }
    } else {
        Usage {
            page: None,
            id: raw as u16,
        }
    }
}

fn unsigned_value(data: &[u8]) -> u32 {
    data.iter().enumerate().fold(0_u32, |acc, (index, byte)| {
        acc | (u32::from(*byte) << (index * 8))
    })
}

fn signed_value(data: &[u8]) -> i32 {
    match data.len() {
        0 => 0,
        1 => i8::from_le_bytes([data[0]]) as i32,
        2 => i16::from_le_bytes([data[0], data[1]]) as i32,
        _ => i32::from_le_bytes([
            data.first().copied().unwrap_or(0),
            data.get(1).copied().unwrap_or(0),
            data.get(2).copied().unwrap_or(0),
            data.get(3).copied().unwrap_or(0),
        ]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_report_id_buttons_and_axes() {
        let descriptor = [
            0x05, 0x01, 0x09, 0x05, 0xA1, 0x01, 0x85, 0x01, 0x05, 0x09, 0x19, 0x01, 0x29, 0x10,
            0x15, 0x00, 0x25, 0x01, 0x75, 0x01, 0x95, 0x10, 0x81, 0x02, 0x05, 0x01, 0x09, 0x30,
            0x09, 0x31, 0x09, 0x32, 0x09, 0x35, 0x15, 0x81, 0x25, 0x7F, 0x75, 0x08, 0x95, 0x04,
            0x81, 0x02, 0xC0,
        ];
        let parser = HidReportParser::from_descriptor(&descriptor).unwrap();
        let shape = parser.shape();
        assert_eq!(shape.input_report_bytes, 7);
        assert_eq!(shape.report_ids, vec![1]);
        assert_eq!(shape.button_count, 16);
        assert_eq!(shape.axis_count, 4);

        let state = parser
            .parse_input_report(&[0x01, 0x01, 0x80, 0x7F, 0x81, 0x00, 0x40])
            .unwrap();
        assert!(state.buttons[0]);
        assert!(state.buttons[15]);
        assert!((state.axes[0] - 1.0).abs() < 0.001);
        assert!((state.axes[1] + 1.0).abs() < 0.001);
        assert!(state.axes[2].abs() < 0.001);
        assert!((state.axes[3] - 0.503).abs() < 0.01);
    }

    #[test]
    fn parses_without_report_id() {
        let descriptor = [
            0x05, 0x01, 0x09, 0x04, 0xA1, 0x01, 0x05, 0x09, 0x19, 0x01, 0x29, 0x08, 0x15, 0x00,
            0x25, 0x01, 0x75, 0x01, 0x95, 0x08, 0x81, 0x02, 0x05, 0x01, 0x09, 0x30, 0x09, 0x31,
            0x15, 0x00, 0x26, 0xFF, 0x00, 0x75, 0x08, 0x95, 0x02, 0x81, 0x02, 0xC0,
        ];
        let parser = HidReportParser::from_descriptor(&descriptor).unwrap();
        let shape = parser.shape();
        assert_eq!(shape.input_report_bytes, 3);
        assert!(shape.report_ids.is_empty());
        assert_eq!(shape.button_count, 8);
        assert_eq!(shape.axis_count, 2);

        let state = parser
            .parse_input_report(&[0b0000_0011, 0x00, 0xFF])
            .unwrap();
        assert!(state.buttons[0]);
        assert!(state.buttons[1]);
        assert!((state.axes[0] + 1.0).abs() < 0.001);
        assert!((state.axes[1] - 1.0).abs() < 0.001);
    }
}
