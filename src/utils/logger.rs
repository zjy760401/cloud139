#[macro_export]
macro_rules! info {
    ($($arg:tt)*) => {
        println!("\x1b[36minfo\x1b[0m {}", format!($($arg)*))
    };
}

#[macro_export]
macro_rules! success {
    ($($arg:tt)*) => {
        println!("\x1b[32msuccess\x1b[0m {}", format!($($arg)*))
    };
}

#[macro_export]
macro_rules! warn {
    ($($arg:tt)*) => {
        println!("\x1b[33mwarn\x1b[0m {}", format!($($arg)*))
    };
}

#[macro_export]
macro_rules! error {
    ($($arg:tt)*) => {
        eprintln!("\x1b[31merror\x1b[0m {}", format!($($arg)*))
    };
}

#[macro_export]
macro_rules! step {
    ($($arg:tt)*) => {
        println!("\x1b[34mstep\x1b[0m {}", format!($($arg)*))
    };
}
