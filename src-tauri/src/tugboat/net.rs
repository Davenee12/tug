//! Which address the phone should use to reach this PC: the IPv4 address of the adapter that has
//! the default gateway (the home Wi-Fi or Ethernet), never loopback, VPNs, WSL or the Hyper-V
//! Default Switch, Docker or link-local. A Hyper-V *external* switch that carries the home network
//! (and its gateway) is used, below any physical adapter. Tugboat listens only on that address.
//! Known limit: Windows Mobile Hotspot (the PC as the hotspot, a "Wi-Fi Direct Virtual Adapter"
//! with no gateway) isn't offered.

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

/// What gives a Hyper-V virtual switch adapter away ("vEthernet (…)", "Hyper-V Virtual Ethernet
/// Adapter"). On their own these say nothing about whether a phone can reach it.
const HYPERV: &[&str] = &["vethernet", "hyper-v", "virtual"];

/// Hyper-V switches that are never the home network: the Default Switch and WSL's are NAT
/// networks private to this PC (and normally have no gateway anyway).
const HYPERV_INTERNAL: &[&str] = &["default switch", "wsl"];

/// A Hyper-V *external* switch: Windows moves the PC's own Wi-Fi or Ethernet address onto a
/// `vEthernet (…)` adapter bound to the real one, so on such a PC the home network's address and
/// gateway are only on the virtual adapter. It counts when it holds the default gateway, isn't the
/// Default Switch or WSL's, and nothing else marks it as a VPN or another virtual network.
fn is_hyperv_external(a: &Adapter) -> bool {
    let name = a.name.to_lowercase();
    let desc = a.description.to_lowercase();
    let hyperv = name.starts_with("vethernet") || desc.contains("hyper-v virtual ethernet");
    hyperv
        && a.has_gateway
        && !HYPERV_INTERNAL.iter().any(|v| name.contains(v) || desc.contains(v))
        && !matches_virtual(a, |v| !HYPERV.contains(&v))
}

fn is_virtual(a: &Adapter) -> bool {
    matches_virtual(a, |_| true)
}

