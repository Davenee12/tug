//! Which address the phone should use to reach this PC: the IPv4 address of the adapter that has
//! the default gateway (the home Wi-Fi or Ethernet), never loopback, VPNs, Hyper-V/WSL vEthernet,
//! Docker or link-local. Drop listens only on that address.

use std::net::Ipv4Addr;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Wifi,
    Ethernet,
    Loopback,
    /// Tunnels and dial-up (PPP): VPNs, Teredo and friends.
    Tunnel,
    Other,
}

#[derive(Debug, Clone)]
pub struct Adapter {
    /// Windows' friendly name ("Wi-Fi", "vEthernet (WSL)").
    pub name: String,
    /// The driver description ("Intel(R) Wi-Fi 6 AX201", "TAP-Windows Adapter V9").
    pub description: String,
    pub kind: Kind,
    pub up: bool,
    pub ipv4: Vec<Ipv4Addr>,
    pub has_gateway: bool,
    /// Windows' interface metric: lower is preferred.
    pub metric: u32,
}

/// Name fragments of virtual adapters a phone on the Wi-Fi can't reach.
const VIRTUAL: &[&str] = &[
    "vethernet",
    "hyper-v",
    "wsl",
    "docker",
    "virtualbox",
    "vmware",
    "vpn",
    "tap-",
    "tap adapter",
    "tun",
    "wireguard",
    "wintun",
    "tailscale",
    "zerotier",
    "hamachi",
    "anyconnect",
    "fortinet",
    "forticlient",
    "globalprotect",
    "pangp",
    "nordlynx",
    "cloudflare warp",
    "loopback",
    "bluetooth",
    "virtual",
    "teredo",
    "isatap",
];

fn is_virtual(a: &Adapter) -> bool {
    let name = a.name.to_lowercase();
    let desc = a.description.to_lowercase();
    VIRTUAL.iter().any(|v| {
        // "tun" is too short to search for inside words ("Tuning"): only as a word, or "tun0".
        if *v == "tun" {
            let tun_word = |w: &str| {
                w.strip_prefix("tun")
                    .is_some_and(|n| n.chars().all(|c| c.is_ascii_digit()))
            };
            return name.split(|c: char| !c.is_alphanumeric()).any(tun_word)
                || desc.split(|c: char| !c.is_alphanumeric()).any(tun_word);
        }
        name.contains(v) || desc.contains(v)
    })
}

/// 10/8, 172.16/12, 192.168/16: what home routers hand out.
fn private(ip: Ipv4Addr) -> bool {
    ip.is_private()
}

/// 100.64/10 (carrier-grade NAT): on a PC that's nearly always Tailscale, not the Wi-Fi.
fn cgnat(ip: Ipv4Addr) -> bool {
    let [a, b, ..] = ip.octets();
    a == 100 && (64..128).contains(&b)
}

/// A score for using `ip` on `a`, or `None` if the phone can't reach it.
fn score(a: &Adapter, ip: Ipv4Addr) -> Option<i64> {
    if !a.up || matches!(a.kind, Kind::Loopback | Kind::Tunnel) || is_virtual(a) {
        return None;
    }
    if ip.is_loopback() || ip.is_link_local() || ip.is_unspecified() || ip.is_broadcast() || ip.is_multicast() {
        return None;
    }
    if cgnat(ip) {
        return None;
    }
    let mut s = 0i64;
    if a.has_gateway {
        s += 10_000;
    }
    if private(ip) {
        s += 1_000;
    }
    if matches!(a.kind, Kind::Wifi | Kind::Ethernet) {
        s += 500;
    }
    s -= a.metric.min(400) as i64;
    Some(s)
}

/// The best address to serve Drop on, if any adapter qualifies. Prefers the adapter with the
/// default gateway, then private (home) ranges, then real Wi-Fi/Ethernet, then the lower metric;
/// ties go to the first listed.
pub fn best(adapters: &[Adapter]) -> Option<Ipv4Addr> {
    let mut best: Option<(i64, Ipv4Addr)> = None;
    for a in adapters {
        for &ip in &a.ipv4 {
            if let Some(s) = score(a, ip) {
                if best.is_none_or(|(b, _)| s > b) {
                    best = Some((s, ip));
                }
            }
        }
    }
    best.map(|(_, ip)| ip)
}

