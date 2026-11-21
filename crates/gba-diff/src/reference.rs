//! Client for the reference shims, which drive another emulator over a line protocol on
//! stdin/stdout: `mgba-ref` (tools/mgba-ref/mgba_ref.c, mGBA) and `mesen-ref`
//! (tools/mesen-ref/mesen_ref.cpp, Mesen 2). Both speak the same protocol; see
//! docs/mgba-reference.md and docs/mesen-reference.md.

use std::{
    io::{self, BufRead, BufReader, Read, Write},
    path::Path,
    process::{Child, ChildStdin, ChildStdout, Command, Stdio},
};

use gba_core::video::{HEIGHT, WIDTH};

use crate::Registers;

/// Which emulator a reference shim drives.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// mGBA: high-level BIOS calls and a skip-BIOS start at the cartridge entry.
    Mgba,
    /// Mesen 2: no high-level BIOS, so it runs this emulator's firmware from reset.
    Mesen,
}

impl Kind {
    /// Identify a shim from its reply to `name`. `mgba-ref` predates the command and answers
    /// with an error (before a ROM is loaded, "err no ROM loaded").
    fn from_reply(reply: &str) -> Option<Self> {
        if reply == "ok mesen2" {
            Some(Self::Mesen)
        } else if reply.starts_with("err ") {
            Some(Self::Mgba)
        } else {
            None
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Mgba => "mGBA",
            Self::Mesen => "Mesen",
        }
    }
}

pub struct Reference {
    child: Child,
    input: ChildStdin,
    output: BufReader<ChildStdout>,
    program: String,
    kind: Kind,
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
        let program = binary.file_name().map_or_else(
            || binary.display().to_string(),
            |name| name.to_string_lossy().into_owned(),
        );
        let mut reference = Self {
            child,
            input,
            output,
            program,
            kind: Kind::Mgba,
        };
        let reply = reference.exchange("name")?;
        reference.kind = Kind::from_reply(&reply).ok_or_else(|| {
            io::Error::other(format!(
                "{}: not a reference shim (`name` answered `{reply}`)",
                reference.program
            ))
        })?;
        Ok(reference)
    }

    pub fn kind(&self) -> Kind {
        self.kind
    }

    pub fn label(&self) -> &'static str {
        self.kind.label()
    }

    /// Send one command and return its reply line, without the line ending.
    fn exchange(&mut self, line: &str) -> io::Result<String> {
        if line.contains('\n') {
            return Err(io::Error::other("reference commands are single lines"));
        }
        writeln!(self.input, "{line}")?;
        self.input.flush()?;
        let mut reply = String::new();
        if self.output.read_line(&mut reply)? == 0 {
            return Err(io::Error::other(format!(
                "{} exited unexpectedly",
                self.program
            )));
        }
        Ok(reply.trim_end().to_owned())
    }

    fn command(&mut self, line: &str) -> io::Result<String> {
        let reply = self.exchange(line)?;
        match reply.strip_prefix("ok") {
            Some(rest) => Ok(rest.trim_start().to_owned()),
            None => Err(io::Error::other(format!(
                "{}: {reply} (for `{line}`)",
                self.program
            ))),
        }
    }

    pub fn load(&mut self, rom: &Path, bios: Option<&Path>) -> io::Result<()> {
        let path = |path: &Path| {
            let text = path.to_str().unwrap_or_default().to_owned();
            if text.is_empty() || text.contains(char::is_whitespace) {
                Err(io::Error::other(format!(
                    "{}: paths passed to a reference shim must be UTF-8 without whitespace",
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

    /// Load as GBA Eval's reference runs Mesen (`load ROM BIOS eval`): boot skipped, keys
    /// taken at Mesen's VBlank poll. mesen-ref only.
    pub fn load_eval(&mut self, rom: &Path, bios: &Path) -> io::Result<()> {
        let text = |path: &Path| path.to_str().unwrap_or_default().to_owned();
        for path in [rom, bios] {
            let path = text(path);
            if path.is_empty() || path.contains(char::is_whitespace) {
                return Err(io::Error::other(format!(
                    "{path}: paths passed to a reference shim must be UTF-8 without whitespace"
                )));
            }
        }
        self.command(&format!("load {} {} eval", text(rom), text(bios)))
            .map(drop)
    }

    pub fn set_keys(&mut self, mask: u16) -> io::Result<()> {
        self.command(&format!("keys {mask}")).map(drop)
    }

    pub fn write16(&mut self, address: u32, value: u16) -> io::Result<()> {
        self.command(&format!("write16 {address:#x} {value:#x}"))
            .map(drop)
    }

    /// Attach the cartridge RTC and run it on emulated time from `unix` seconds (UTC; mGBA only).
    pub fn set_rtc(&mut self, unix: u64) -> io::Result<()> {
        self.command(&format!("rtc {unix}")).map(drop)
    }

    /// Move mGBA's display clock to `line` at `cycle` before any frame runs (mGBA only).
    pub fn align_video(&mut self, line: u16, cycle: u16) -> io::Result<()> {
        self.command(&format!("video {line} {cycle}")).map(drop)
    }

    /// The next `count` instructions the reference executes outside the BIOS.
    pub fn sync(&mut self, count: usize) -> io::Result<Vec<crate::trace::Record>> {
        let reply = self.command(&format!("sync {count}"))?;
        if reply.parse::<usize>().ok() != Some(count) {
            return Err(io::Error::other(format!("malformed sync reply: {reply}")));
        }
        let mut records = Vec::with_capacity(count);
        for _ in 0..count {
            let mut line = String::new();
            if self.output.read_line(&mut line)? == 0 {
                return Err(io::Error::other(format!(
                    "{} exited during sync",
                    self.program
                )));
            }
            let record = crate::trace::Record::parse(line.trim_end()).ok_or_else(|| {
                io::Error::other(format!("{} sync: {}", self.program, line.trim_end()))
            })?;
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
        // Both shims report mGBA's 32-bit layout: red in the low byte, each channel value << 3.
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

#[cfg(test)]
mod tests {
    use super::Kind;

    #[test]
    fn shims_are_identified_by_their_name_reply() {
        assert_eq!(Kind::from_reply("ok mesen2"), Some(Kind::Mesen));
        // mgba-ref has no `name` command.
        assert_eq!(Kind::from_reply("err no ROM loaded"), Some(Kind::Mgba));
        assert_eq!(Kind::from_reply("err unknown command"), Some(Kind::Mgba));
        assert_eq!(Kind::from_reply("ok something-else"), None);
        assert_eq!(Kind::Mesen.label(), "Mesen");
    }
}
