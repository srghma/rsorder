use std::collections::HashMap;

pub struct Helper {
    cache: HashMap<u32, u32>,
}

pub struct Config {
    pub value: u32,
}

// mutual start
fn ping(n: u32) -> u32 {
    if n == 0 { 0 } else { pong(n - 1) }
}

fn pong(n: u32) -> u32 {
    if n == 0 { 1 } else { ping(n - 1) }
}
// mutual end

pub fn run(c: Config) -> u32 {
    Helper::new().apply(c.value) + ping(3)
}

const SCALE: u32 = 10;

impl Helper {
    fn new() -> Helper {
        Helper {
            cache: HashMap::new(),
        }
    }
    fn apply(&self, v: u32) -> u32 {
        v * SCALE
    }
}
