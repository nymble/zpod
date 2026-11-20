//! Client side for the lab apps (runs as the app user).

use std::io::{self, BufRead, BufReader, Read, Write};
use std::os::unix::net::UnixStream;
use std::time::Duration;

fn connect() -> io::Result<UnixStream> {
    let p = std::env::var("ZPOD_LABD_SOCKET").unwrap_or_else(|_| crate::SOCKET.to_string());
    UnixStream::connect(p)
}

/// Send one command, return the `ok` text or the `err` reason as Err.
pub fn request(cmd: &str) -> Result<String, String> {
    request_timeout(cmd, Duration::from_secs(15))
}

/// Like [`request`], with an explicit read timeout (e.g. 90s for `wlan-tx-rid`).
pub fn request_timeout(cmd: &str, timeout: Duration) -> Result<String, String> {
    let mut s = connect().map_err(|e| format!("lab helper not running ({e})"))?;
    s.set_read_timeout(Some(timeout)).ok();
    s.set_write_timeout(Some(Duration::from_secs(5))).ok();
    s.write_all(format!("{cmd}\n").as_bytes()).map_err(|e| e.to_string())?;
    let mut line = String::new();
    BufReader::new(&s).read_line(&mut line).map_err(|e| e.to_string())?;
    reply(&line)
}

fn reply(line: &str) -> Result<String, String> {
    let l = line.trim();
    if let Some(r) = l.strip_prefix("ok") {
        Ok(r.trim().to_string())
    } else if let Some(r) = l.strip_prefix("err") {
        Err(r.trim().to_string())
    } else {
        Err(format!("bad reply {l:?}"))
    }
}

/// An open stream; `next()` returns the next non-empty frame.
pub struct Stream {
    r: BufReader<UnixStream>,
}

impl Stream {
    pub fn open(cmd: &str) -> Result<Stream, String> {
        let mut s = connect().map_err(|e| format!("lab helper not running ({e})"))?;
        s.write_all(format!("{cmd}\n").as_bytes()).map_err(|e| e.to_string())?;
        let mut r = BufReader::new(s);
        let mut line = String::new();
        r.read_line(&mut line).map_err(|e| e.to_string())?;
        reply(&line)?;
        Ok(Stream { r })
    }

    /// A handle that can end the stream from another thread
    /// (`shutdown(Both)` makes `next()` fail).
    pub fn closer(&self) -> io::Result<UnixStream> {
        self.r.get_ref().try_clone()
    }

    /// Blocks; Err when the daemon closes the stream.
    pub fn next(&mut self) -> io::Result<Vec<u8>> {
        loop {
            let mut h = [0u8; 4];
            self.r.read_exact(&mut h)?;
            let n = u32::from_le_bytes(h) as usize;
            if n == 0 {
                continue;
            }
            if n > 65536 {
                return Err(io::Error::new(io::ErrorKind::InvalidData, "frame too large"));
            }
            let mut b = vec![0u8; n];
            self.r.read_exact(&mut b)?;
            return Ok(b);
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn replies() {
        assert_eq!(super::reply("ok zpod-labd 1\n"), Ok("zpod-labd 1".into()));
        assert_eq!(super::reply("err nope\n"), Err("nope".into()));
        assert!(super::reply("what").is_err());
    }
}
