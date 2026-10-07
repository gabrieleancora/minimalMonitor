use std::{io, net::IpAddr};

const PAYLOAD: &[u8; 16] = b"wp00-echo-proof!";

pub fn echo(address: IpAddr) -> io::Result<()> {
    // This feasibility program only accepts its explicitly authorized local targets.
    if !address.is_loopback() {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "fixture policy",
        ));
    }
    platform::echo(address)
}

#[cfg(windows)]
mod platform {
    use super::*;
    use windows_sys::Win32::{
        Foundation::{HANDLE, INVALID_HANDLE_VALUE},
        NetworkManagement::IpHelper::*,
        Networking::WinSock::*,
    };

    struct Handle(HANDLE);
    impl Drop for Handle {
        fn drop(&mut self) {
            // SAFETY: only successfully created ICMP handles enter this owner;
            // all synchronous requests have returned before it is dropped.
            unsafe {
                IcmpCloseHandle(self.0);
            }
        }
    }

    pub fn echo(address: IpAddr) -> io::Result<()> {
        // u64 backing gives reply structures sufficient alignment; 1024 bytes
        // exceeds header + payload + ICMP error + IO_STATUS_BLOCK requirements.
        let mut reply = [0u64; 128];
        // SAFETY: all API pointers reference initialized buffers/structures of
        // adequate size alive for the full synchronous call. No APC/event is
        // provided, so Windows cannot retain these pointers after return.
        unsafe {
            let raw = if address.is_ipv4() {
                IcmpCreateFile()
            } else {
                Icmp6CreateFile()
            };
            if raw == INVALID_HANDLE_VALUE {
                return Err(io::Error::last_os_error());
            }
            let handle = Handle(raw);
            let count = match address {
                IpAddr::V4(ip) => IcmpSendEcho(
                    handle.0,
                    u32::from_ne_bytes(ip.octets()),
                    PAYLOAD.as_ptr().cast(),
                    PAYLOAD.len() as u16,
                    std::ptr::null(),
                    reply.as_mut_ptr().cast(),
                    std::mem::size_of_val(&reply) as u32,
                    5000,
                ),
                IpAddr::V6(ip) => {
                    let mut source: SOCKADDR_IN6 = std::mem::zeroed();
                    source.sin6_family = AF_INET6;
                    let mut destination = source;
                    destination.sin6_addr.u.Byte = ip.octets();
                    Icmp6SendEcho2(
                        handle.0,
                        std::ptr::null_mut(),
                        None,
                        std::ptr::null(),
                        &source,
                        &destination,
                        PAYLOAD.as_ptr().cast(),
                        PAYLOAD.len() as u16,
                        std::ptr::null(),
                        reply.as_mut_ptr().cast(),
                        std::mem::size_of_val(&reply) as u32,
                        5000,
                    )
                }
            };
            if count == 0 {
                return Err(io::Error::last_os_error());
            }
            let valid = match address {
                IpAddr::V4(ip) => {
                    let parsed = &*reply.as_ptr().cast::<ICMP_ECHO_REPLY>();
                    let start = parsed.Data as usize;
                    let low = reply.as_ptr() as usize;
                    let high = low + std::mem::size_of_val(&reply);
                    parsed.Status == 0
                        && parsed.Address == u32::from_ne_bytes(ip.octets())
                        && parsed.DataSize as usize == PAYLOAD.len()
                        && start >= low
                        && start
                            .checked_add(PAYLOAD.len())
                            .is_some_and(|end| end <= high)
                        && std::slice::from_raw_parts(parsed.Data.cast::<u8>(), PAYLOAD.len())
                            == PAYLOAD
                }
                IpAddr::V6(ip) => {
                    let parsed = &*reply.as_ptr().cast::<ICMPV6_ECHO_REPLY_LH>();
                    let offset = std::mem::size_of::<ICMPV6_ECHO_REPLY_LH>();
                    let bytes = std::slice::from_raw_parts(
                        reply.as_ptr().cast::<u8>().add(offset),
                        PAYLOAD.len(),
                    );
                    let source: [u8; 16] = std::mem::transmute(parsed.Address.sin6_addr);
                    parsed.Status == 0 && source == ip.octets() && bytes == PAYLOAD
                }
            };
            // Echo type/request correlation is provided by the native synchronous
            // API; additionally check status, source, and returned payload here.
            if valid {
                Ok(())
            } else {
                Err(io::Error::other("invalid echo reply"))
            }
        }
    }
}

