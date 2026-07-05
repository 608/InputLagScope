#[cfg(windows)]
mod imp {
    use std::ffi::c_void;
    use std::hint::spin_loop;
    use std::mem::zeroed;
    use std::ptr::{null, null_mut};
    use std::time::{Duration, Instant};

    use anyhow::{anyhow, Result};
    use windows_sys::Win32::Devices::HumanInterfaceDevice::{
        HidD_FreePreparsedData, HidD_GetManufacturerString, HidD_GetPreparsedData,
        HidD_GetProductString, HidD_GetSerialNumberString, HidP_GetCaps, HIDP_CAPS,
    };
    use windows_sys::Win32::Foundation::{
        CloseHandle, GetLastError, ERROR_IO_INCOMPLETE, ERROR_IO_PENDING, HANDLE,
        INVALID_HANDLE_VALUE, WAIT_OBJECT_0, WAIT_TIMEOUT,
    };
    use windows_sys::Win32::Storage::FileSystem::{
        CreateFileW, ReadFile, FILE_ATTRIBUTE_NORMAL, FILE_FLAG_OVERLAPPED, FILE_GENERIC_READ,
        FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
    };
    use windows_sys::Win32::System::Threading::{CreateEventW, ResetEvent, WaitForSingleObject};
    use windows_sys::Win32::System::IO::{CancelIoEx, GetOverlappedResult, OVERLAPPED};

    const BUSY_POLL_COMPLETION_US: u64 = 2_000;

    #[derive(Clone, Debug, Default)]
    pub struct HidDeviceStrings {
        pub product: Option<String>,
        pub manufacturer: Option<String>,
        pub serial_number: Option<String>,
    }

    pub struct WindowsHidReader {
        handle: HANDLE,
        event: HANDLE,
        input_report_bytes: usize,
    }

    impl WindowsHidReader {
        pub fn open(path: &str, fallback_report_bytes: usize) -> Result<Self> {
            let wide_path = to_wide(path);
            let handle = unsafe {
                CreateFileW(
                    wide_path.as_ptr(),
                    FILE_GENERIC_READ,
                    FILE_SHARE_READ | FILE_SHARE_WRITE,
                    null(),
                    OPEN_EXISTING,
                    FILE_ATTRIBUTE_NORMAL | FILE_FLAG_OVERLAPPED,
                    null_mut(),
                )
            };
            if handle == INVALID_HANDLE_VALUE {
                return Err(anyhow!("CreateFileW failed: {}", unsafe { GetLastError() }));
            }

            let event = unsafe { CreateEventW(null(), 1, 0, null()) };
            if event.is_null() {
                unsafe {
                    CloseHandle(handle);
                }
                return Err(anyhow!("CreateEventW failed: {}", unsafe {
                    GetLastError()
                }));
            }

            let input_report_bytes = input_report_bytes(handle)
                .unwrap_or(fallback_report_bytes)
                .clamp(1, 1024);

            Ok(Self {
                handle,
                event,
                input_report_bytes,
            })
        }

