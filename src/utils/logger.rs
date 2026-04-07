use std::sync::atomic::{AtomicBool, Ordering};

/// 全局静默模式标志
pub static QUIET: AtomicBool = AtomicBool::new(false);

pub fn set_quiet(quiet: bool) {
    QUIET.store(quiet, Ordering::Relaxed);
}

pub fn is_quiet() -> bool {
    QUIET.load(Ordering::Relaxed)
}

#[macro_export]
macro_rules! info {
    ($($arg:tt)*) => {
        if !$crate::utils::logger::is_quiet() {
            println!("\x1b[36minfo\x1b[0m {}", format!($($arg)*))
        }
    };
}

#[macro_export]
macro_rules! success {
    ($($arg:tt)*) => {
        if !$crate::utils::logger::is_quiet() {
            println!("\x1b[32msuccess\x1b[0m {}", format!($($arg)*))
        }
    };
}

#[macro_export]
macro_rules! warn {
    ($($arg:tt)*) => {
        if !$crate::utils::logger::is_quiet() {
            println!("\x1b[33mwarn\x1b[0m {}", format!($($arg)*))
        }
    };
}

#[macro_export]
macro_rules! error {
    ($($arg:tt)*) => {
        if !$crate::utils::logger::is_quiet() {
            eprintln!("\x1b[31merror\x1b[0m {}", format!($($arg)*))
        }
    };
}

#[macro_export]
macro_rules! step {
    ($($arg:tt)*) => {
        if !$crate::utils::logger::is_quiet() {
            println!("\x1b[34mstep\x1b[0m {}", format!($($arg)*))
        }
    };
}
