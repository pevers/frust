extern crate hd44780_driver;
extern crate linux_embedded_hal;

use crate::context::Context;
use hd44780_driver::{Cursor, CursorBlink, Display, DisplayMode, HD44780};
use linux_embedded_hal::{Delay, Pin};
use std::fmt::Write;
use sysfs_gpio::Direction;

pub fn init_lcd() -> HD44780<Delay, hd44780_driver::bus::FourBitBus<Pin, Pin, Pin, Pin, Pin, Pin>> {
    let rs = Pin::new(26);
    let en = Pin::new(19);

    let db0 = Pin::new(13);
    let db1 = Pin::new(6);
    let db2 = Pin::new(5);
    let db3 = Pin::new(11);

    rs.export().unwrap();
    en.export().unwrap();

    db0.export().unwrap();
    db1.export().unwrap();
    db2.export().unwrap();
    db3.export().unwrap();

    rs.set_direction(Direction::Low).unwrap();
    en.set_direction(Direction::Low).unwrap();

    db0.set_direction(Direction::Low).unwrap();
    db1.set_direction(Direction::Low).unwrap();
    db2.set_direction(Direction::Low).unwrap();
    db3.set_direction(Direction::Low).unwrap();

    let mut lcd = HD44780::new_4bit(rs, en, db0, db1, db2, db3, Delay);

    lcd.reset();
    lcd.clear();
    lcd.set_display_mode(DisplayMode {
        display: Display::On,
        cursor_visibility: Cursor::Invisible,
        cursor_blink: CursorBlink::Off,
    });

    lcd
}

pub trait PrintStatus {
    fn update(&mut self, context: Context);
}

impl PrintStatus for HD44780<Delay, hd44780_driver::bus::FourBitBus<Pin, Pin, Pin, Pin, Pin, Pin>> {
    fn update(&mut self, context: Context) {
        self.set_cursor_pos(0);
        let first_line = format!(
            "IN:  {:.1}  T:{:.1}",
            context.inside_temp, context.config.target_temp
        );
        self.write_str(&first_line)
            .expect("Could not write to the display");
        self.set_cursor_pos(40);
        let second_line = format!("OUT: {:.1}", context.outside_temp);
        self.write_str(&second_line)
            .expect("Could not write to the display");
    }
}
