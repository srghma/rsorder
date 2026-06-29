use std::fmt;

pub fn keep_first() -> u32 { keep_second() }

pub fn keep_second() -> u32 { 0 }

// TO REORDER same-level-outside-of-mutual--alphabetically

const BETA: u32 = 2;

pub fn alpha() -> u32 { 1 }

pub fn zeta() -> u32 { alpha() }

// TO REORDER END

// TO REORDER

// mutual start
fn even(n: u32) -> u32 { if n == 0 { 1 } else { odd(n - 1) } }

fn odd(n: u32) -> u32 { if n == 0 { 0 } else { even(n - 1) } }
// mutual end

fn user() -> u32 { even(4) }

// TO REORDER END

pub fn keep_last() -> u32 { keep_first() }
