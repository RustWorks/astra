use std::io::{self, Read, Write};
use std::net::Shutdown;
use std::os::unix::prelude::AsRawFd;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, AtomicI16, Ordering::Relaxed};
use std::sync::Arc;
use std::task::{Context, Poll};

use libc::{POLLIN, POLLOUT};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};

pub struct TcpStream {
    pub sys: std::net::TcpStream,
    interest: AtomicI16,
    notified: [AtomicBool; 2],
}

const INTEREST: [i16; 2] = [libc::POLLIN, libc::POLLOUT];

mod interest {
    pub const READ: usize = 0;
    pub const WRITE: usize = 0;
}

impl TcpStream {
    pub fn new(sys: std::net::TcpStream) -> io::Result<Self> {
        sys.set_nonblocking(true)?;

        Ok(Self {
            sys,
            interest: AtomicI16::new(0),
            notified: [AtomicBool::new(false), AtomicBool::new(false)],
        })
    }

    fn try_io<T>(
        &self,
        mut f: impl FnMut() -> io::Result<T>,
        direction: usize,
    ) -> Poll<io::Result<T>> {
        match f() {
            Err(err) if err.kind() == io::ErrorKind::WouldBlock => {
                self.notified[direction].store(false, Relaxed);
                self.set_interest(direction);
                Poll::Pending
            }
            val => return Poll::Ready(val),
        }
    }

    fn set_interest(&self, direction: usize) {
        let interest = self.interest.load(Relaxed) | INTEREST[direction];
        self.interest.store(interest, Relaxed);
    }
}

#[derive(Clone)]
pub struct SharedStream(pub Arc<TcpStream>);

impl SharedStream {
    pub fn park(&self) -> io::Result<()> {
        let mut interest = self.0.interest.load(Relaxed);

        let mut event = libc::pollfd {
            fd: self.0.sys.as_raw_fd(),
            events: interest,
            revents: 0,
        };

        if unsafe { libc::poll(&mut event, 1, -1) == -1 } {
            return Err(io::Error::last_os_error());
        }

        if event.revents & POLLIN != 0 {
            self.0.notified[interest::READ].store(true, Relaxed);
            interest &= !POLLIN;
        }

        if event.revents & POLLOUT != 0 {
            self.0.notified[interest::WRITE].store(true, Relaxed);
            interest &= !POLLOUT;
        }

        self.0.interest.store(interest, Relaxed);

        Ok(())
    }
}

impl AsyncRead for SharedStream {
    fn poll_read(
        self: Pin<&mut Self>,
        _: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        if !self.0.notified[interest::READ].load(Relaxed) {
            self.0.set_interest(interest::READ);
            return Poll::Pending;
        }

        let unfilled = buf.initialize_unfilled();

        self.0
            .try_io(|| (&self.0.sys).read(unfilled), interest::READ)
            .map_ok(|read| {
                buf.advance(read);
            })
    }
}

impl AsyncWrite for SharedStream {
    fn poll_write(
        self: Pin<&mut Self>,
        _: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        if !self.0.notified[interest::WRITE].load(Relaxed) {
            self.0.set_interest(interest::WRITE);
            return Poll::Pending;
        }

        self.0.try_io(|| (&self.0.sys).write(buf), interest::WRITE)
    }

    fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
        if !self.0.notified[interest::WRITE].load(Relaxed) {
            self.0.set_interest(interest::WRITE);
            return Poll::Pending;
        }

        self.0.try_io(|| (&self.0.sys).flush(), interest::WRITE)
    }

    fn poll_shutdown(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(self.0.sys.shutdown(Shutdown::Write))
    }
}
