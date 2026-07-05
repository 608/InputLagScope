#[cfg(not(test))]
fn main() {
    let mut args = std::env::args().skip(1).collect::<Vec<_>>();
    if args.first().is_some_and(|arg| arg == "--headless-measure") {
        args.remove(0);
        attach_parent_console();
        if let Err(error) = inputlagscope_lib::run_headless_cli(args) {
            eprintln!("{error:#}");
            std::process::exit(1);
        }
        return;
    }

    detach_console_for_gui();
    inputlagscope_lib::run();
}

#[cfg(all(not(test), windows))]
fn attach_parent_console() {
    use windows_sys::Win32::System::Console::{AttachConsole, ATTACH_PARENT_PROCESS};
    // Continue headless mode if attaching to the console fails.
    unsafe {
        let _ = AttachConsole(ATTACH_PARENT_PROCESS);
    }
}

#[cfg(all(not(test), windows))]
fn detach_console_for_gui() {
    use windows_sys::Win32::System::Console::FreeConsole;
    unsafe {
        let _ = FreeConsole();
    }
}

#[cfg(all(not(test), not(windows)))]
fn attach_parent_console() {}

#[cfg(all(not(test), not(windows)))]
fn detach_console_for_gui() {}

#[cfg(test)]
fn main() {}
