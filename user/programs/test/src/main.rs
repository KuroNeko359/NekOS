#![no_std]
#![no_main]

use userlib::{entry, println};

fn main() {
    println!("Test");
}

entry!(main);
