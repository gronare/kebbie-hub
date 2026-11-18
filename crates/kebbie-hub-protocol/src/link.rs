use std::net::IpAddr;

use serde::{Deserialize, Serialize};

use crate::message::check;

pub const MAX_LINK_ADDRESSES: usize = 16;

// Where a machine takes direct links from the account's other machines: its addresses on the private networks it is on, and the port it listens on there.
// Unknown fields and networks are tolerated, so a machine that announces more than this version knows never stops an older one reading the rest.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Addresses {
    pub port: u16,
    pub addresses: Vec<LinkAddress>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LinkAddress {
    pub network: Network,
    pub ip: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Network {
    Tailscale,
    Wireguard,
    // A network a newer kebbie announces; a reader skips the address.
    #[serde(other)]
    Other,
}

impl Addresses {
    pub fn check(&self) -> Result<(), &'static str> {
        check(self.port != 0 || self.addresses.is_empty(), "the port is 0")?;
        check(self.addresses.len() <= MAX_LINK_ADDRESSES, "too many addresses")?;
        check(self.addresses.iter().all(|address| is_listening_ip(&address.ip)), "an address is not one ip in its plain form")
    }
}

// One address a listener can bind, written as Rust writes it, so no two spellings name the same address.
pub fn is_listening_ip(text: &str) -> bool {
    text.parse::<IpAddr>().is_ok_and(|ip| !ip.is_unspecified() && !ip.is_multicast() && ip.to_string() == text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::message::Payload;

    fn addresses() -> Addresses {
        Addresses { port: 47_011, addresses: vec![LinkAddress { network: Network::Tailscale, ip: "100.101.102.103".into() }, LinkAddress { network: Network::Wireguard, ip: "fd7a:115c:a1e0::1".into() }] }
    }

    #[test]
    fn addresses_name_plain_ips_on_a_port_and_a_machine_may_announce_none() {
        assert_eq!(Payload::Addresses(addresses()).validate(), Ok(()));
        assert_eq!(Payload::Addresses(Addresses { port: 0, addresses: Vec::new() }).validate(), Ok(()), "a machine that stops listening announces no address");
        let with = |change: &dyn Fn(&mut Addresses)| {
            let mut announced = addresses();
            change(&mut announced);
            Payload::Addresses(announced).validate()
        };
        assert_eq!(with(&|announced| announced.addresses[0].ip = "127.0.0.1".into()), Ok(()), "two machines on one host link over loopback");
        for (refused, change) in [
            ("port 0", &(|announced: &mut Addresses| announced.port = 0) as &dyn Fn(&mut Addresses)),
            ("a host name", &|announced| announced.addresses[0].ip = "mac.tailnet.ts.net".into()),
            ("every interface", &|announced| announced.addresses[0].ip = "0.0.0.0".into()),
            ("every v6 interface", &|announced| announced.addresses[0].ip = "::".into()),
            ("multicast", &|announced| announced.addresses[0].ip = "224.0.0.1".into()),
            ("a port in the address", &|announced| announced.addresses[0].ip = "100.64.0.1:22".into()),
            ("a second spelling", &|announced| announced.addresses[1].ip = "FD7A:115C:A1E0::1".into()),
            ("too many", &|announced| announced.addresses = vec![announced.addresses[0].clone(); MAX_LINK_ADDRESSES + 1]),
        ] {
            assert!(with(change).is_err(), "{refused}");
        }
        let json = serde_json::to_value(Payload::Addresses(addresses())).unwrap();
        assert_eq!(json, serde_json::json!({ "type": "addresses", "port": 47_011, "addresses": [{ "network": "tailscale", "ip": "100.101.102.103" }, { "network": "wireguard", "ip": "fd7a:115c:a1e0::1" }] }));
        let mut newer = json;
        newer["addresses"][0]["name"] = serde_json::json!("mac");
        newer["addresses"][1]["network"] = serde_json::json!("zerotier");
        newer["relay"] = serde_json::json!(true);
        let Payload::Addresses(read) = serde_json::from_value::<Payload>(newer).unwrap() else { panic!("not addresses") };
        assert_eq!((read.addresses[0].clone(), read.addresses[1].network), (addresses().addresses[0].clone(), Network::Other), "a field or network this version does not know is passed over");
    }
}