/// The best address right now (see `best`).
pub fn current() -> Option<Ipv4Addr> {
    best(&adapters())
}

#[cfg(windows)]
fn adapters() -> Vec<Adapter> {
    use windows::Win32::Foundation::{ERROR_BUFFER_OVERFLOW, ERROR_SUCCESS};
    use windows::Win32::NetworkManagement::IpHelper::{
        GetAdaptersAddresses, GAA_FLAG_INCLUDE_GATEWAYS, GAA_FLAG_SKIP_ANYCAST, GAA_FLAG_SKIP_DNS_SERVER,
        GAA_FLAG_SKIP_MULTICAST, IF_TYPE_ETHERNET_CSMACD, IF_TYPE_IEEE80211, IF_TYPE_PPP, IF_TYPE_SOFTWARE_LOOPBACK,
        IF_TYPE_TUNNEL, IP_ADAPTER_ADDRESSES_LH,
    };
    use windows::Win32::NetworkManagement::Ndis::IfOperStatusUp;
    use windows::Win32::Networking::WinSock::{AF_INET, SOCKADDR_IN};

    let flags = GAA_FLAG_INCLUDE_GATEWAYS | GAA_FLAG_SKIP_ANYCAST | GAA_FLAG_SKIP_MULTICAST | GAA_FLAG_SKIP_DNS_SERVER;
    // The documented pattern: start with 15 KB and grow if Windows asks for more.
    let mut len: u32 = 15 * 1024;
    let mut buf: Vec<u64> = Vec::new();
    for _ in 0..4 {
        // u64 elements keep the buffer aligned for IP_ADAPTER_ADDRESSES_LH.
        buf = vec![0u64; (len as usize).div_ceil(8)];
        // SAFETY: `buf` is at least `len` bytes, suitably aligned, and outlives the call.
        let rc = unsafe {
            GetAdaptersAddresses(
                AF_INET.0 as u32,
                flags,
                None,
                Some(buf.as_mut_ptr() as *mut IP_ADAPTER_ADDRESSES_LH),
                &mut len,
            )
        };
        if rc == ERROR_SUCCESS.0 {
            break;
        }
        if rc != ERROR_BUFFER_OVERFLOW.0 {
            log::debug!("drop: GetAdaptersAddresses failed ({rc})");
            return Vec::new();
        }
        buf.clear();
    }
    if buf.is_empty() {
        return Vec::new();
    }

    let mut out = Vec::new();
    let mut cur = buf.as_ptr() as *const IP_ADAPTER_ADDRESSES_LH;
    // SAFETY: Windows filled `buf` with a linked list of adapters that lives inside it; every
    // pointer below points into that buffer, which stays alive until we return.
    unsafe {
        while let Some(a) = cur.as_ref() {
            let mut ipv4 = Vec::new();
            let mut ua = a.FirstUnicastAddress;
            while let Some(u) = ua.as_ref() {
                let sa = u.Address.lpSockaddr;
                if !sa.is_null() && (*sa).sa_family == AF_INET {
                    let sin = &*(sa as *const SOCKADDR_IN);
                    ipv4.push(Ipv4Addr::from(u32::from_be(sin.sin_addr.S_un.S_addr)));
                }
                ua = u.Next;
            }
            let kind = match a.IfType {
                IF_TYPE_IEEE80211 => Kind::Wifi,
                IF_TYPE_ETHERNET_CSMACD => Kind::Ethernet,
                IF_TYPE_SOFTWARE_LOOPBACK => Kind::Loopback,
                IF_TYPE_TUNNEL | IF_TYPE_PPP => Kind::Tunnel,
                _ => Kind::Other,
            };
            out.push(Adapter {
                name: a.FriendlyName.to_string().unwrap_or_default(),
                description: a.Description.to_string().unwrap_or_default(),
                kind,
                up: a.OperStatus == IfOperStatusUp,
                ipv4,
                has_gateway: !a.FirstGatewayAddress.is_null(),
                metric: a.Ipv4Metric,
            });
            cur = a.Next;
        }
    }
    out
}

