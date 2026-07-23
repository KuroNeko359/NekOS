use core::fmt::{self, Write};

struct UserWriter;

impl Write for UserWriter {
    fn write_str(&mut self, text: &str) -> fmt::Result {
        if crate::io::write(1, text.as_bytes()) == text.len() as isize {
            Ok(())
        } else {
            Err(fmt::Error)
        }
    }
}

#[doc(hidden)]
pub fn _print(args: fmt::Arguments) {
    let _ = UserWriter.write_fmt(args);
}

#[macro_export]
macro_rules! print {
    ($($arg:tt)*) => {
        $crate::_print(format_args!($($arg)*))
    };
}

#[macro_export]
macro_rules! println {
    () => {
        $crate::print!("\n")
    };
    ($($arg:tt)*) => {
        $crate::print!("{}\n", format_args!($($arg)*))
    };
}
