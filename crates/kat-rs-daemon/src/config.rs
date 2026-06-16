use std::net::{IpAddr, Ipv4Addr, SocketAddr};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DaemonConfig {
    pub host: IpAddr,
    pub port: u16,
}

impl Default for DaemonConfig {
    fn default() -> Self {
        Self {
            host: IpAddr::V4(Ipv4Addr::LOCALHOST),
            port: 3030,
        }
    }
}

impl DaemonConfig {
    pub fn socket_addr(self) -> SocketAddr {
        SocketAddr::new(self.host, self.port)
    }
}
