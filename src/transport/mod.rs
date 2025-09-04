use getifaddrs::{Interfaces, getifaddrs};
use std::net::IpAddr;

pub mod mqtt;
pub mod stream;
pub mod udp;

pub fn if_index_by_addr(if_address: &IpAddr) -> Result<u32, std::io::Error> {
    if if_address.is_unspecified() {
        return Ok(0);
    }
    let interfaces = getifaddrs().unwrap().collect::<Interfaces>();

    for (_, interface) in interfaces {
        for addr in interface.address.iter().flatten() {
            if let Some(ip_addr) = addr.ip_addr() {
                if *if_address == ip_addr {
                    if let Some(index) = interface.index {
                        return Ok(index);
                    } else {
                        return Err(std::io::Error::new(
                            std::io::ErrorKind::NotFound,
                            format!(
                                "Interface {} is present for address {}, but index is not available",
                                interface.name, if_address
                            ),
                        ));
                    }
                }
            }
        }
    }
    Err(std::io::Error::new(
        std::io::ErrorKind::NotFound,
        "Interface not found",
    ))
}