/// Whether any `VIRTUAL` fragment that `consider` accepts appears in the adapter's name or
/// description.
fn matches_virtual(a: &Adapter, consider: impl Fn(&str) -> bool) -> bool {
    let name = a.name.to_lowercase();
    let desc = a.description.to_lowercase();
    VIRTUAL.iter().filter(|v| consider(v)).any(|v| {
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

/// A score for using `ip` on `a`, or `None` if the phone can't (or mustn't) reach it.
fn score(a: &Adapter, ip: Ipv4Addr) -> Option<i64> {
    let hyperv_external = is_hyperv_external(a);
    if !a.up || matches!(a.kind, Kind::Loopback | Kind::Tunnel) || (is_virtual(a) && !hyperv_external) {
        return None;
    }
    if ip.is_loopback() || ip.is_link_local() || ip.is_unspecified() || ip.is_broadcast() || ip.is_multicast() {
        return None;
    }
    // Only home-network (private) addresses: Tugboat must never be reachable from the internet,
    // e.g. on a network that hands out public addresses. 100.64/10 is nearly always Tailscale.
    if cgnat(ip) || !private(ip) {
        return None;
    }
    let mut s = 0i64;
    if a.has_gateway {
        s += 10_000;
    }
    if hyperv_external {
        // Below any physical adapter on the home network: it's chosen only when the real
        // network is reached through it (above adapters with no gateway at all, though: the
        // gateway is what says "this is the network the phone is on").
        s -= HYPERV_PENALTY;
    } else if matches!(a.kind, Kind::Wifi | Kind::Ethernet) {
        s += 500;
    }
    s -= a.metric.min(400) as i64;
    Some(s)
}

/// How far a Hyper-V external switch ranks below a physical adapter that has the gateway.
const HYPERV_PENALTY: i64 = 5_000;

/// The best address to serve Tugboat on, if any adapter qualifies. Only private (home) ranges;
/// prefers the adapter with the default gateway, then real Wi-Fi/Ethernet, then the lower
/// metric; ties go to the first listed.
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
            log::debug!("tugboat: GetAdaptersAddresses failed ({rc})");
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
    fn never_uses_a_public_address() {
        let public = adapter("Ethernet", "Intel(R) Ethernet", Kind::Ethernet, [128, 3, 4, 5], true, 5);
        assert_eq!(best(std::slice::from_ref(&public)), None);
        assert_eq!(best(&[public, wifi()]), Some(Ipv4Addr::new(192, 168, 1, 20)));
    }

    /// A Hyper-V external switch bound to the PC's Wi-Fi: the home address and gateway sit on the
    /// vEthernet adapter; the Wi-Fi adapter itself has no IPv4 address.
    fn external_switch() -> Adapter {
        adapter(
            "vEthernet (External Switch)",
            "Hyper-V Virtual Ethernet Adapter #3",
            Kind::Ethernet,
            [192, 168, 1, 20],
            true,
            25,
        )
    }

    #[test]
    fn uses_a_hyperv_external_switch_that_carries_the_home_network() {
        let mut bare_wifi = wifi();
        bare_wifi.ipv4.clear();
        let default_switch = adapter(
            "vEthernet (Default Switch)",
            "Hyper-V Virtual Ethernet Adapter",
            Kind::Ethernet,
            [172, 17, 80, 1],
            false,
            15,
        );
        let wsl = adapter(
            "vEthernet (WSL (Hyper-V firewall))",
            "Hyper-V Virtual Ethernet Adapter #2",
            Kind::Ethernet,
            [172, 28, 0, 1],
            false,
            15,
        );
        let list = vec![default_switch, wsl, bare_wifi, external_switch()];
        assert_eq!(best(&list), Some(Ipv4Addr::new(192, 168, 1, 20)));
    }

    #[test]
    fn a_physical_adapter_on_the_home_network_beats_the_external_switch() {
        let eth = adapter(
            "Ethernet",
            "Realtek PCIe GbE",
            Kind::Ethernet,
            [192, 168, 1, 30],
            true,
            400,
        );
        assert_eq!(best(&[external_switch(), eth]), Some(Ipv4Addr::new(192, 168, 1, 30)));
        // But it beats an adapter with no gateway (a direct cable to another device).
        let direct = adapter("Ethernet 2", "Realtek USB GbE", Kind::Ethernet, [10, 0, 0, 5], false, 5);
        assert_eq!(best(&[direct, external_switch()]), Some(Ipv4Addr::new(192, 168, 1, 20)));
    }

    #[test]
    fn still_skips_internal_switches_vpns_and_public_or_cgnat_addresses() {
        // The Default Switch or WSL's, even if one had a gateway.
        let mut default_switch = external_switch();
        default_switch.name = "vEthernet (Default Switch)".into();
        let mut wsl = external_switch();
        wsl.name = "vEthernet (WSL)".into();
        // A VPN that happens to sit on a Hyper-V switch.
        let mut vpn = external_switch();
        vpn.name = "vEthernet (Corp VPN)".into();
        // No gateway: an internal or private switch, not the home network.
        let mut internal = external_switch();
        internal.has_gateway = false;
        // Tailscale-style and public addresses never qualify.
        let mut cgnat = external_switch();
        cgnat.ipv4 = vec![Ipv4Addr::new(100, 101, 102, 103)];
        let mut public = external_switch();
        public.ipv4 = vec![Ipv4Addr::new(128, 3, 4, 5)];
        for a in [default_switch, wsl, vpn, internal, cgnat, public] {
            assert_eq!(best(std::slice::from_ref(&a)), None, "{}", a.name);
        }
        // TAP adapters stay out whatever they're called.
        let tap = adapter(
            "vEthernet (x)",
            "TAP-Windows Adapter V9",
            Kind::Ethernet,
            [192, 168, 1, 40],
            true,
            1,
        );
        assert_eq!(best(&[tap]), None);
    }

    #[test]
    fn mobile_hotspot_is_still_not_offered() {
        // Known limit: the PC's own hotspot has no gateway and is a virtual adapter.
        let hotspot = adapter(
            "Local Area Connection* 10",
            "Microsoft Wi-Fi Direct Virtual Adapter #2",
            Kind::Wifi,
            [192, 168, 137, 1],
            false,
            25,
        );
        assert_eq!(best(&[hotspot]), None);
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
