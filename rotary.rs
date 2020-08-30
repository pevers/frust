#[derive(Debug, PartialEq)]
pub enum Direction {
    Clockwise,
    CounterClockwise,
    None,
}

impl From<u8> for Direction {
    fn from(s: u8) -> Self {
        match s {
            0b1011 | 0b1001 => Direction::Clockwise,
            0b0011 | 0b0001 => Direction::CounterClockwise,
            _ => Direction::None,
        }
    }
}

pub struct Rotary {
    state: u8,
}

impl Rotary {
    pub fn new() -> Self {
        Self { state: 0u8 }
    }

    pub fn update(&mut self, clock: u8, data: u8) -> Direction {
        let mut s = self.state & 0b11;

        // move in the new state
        if clock == 1 {
            s |= 0b100;
        }
        if data == 1 {
            s |= 0b1000;
        }

        // shift new to old
        self.state = s >> 2;

        s.into()
    }
}
