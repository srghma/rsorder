// A small out-of-order example. Run: rsorder sample/demo.rs --stdout

use std::collections::HashMap;

/// `run` is defined first but uses everything below it.
pub fn run(c: Config) -> u32 {
    Helper::new().apply(c.value) + ping(3)
}

const SCALE: u32 = 10;

pub struct Helper { cache: HashMap<u32, u32> }

impl Helper {
    fn new() -> Helper { Helper { cache: HashMap::new() } }
    fn apply(&self, v: u32) -> u32 { v * SCALE }
}

pub struct Config { pub value: u32 }

// ping and pong are mutually recursive -> wrapped in a mutual block
fn ping(n: u32) -> u32 { if n == 0 { 0 } else { pong(n - 1) } }
fn pong(n: u32) -> u32 { if n == 0 { 1 } else { ping(n - 1) } }