#[cfg(target_os = "linux")]
mod platform {
    use super::*;
    use socket2::{Domain, Protocol, Socket, Type};
    use std::{
        net::{SocketAddr, UdpSocket},
        time::{Duration, Instant},
    };

    fn checksum(bytes: &[u8]) -> u16 {
        let mut sum = bytes
            .chunks_exact(2)
            .map(|b| u16::from_be_bytes([b[0], b[1]]) as u32)
            .sum::<u32>();
        while sum >> 16 != 0 {
            sum = (sum & 0xffff) + (sum >> 16);
        }
        !(sum as u16)
    }

    fn valid_reply(bytes: &[u8], v6: bool, identifier: u16) -> bool {
        bytes.len() == 8 + PAYLOAD.len()
            && bytes[0] == if v6 { 129 } else { 0 }
            && bytes[1] == 0
            && bytes[4..6] == identifier.to_be_bytes()
            && bytes[6..8] == 1u16.to_be_bytes()
            && bytes[8..] == *PAYLOAD
    }

    pub fn echo(address: IpAddr) -> io::Result<()> {
        let v6 = address.is_ipv6();
        // No raw-socket fallback. PermissionDenied is a local setup fault.
        let socket = Socket::new(
            if v6 { Domain::IPV6 } else { Domain::IPV4 },
            Type::DGRAM,
            Some(if v6 {
                Protocol::ICMPV6
            } else {
                Protocol::ICMPV4
            }),
        )?;
        socket.connect(&SocketAddr::new(address, 0).into())?;
        let socket: UdpSocket = socket.into();
        // Linux assigns the echo identifier as the datagram socket's local port.
        let identifier = socket.local_addr()?.port();
        let mut request = [0; 24];
        request[0] = if v6 { 128 } else { 8 };
        request[4..6].copy_from_slice(&identifier.to_be_bytes());
        request[6..8].copy_from_slice(&1u16.to_be_bytes());
        request[8..].copy_from_slice(PAYLOAD);
        if !v6 {
            let sum = checksum(&request);
            request[2..4].copy_from_slice(&sum.to_be_bytes());
        }
        // IPv6 checksum includes a pseudoheader and is computed by the kernel.
        let deadline = Instant::now() + Duration::from_secs(5);
        socket.send(&request)?;
        let mut reply = [0; 512];
        for _ in 0..8 {
            let remaining = deadline
                .checked_duration_since(Instant::now())
                .ok_or_else(|| io::Error::new(io::ErrorKind::TimedOut, "echo deadline"))?;
            socket.set_read_timeout(Some(remaining))?;
            let (len, source) = socket.recv_from(&mut reply)?;
            if source.ip() == address && valid_reply(&reply[..len], v6, identifier) {
                return Ok(());
            }
        }
        Err(io::Error::other("invalid echo replies"))
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        #[test]
        fn rejects_wrong_type_identifier_sequence_and_payload() {
            let mut packet = [0; 24];
            packet[4..6].copy_from_slice(&12u16.to_be_bytes());
            packet[6..8].copy_from_slice(&1u16.to_be_bytes());
            packet[8..].copy_from_slice(PAYLOAD);
            assert!(valid_reply(&packet, false, 12));
            for index in [0, 1, 4, 6, 8] {
                let mut invalid = packet;
                invalid[index] ^= 1;
                assert!(!valid_reply(&invalid, false, 12));
            }
        }
    }
}

#[cfg(not(any(windows, target_os = "linux")))]
mod platform {
    use super::*;
    pub fn echo(_: IpAddr) -> io::Result<()> {
        Err(io::ErrorKind::Unsupported.into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn local_ipv4_ipv6_echo() -> io::Result<()> {
        echo("127.0.0.1".parse().unwrap())?;
        echo("::1".parse().unwrap())
    }
    #[test]
    fn rejects_outside_fixture_policy() {
        assert_eq!(
            echo("192.0.2.1".parse().unwrap()).unwrap_err().kind(),
            io::ErrorKind::PermissionDenied
        );
    }
}
