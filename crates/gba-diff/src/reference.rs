//! Client for the `mgba-ref` shim (tools/mgba-ref/mgba_ref.c), which drives a locally
//! built mGBA core over a line protocol on stdin/stdout.

use std::{
    io::{self, BufRead, BufReader, Read, Write},
    path::Path,
    process::{Child, ChildStdin, ChildStdout, Command, Stdio},
};

use gba_core::video::{HEIGHT, WIDTH};

use crate::Registers;

pub struct Reference {
    child: Child,
    input: ChildStdin,
    output: BufReader<ChildStdout>,
}

impl Reference {
    pub fn spawn(binary: &Path) -> io::Result<Self> {
        let mut child = Command::new(binary)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .map_err(|error| {
                io::Error::new(error.kind(), format!("{}: {error}", binary.display()))
            })?;
        let input = child.stdin.take().expect("piped stdin");
        let output = BufReader::new(child.stdout.take().expect("piped stdout"));
        Ok(Self {
            child,
            input,
            output,
        })
    }

    fn command(&mut self, line: &str) -> io::Result<String> {
        if line.contains('\n') {
            return Err(io::Error::other("reference commands are single lines"));
        }
        writeln!(self.input, "{line}")?;
        self.input.flush()?;
        let mut reply = String::new();
        if self.output.read_line(&mut reply)? == 0 {
            return Err(io::Error::other("mgba-ref exited unexpectedly"));
        }
        let reply = reply.trim_end();
        match reply.strip_prefix("ok") {
            Some(rest) => Ok(rest.trim_start().to_owned()),
            None => Err(io::Error::other(format!(
                "mgba-ref: {reply} (for `{line}`)"
            ))),
        }
    }

    pub fn load(&mut self, rom: &Path, bios: Option<&Path>) -> io::Result<()> {
        let path = |path: &Path| {
            let text = path.to_str().unwrap_or_default().to_owned();
            if text.is_empty() || text.contains(char::is_whitespace) {
                Err(io::Error::other(format!(
                    "{}: paths passed to mgba-ref must be UTF-8 without whitespace",
                    path.display()
                )))
            } else {
                Ok(text)
            }
        };
        let mut line = format!("load {}", path(rom)?);
        if let Some(bios) = bios {
            line = format!("{line} {}", path(bios)?);
        }
        self.command(&line).map(drop)
    }

    pub fn set_keys(&mut self, mask: u16) -> io::Result<()> {
        self.command(&format!("keys {mask}")).map(drop)
    }

    pub fn write16(&mut self, address: u32, value: u16) -> io::Result<()> {
        self.command(&format!("write16 {address:#x} {value:#x}"))
            .map(drop)
    }

    /// Move mGBA's display clock to `line` at `cycle` before any frame runs.
    pub fn align_video(&mut self, line: u16, cycle: u16) -> io::Result<()> {
        self.command(&format!("video {line} {cycle}")).map(drop)
    }

    /// The next `count` instructions mGBA executes outside the BIOS.
    pub fn sync(&mut self, count: usize) -> io::Result<Vec<crate::trace::Record>> {
        let reply = self.command(&format!("sync {count}"))?;
        if reply.parse::<usize>().ok() != Some(count) {
            return Err(io::Error::other(format!("malformed sync reply: {reply}")));
        }
        let mut records = Vec::with_capacity(count);
        for _ in 0..count {
            let mut line = String::new();
            if self.output.read_line(&mut line)? == 0 {
                return Err(io::Error::other("mgba-ref exited during sync"));
            }
            let record = crate::trace::Record::parse(line.trim_end())
                .ok_or_else(|| io::Error::other(format!("mgba-ref sync: {}", line.trim_end())))?;
            records.push(record);
        }
        Ok(records)
    }

    pub fn run_frame(&mut self) -> io::Result<()> {
        self.command("frame").map(drop)
    }

    pub fn registers(&mut self) -> io::Result<Registers> {
        let reply = self.command("regs")?;
        let fields: Vec<&str> = reply.split_whitespace().collect();
        if fields.len() != 19 {
            return Err(io::Error::other(format!("malformed regs reply: {reply}")));
        }
        let hex = |text: &str| {
            u32::from_str_radix(text, 16)
                .map_err(|_| io::Error::other(format!("malformed register value: {text}")))
        };
        let mut gprs = [0; 16];
        for (slot, text) in gprs.iter_mut().zip(&fields) {
            *slot = hex(text)?;
        }
        Ok(Registers {
            gprs,
            cpsr: hex(fields[16])?,
            thumb: fields[18] == "thumb",
        })
    }

    pub fn read(&mut self, address: u32, length: usize) -> io::Result<Vec<u8>> {
        let reply = self.command(&format!("read {address:#x} {length}"))?;
        if reply.parse::<usize>().ok() != Some(length) {
            return Err(io::Error::other(format!("malformed read reply: {reply}")));
        }
        let mut bytes = vec![0; length];
        self.output.read_exact(&mut bytes)?;
        Ok(bytes)
    }

    /// The last completed frame as RGB555 values.
    pub fn frame(&mut self) -> io::Result<Vec<u16>> {
        let reply = self.command("fb")?;
        if reply != format!("{WIDTH} {HEIGHT}") {
            return Err(io::Error::other(format!("malformed fb reply: {reply}")));
        }
        let mut bytes = vec![0; WIDTH * HEIGHT * 4];
        self.output.read_exact(&mut bytes)?;
        // mGBA's 32-bit color keeps red in the low byte: each channel is value << 3.
        Ok(bytes
            .chunks_exact(4)
            .map(|pixel| {
                let value = u32::from_le_bytes([pixel[0], pixel[1], pixel[2], pixel[3]]);
                let red = (value >> 3) & 31;
                let green = (value >> 11) & 31;
                let blue = (value >> 19) & 31;
                (red | green << 5 | blue << 10) as u16
            })
            .collect())
    }
}

impl Drop for Reference {
    fn drop(&mut self) {
        let _ = writeln!(self.input, "quit");
        let _ = self.input.flush();
        let _ = self.child.wait();
    }
}
