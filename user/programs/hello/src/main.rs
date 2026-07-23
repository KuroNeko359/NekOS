#![no_std]
#![no_main]

use userlib::{entry, println};

fn main() {
    println!("Hello from Rust hello program!");
}

entry!(main);
