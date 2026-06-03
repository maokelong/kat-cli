use std::io::{self, Write};

pub fn write_line(out: &mut dyn Write, line: &str) -> io::Result<()> {
    writeln!(out, "{line}")
}
