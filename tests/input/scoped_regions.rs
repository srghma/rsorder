use std::fmt;

pub fn keep_first() -> u32 {
    keep_second()
}
pub fn keep_second() -> u32 {
    0
}

// TO REORDER sorting-non-mutual=alphabetical
pub fn zeta() -> u32 {
    alpha()
}
pub fn alpha() -> u32 {
    1
}
const BETA: u32 = 2;
// TO REORDER END

// TO REORDER
fn user() -> u32 {
    even(4)
}
fn even(n: u32) -> u32 {
    if n == 0 { 1 } else { odd(n - 1) }
}
fn odd(n: u32) -> u32 {
    if n == 0 { 0 } else { even(n - 1) }
}
// TO REORDER END

pub fn keep_last() -> u32 {
    keep_first()
}