        pub fn read_timeout(&self, timeout_ms: u32) -> Result<Option<WindowsHidReport>> {
            let mut buffer = vec![0_u8; self.input_report_bytes];
            let mut bytes_read = 0_u32;
            let mut overlapped = unsafe { zeroed::<OVERLAPPED>() };
            overlapped.hEvent = self.event;

            unsafe {
                ResetEvent(self.event);
            }

            let ok = unsafe {
                ReadFile(
                    self.handle,
                    buffer.as_mut_ptr().cast(),
                    buffer.len() as u32,
                    &mut bytes_read,
                    &mut overlapped,
                )
            };

            if ok != 0 {
                let timestamp_ns = crate::clock::timestamp_ns();
                buffer.truncate(bytes_read as usize);
                return Ok((!buffer.is_empty()).then_some(WindowsHidReport {
                    timestamp_ns,
                    raw: buffer,
                }));
            }

            let error = unsafe { GetLastError() };
            if error != ERROR_IO_PENDING {
                return Err(anyhow!("ReadFile failed: {error}"));
            }

            let busy_deadline = Instant::now() + Duration::from_micros(BUSY_POLL_COMPLETION_US);
            while Instant::now() < busy_deadline {
                let mut transferred = 0_u32;
                let ok =
                    unsafe { GetOverlappedResult(self.handle, &overlapped, &mut transferred, 0) };
                if ok != 0 {
                    let timestamp_ns = crate::clock::timestamp_ns();
                    buffer.truncate(transferred as usize);
                    return Ok((!buffer.is_empty()).then_some(WindowsHidReport {
                        timestamp_ns,
                        raw: buffer,
                    }));
                }

                let error = unsafe { GetLastError() };
                if error != ERROR_IO_INCOMPLETE {
                    unsafe {
                        CancelIoEx(self.handle, &overlapped);
                    }
                    return Err(anyhow!("GetOverlappedResult failed: {error}"));
                }
                spin_loop();
            }

            let wait = unsafe { WaitForSingleObject(self.event, timeout_ms) };
            if wait == WAIT_TIMEOUT {
                unsafe {
                    CancelIoEx(self.handle, &overlapped);
                }
                return Ok(None);
            }
            if wait != WAIT_OBJECT_0 {
                unsafe {
                    CancelIoEx(self.handle, &overlapped);
                }
                return Err(anyhow!("WaitForSingleObject failed: {wait}"));
            }

            let timestamp_ns = crate::clock::timestamp_ns();
            let mut transferred = 0_u32;
            let ok = unsafe { GetOverlappedResult(self.handle, &overlapped, &mut transferred, 0) };
            if ok == 0 {
                return Err(anyhow!("GetOverlappedResult failed: {}", unsafe {
                    GetLastError()
                }));
            }

            buffer.truncate(transferred as usize);
            Ok((!buffer.is_empty()).then_some(WindowsHidReport {
                timestamp_ns,
                raw: buffer,
            }))
        }
    }

    pub struct WindowsHidReport {
        pub timestamp_ns: u64,
        pub raw: Vec<u8>,
    }

    impl Drop for WindowsHidReader {
        fn drop(&mut self) {
            unsafe {
                CloseHandle(self.event);
                CloseHandle(self.handle);
            }
        }
    }

    pub fn read_hid_device_strings(path: &str) -> HidDeviceStrings {
        let wide_path = to_wide(path);
        let handle = unsafe {
            CreateFileW(
                wide_path.as_ptr(),
                0,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                null(),
                OPEN_EXISTING,
                FILE_ATTRIBUTE_NORMAL,
                null_mut(),
            )
        };
        if handle == INVALID_HANDLE_VALUE {
            return HidDeviceStrings::default();
        }

        let strings = HidDeviceStrings {
            product: read_hid_string(handle, HidD_GetProductString),
            manufacturer: read_hid_string(handle, HidD_GetManufacturerString),
            serial_number: read_hid_string(handle, HidD_GetSerialNumberString),
        };
        unsafe {
            CloseHandle(handle);
        }
        strings
    }

    fn input_report_bytes(handle: HANDLE) -> Option<usize> {
        let mut preparsed = 0_isize;
        let ok = unsafe { HidD_GetPreparsedData(handle, &mut preparsed) };
        if !ok || preparsed == 0 {
            return None;
        }

        let mut caps = unsafe { zeroed::<HIDP_CAPS>() };
        let status = unsafe { HidP_GetCaps(preparsed, &mut caps) };
        unsafe {
            HidD_FreePreparsedData(preparsed);
        }

        (status >= 0 && caps.InputReportByteLength > 0)
            .then_some(caps.InputReportByteLength as usize)
    }

    fn read_hid_string(
        handle: HANDLE,
        reader: unsafe extern "system" fn(HANDLE, *mut c_void, u32) -> bool,
    ) -> Option<String> {
        let mut buffer = [0_u16; 256];
        let ok = unsafe {
            reader(
                handle,
                buffer.as_mut_ptr().cast(),
                (buffer.len() * std::mem::size_of::<u16>()) as u32,
            )
        };
        if !ok {
            return None;
        }

        let len = buffer
            .iter()
            .position(|value| *value == 0)
            .unwrap_or(buffer.len());
        let value = String::from_utf16_lossy(&buffer[..len]).trim().to_string();
        (!value.is_empty()).then_some(value)
    }

    fn to_wide(value: &str) -> Vec<u16> {
        value.encode_utf16().chain(std::iter::once(0)).collect()
    }
}

#[cfg(windows)]
pub use imp::{read_hid_device_strings, WindowsHidReader};
