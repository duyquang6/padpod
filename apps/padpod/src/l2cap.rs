//! Raw Bluetooth L2CAP sockets: the two HID channels.
//!
//! libc has no Bluetooth types, so the address structure is declared here.

use std::io;
use std::os::unix::io::{AsRawFd, FromRawFd, OwnedFd, RawFd};

const AF_BLUETOOTH: libc::c_int = 31;
const BTPROTO_L2CAP: libc::c_int = 0;

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct SockaddrL2 {
    family: libc::sa_family_t,
    psm: u16,
    /// Little-endian: the last octet of the printed address comes first.
    bdaddr: [u8; 6],
    cid: u16,
    bdaddr_type: u8,
}

/// A Bluetooth device address, stored as printed (most significant first).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Addr(pub [u8; 6]);

impl Addr {
    pub fn parse(text: &str) -> Option<Self> {
        let parts: Vec<u8> = text.trim().split(':').map(|p| u8::from_str_radix(p, 16).ok()).collect::<Option<_>>()?;
        Some(Self(parts.try_into().ok()?))
    }

    fn to_wire(self) -> [u8; 6] {
        let mut b = self.0;
        b.reverse();
        b
    }

    fn from_wire(mut b: [u8; 6]) -> Self {
        b.reverse();
        Self(b)
    }
}

impl std::fmt::Display for Addr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let [a, b, c, d, e, g] = self.0;
        write!(f, "{a:02X}:{b:02X}:{c:02X}:{d:02X}:{e:02X}:{g:02X}")
    }
}

fn socket() -> io::Result<OwnedFd> {
    let fd = unsafe { libc::socket(AF_BLUETOOTH, libc::SOCK_SEQPACKET | libc::SOCK_CLOEXEC, BTPROTO_L2CAP) };
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(unsafe { OwnedFd::from_raw_fd(fd) })
}

fn addr(psm: u16, bdaddr: [u8; 6]) -> SockaddrL2 {
    SockaddrL2 { family: AF_BLUETOOTH as _, psm: psm.to_le(), bdaddr, ..Default::default() }
}

const ADDR_LEN: libc::socklen_t = std::mem::size_of::<SockaddrL2>() as _;

/// A channel waiting for the host to connect.
pub struct Listener(OwnedFd);

impl Listener {
    pub fn bind(psm: u16) -> io::Result<Self> {
        let fd = socket()?;
        let a = addr(psm, [0; 6]);
        if unsafe { libc::bind(fd.as_raw_fd(), &a as *const _ as *const libc::sockaddr, ADDR_LEN) } < 0 {
            return Err(io::Error::last_os_error());
        }
        if unsafe { libc::listen(fd.as_raw_fd(), 1) } < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(Self(fd))
    }

    /// Take a waiting connection, or None if there is none yet.
    pub fn accept(&self) -> io::Result<Option<(Channel, Addr)>> {
        let mut a = SockaddrL2::default();
        let mut len = ADDR_LEN;
        let fd = unsafe {
            libc::accept4(
                self.0.as_raw_fd(),
                &mut a as *mut _ as *mut libc::sockaddr,
                &mut len,
                libc::SOCK_CLOEXEC,
            )
        };
        if fd < 0 {
            let e = io::Error::last_os_error();
            return if e.kind() == io::ErrorKind::WouldBlock { Ok(None) } else { Err(e) };
        }
        let channel = Channel(unsafe { OwnedFd::from_raw_fd(fd) });
        Ok(Some((channel, Addr::from_wire(a.bdaddr))))
    }

    pub fn set_nonblocking(&self) -> io::Result<()> {
        set_nonblocking(self.0.as_raw_fd())
    }
}

impl AsRawFd for Listener {
    fn as_raw_fd(&self) -> RawFd {
        self.0.as_raw_fd()
    }
}

/// One open HID channel.
pub struct Channel(OwnedFd);

impl Channel {
    /// Connect out to a host, blocking until the link is up or refused. Used to
    /// reconnect to the host already paired with, which is the device's job
    /// in the HID profile.
    pub fn connect(to: Addr, psm: u16) -> io::Result<Self> {
        let fd = socket()?;
        let a = addr(psm, to.to_wire());
        if unsafe { libc::connect(fd.as_raw_fd(), &a as *const _ as *const libc::sockaddr, ADDR_LEN) } < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(Self(fd))
    }

    /// Never blocks: with the radio behind, a full send queue is reported as
    /// WouldBlock and the caller moves on, rather than the loop that reads
    /// the pad stalling until the queue drains - felt as input lag.
    pub fn send(&self, bytes: &[u8]) -> io::Result<()> {
        let flags = libc::MSG_NOSIGNAL | libc::MSG_DONTWAIT;
        let n = unsafe { libc::send(self.0.as_raw_fd(), bytes.as_ptr() as *const _, bytes.len(), flags) };
        if n < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    /// One message, or None if nothing is waiting. A closed channel is an
    /// error, so the caller has one place to notice the host leaving.
    pub fn recv(&self) -> io::Result<Option<Vec<u8>>> {
        let mut buf = [0u8; 256];
        let n = unsafe {
            libc::recv(self.0.as_raw_fd(), buf.as_mut_ptr() as *mut _, buf.len(), libc::MSG_DONTWAIT)
        };
        if n < 0 {
            let e = io::Error::last_os_error();
            return if e.kind() == io::ErrorKind::WouldBlock { Ok(None) } else { Err(e) };
        }
        if n == 0 {
            return Err(io::Error::new(io::ErrorKind::ConnectionAborted, "the host closed the channel"));
        }
        Ok(Some(buf[..n as usize].to_vec()))
    }
}

impl AsRawFd for Channel {
    fn as_raw_fd(&self) -> RawFd {
        self.0.as_raw_fd()
    }
}

fn set_nonblocking(fd: RawFd) -> io::Result<()> {
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    if flags < 0 || unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_address_structure_matches_the_kernels() {
        // struct sockaddr_l2 is 14 bytes on Linux.
        assert_eq!(std::mem::size_of::<SockaddrL2>(), 14);
    }

    #[test]
    fn addresses_print_as_written_and_go_out_reversed() {
        let a = Addr::parse("12:34:56:78:9A:BC").unwrap();
        assert_eq!(a.to_string(), "12:34:56:78:9A:BC");
        assert_eq!(a.to_wire(), [0xBC, 0x9A, 0x78, 0x56, 0x34, 0x12]);
        assert_eq!(Addr::from_wire(a.to_wire()), a);
        assert_eq!(Addr::parse("nonsense"), None);
        assert_eq!(Addr::parse("12:34:56:78:9A"), None);
    }
}