#[cfg(not(windows))]
fn adapters() -> Vec<Adapter> {
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn adapter(name: &str, desc: &str, kind: Kind, ip: [u8; 4], gw: bool, metric: u32) -> Adapter {
        Adapter {
            name: name.into(),
            description: desc.into(),
            kind,
            up: true,
            ipv4: vec![Ipv4Addr::from(ip)],
            has_gateway: gw,
            metric,
        }
    }

    fn wifi() -> Adapter {
        adapter(
            "Wi-Fi",
            "Intel(R) Wi-Fi 6 AX201 160MHz",
            Kind::Wifi,
            [192, 168, 1, 20],
            true,
            35,
        )
    }

    #[test]
    fn picks_the_wifi_with_the_gateway_over_virtual_adapters() {
        let list = vec![
            adapter(
                "Loopback Pseudo-Interface 1",
                "Software Loopback Interface 1",
                Kind::Loopback,
                [127, 0, 0, 1],
                false,
                75,
            ),
            adapter(
                "vEthernet (WSL (Hyper-V firewall))",
                "Hyper-V Virtual Ethernet Adapter",
                Kind::Ethernet,
                [172, 28, 0, 1],
                false,
                15,
            ),
            adapter(
                "vEthernet (Default Switch)",
                "Hyper-V Virtual Ethernet Adapter #2",
                Kind::Ethernet,
                [172, 17, 80, 1],
                false,
                15,
            ),
            adapter("Ethernet 3", "Docker NAT", Kind::Ethernet, [10, 0, 75, 1], false, 5),
            adapter(
                "Tailscale",
                "Tailscale Tunnel",
                Kind::Other,
                [100, 101, 102, 103],
                false,
                5,
            ),
            adapter(
                "Ethernet 2",
                "TAP-Windows Adapter V9",
                Kind::Ethernet,
                [10, 8, 0, 6],
                true,
                1,
            ),
            adapter(
                "Local Area Connection* 10",
                "Microsoft Wi-Fi Direct Virtual Adapter #2",
                Kind::Wifi,
                [192, 168, 137, 1],
                false,
                25,
            ),
            wifi(),
        ];
        assert_eq!(best(&list), Some(Ipv4Addr::new(192, 168, 1, 20)));
    }

    #[test]
    fn skips_link_local_down_and_tunnels() {
        let mut down = wifi();
        down.up = false;
        let apipa = adapter(
            "Ethernet",
            "Realtek PCIe GbE",
            Kind::Ethernet,
            [169, 254, 3, 4],
            false,
            25,
        );
        let vpn = adapter("Corp", "Some VPN client", Kind::Tunnel, [10, 1, 2, 3], true, 1);
        assert_eq!(best(&[down, apipa, vpn]), None);
        assert_eq!(best(&[]), None);
    }

    #[test]
    fn prefers_the_gateway_then_lower_metric() {
        let eth_no_gw = adapter("Ethernet", "Realtek PCIe GbE", Kind::Ethernet, [10, 0, 0, 5], false, 5);
        assert_eq!(best(&[eth_no_gw.clone(), wifi()]), Some(Ipv4Addr::new(192, 168, 1, 20)));
        // Both on the home network with gateways: the cable (lower metric) wins.
        let eth = adapter(
            "Ethernet",
            "Realtek PCIe GbE",
            Kind::Ethernet,
            [192, 168, 1, 21],
            true,
            25,
        );
        assert_eq!(best(&[wifi(), eth]), Some(Ipv4Addr::new(192, 168, 1, 21)));
        // With no gateway anywhere (a direct link), a private address still works.
        assert_eq!(best(&[eth_no_gw]), Some(Ipv4Addr::new(10, 0, 0, 5)));
    }

    #[test]
    fn tun_matches_only_as_a_word() {
        let realtek = adapter(
            "Ethernet",
            "Realtek Gaming Tuning Adapter",
            Kind::Ethernet,
            [192, 168, 0, 9],
            true,
            25,
        );
        assert_eq!(best(&[realtek]), Some(Ipv4Addr::new(192, 168, 0, 9)));
        let tun = adapter("tun0", "OpenConnect", Kind::Other, [192, 168, 50, 2], true, 1);
        assert_eq!(best(&[tun]), None);
    }
}
