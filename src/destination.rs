//! No network I/O here. WP-03 must resolve once under its deadline/admission,
//! pass ALL answers, and connect only to this capability's numeric addresses.
use crate::error::{Error, Result};
use ipnet::IpNet;
use serde::Deserialize;
use std::net::{IpAddr, SocketAddr};
use url::{Host, Url};

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Rule {
    pub cidr: String,
    pub host: Option<String>,
    #[serde(default)]
    pub ports: Vec<u16>,
    #[serde(default)]
    pub all_tcp_ports: bool,
    #[serde(default)]
    pub ping: bool,
}
#[derive(Clone)]
struct Grant {
    network: IpNet,
    host: Option<String>,
    ports: Vec<u16>,
    all_tcp_ports: bool,
    ping: bool,
}
#[derive(Clone)]
pub struct Policy {
    grants: Vec<Grant>,
}
pub struct Target {
    host: String,
    port: Option<u16>,
    url: Option<Url>,
}
pub struct ValidatedDestination {
    host: String,
    addresses: Vec<IpAddr>,
    port: Option<u16>,
}
impl Target {
    pub fn http(text: &str) -> Result<Self> {
        valid_text(text, 2048)?;
        let url = Url::parse(text).map_err(|_| Error::InvalidInput)?;
        if !matches!(url.scheme(), "http" | "https")
            || !url.username().is_empty()
            || url.password().is_some()
            || url.fragment().is_some()
        {
            return Err(Error::InvalidInput);
        }
        // Reject URL parser repair/ambiguous numeric forms and encoded authority.
        let authority = text
            .split_once("://")
            .ok_or(Error::InvalidInput)?
            .1
            .split(['/', '?', '#'])
            .next()
            .ok_or(Error::InvalidInput)?;
        if authority.contains(['@', '%', '\\']) || text.contains('\\') {
            return Err(Error::InvalidInput);
        }
        let host = match url.host().ok_or(Error::InvalidInput)? {
            Host::Domain(h) => normalize_host(h)?,
            Host::Ipv4(ip) => ip.to_string(),
            Host::Ipv6(ip) => ip.to_string(),
        };
        let port = url
            .port_or_known_default()
            .filter(|p| *p != 0)
            .ok_or(Error::InvalidInput)?;
        let original_host = if authority.starts_with('[') {
            authority
                .split(']')
                .next()
                .ok_or(Error::InvalidInput)?
                .trim_start_matches('[')
        } else {
            authority.split(':').next().ok_or(Error::InvalidInput)?
        };
        if let Ok(ip) = host.parse::<IpAddr>()
            && original_host.parse::<IpAddr>().ok() != Some(ip)
        {
            return Err(Error::InvalidInput);
        }
        Ok(Self {
            host,
            port: Some(port),
            url: Some(url),
        })
    }
    pub fn tcp(host: &str, port: u16) -> Result<Self> {
        if port == 0 {
            return Err(Error::InvalidInput);
        }
        Ok(Self {
            host: normalize_host(host)?,
            port: Some(port),
            url: None,
        })
    }
    pub fn ping(host: &str) -> Result<Self> {
        Ok(Self {
            host: normalize_host(host)?,
            port: None,
            url: None,
        })
    }
    pub fn host(&self) -> &str {
        &self.host
    }
    pub fn port(&self) -> Option<u16> {
        self.port
    }
    // Sensitive URL access is explicit, with no Debug/Display or log formatting.
    pub fn url(&self) -> Option<&Url> {
        self.url.as_ref()
    }
}
fn valid_text(text: &str, max: usize) -> Result<()> {
    if text.is_empty()
        || text.len() > max
        || text.chars().any(|c| c.is_control() || c.is_whitespace())
    {
        Err(Error::InvalidInput)
    } else {
        Ok(())
    }
}
fn normalize_host(text: &str) -> Result<String> {
    valid_text(text, 253)?;
    if let Ok(ip) = text.parse::<IpAddr>() {
        return Ok(ip.to_string());
    }
    if matches!(Host::parse(text), Ok(Host::Ipv4(_) | Host::Ipv6(_))) {
        return Err(Error::InvalidInput);
    }
    let host = text.to_ascii_lowercase();
    if !host.is_ascii()
        || host.ends_with('.')
        || host.split('.').any(|s| {
            s.is_empty()
                || s.len() > 63
                || s.starts_with('-')
                || s.ends_with('-')
                || !s.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-')
        })
        || host.bytes().all(|b| b.is_ascii_digit() || b == b'.')
    {
        return Err(Error::InvalidInput);
    }
    Ok(host)
}
pub fn normalize(ip: IpAddr) -> IpAddr {
    match ip {
        IpAddr::V6(ip) => ip
            .to_ipv4_mapped()
            .map(IpAddr::V4)
            .unwrap_or(IpAddr::V6(ip)),
        _ => ip,
    }
}
enum Class {
    Public,
    Private,
    Loopback,
    Forbidden,
}
fn class(ip: IpAddr) -> Class {
    match normalize(ip) {
        IpAddr::V4(ip) => {
            let [a, b, c, _] = ip.octets();
            if a == 0
                || a >= 224
                || ip == std::net::Ipv4Addr::new(100, 100, 100, 200)
                || ip == std::net::Ipv4Addr::new(168, 63, 129, 16)
                || (a == 169 && b == 254)
                || (a == 192 && b == 0 && (c == 0 || c == 2))
                || (a == 198 && (b == 18 || b == 19 || (b == 51 && c == 100)))
                || (a == 203 && b == 0 && c == 113)
            {
                Class::Forbidden
            } else if a == 127 {
                Class::Loopback
            } else if a == 10
                || (a == 172 && (16..=31).contains(&b))
                || (a == 192 && b == 168)
                || (a == 100 && (64..=127).contains(&b))
            {
                Class::Private
            } else {
                Class::Public
            }
        }
        IpAddr::V6(ip) => {
            let segments = ip.segments();
            // AWS IMDS is ULA, but metadata is unconditionally forbidden;
            // classify it before the grantable private-address branch.
            if ip == std::net::Ipv6Addr::new(0xfd00, 0x0ec2, 0, 0, 0, 0, 0, 0x0254) {
                Class::Forbidden
            } else if ip.is_loopback() {
                Class::Loopback
            } else if segments[0] & 0xfe00 == 0xfc00 {
                Class::Private
            }
            // Only global unicast 2000::/3; reject documentation, transition,
            // NAT64, site/link-local and other special-use/embedded addresses.
            else if segments[0] & 0xe000 != 0x2000
                || segments[0] == 0x2002
                || (segments[0] == 0x2001 && (segments[1] < 0x0200 || segments[1] == 0x0db8))
                || (segments[0] == 0x3fff && segments[1] < 0x1000)
            {
                Class::Forbidden
            } else {
                Class::Public
            }
        }
    }
}
impl Policy {
    pub fn new(rules: Vec<Rule>) -> Result<Self> {
        if rules.len() > 64 {
            return Err(Error::Configuration);
        }
        let mut grants = Vec::with_capacity(rules.len());
        for rule in rules {
            let network: IpNet = rule.cidr.parse().map_err(|_| Error::Configuration)?;
            // Mapped CIDRs are refused rather than inconsistently normalized.
            if matches!(network,IpNet::V6(n) if n.addr().to_ipv4_mapped().is_some())
                || rule.ports.len() > 64
                || rule.ports.contains(&0)
                || (rule.all_tcp_ports && !rule.ports.is_empty())
                || (rule.ports.is_empty() && !rule.all_tcp_ports && !rule.ping)
            {
                return Err(Error::Configuration);
            }
            if matches!(class(network.addr()), Class::Loopback)
                && network.prefix_len() != if network.addr().is_ipv4() { 32 } else { 128 }
            {
                return Err(Error::Configuration);
            }
            let host = rule
                .host
                .map(|h| normalize_host(&h))
                .transpose()
                .map_err(|_| Error::Configuration)?;
            grants.push(Grant {
                network,
                host,
                ports: rule.ports,
                all_tcp_ports: rule.all_tcp_ports,
                ping: rule.ping,
            });
        }
        Ok(Self { grants })
    }
    pub fn authorize(&self, target: &Target, answers: &[IpAddr]) -> Result<ValidatedDestination> {
        if answers.is_empty() || answers.len() > 32 {
            return Err(Error::Forbidden);
        }
        let mut addresses = Vec::with_capacity(answers.len());
        for answer in answers {
            let ip = normalize(*answer);
            let allowed = match class(ip) {
                Class::Forbidden => false,
                Class::Public => true,
                Class::Private | Class::Loopback => self.grants.iter().any(|r| {
                    r.network.contains(&ip)
                        && !matches!((r.network,ip),(IpNet::V4(n),IpAddr::V4(v)) if n.prefix_len()<31 && (v==n.network() || v==n.broadcast()))
                        && (!matches!(class(ip), Class::Loopback)
                            || r.network.prefix_len() == if ip.is_ipv4() { 32 } else { 128 })
                        && r.host.as_ref().is_none_or(|host| host == target.host())
                        && target.port.map(|p| r.all_tcp_ports || r.ports.contains(&p)).unwrap_or(r.ping)
                }),
            };
            if !allowed {
                return Err(Error::Forbidden);
            }
            if !addresses.contains(&ip) {
                addresses.push(ip);
            }
        }
        // Literal targets cannot be paired with unrelated resolver answers.
        if let Ok(literal) = target.host.parse::<IpAddr>()
            && addresses.iter().any(|ip| *ip != normalize(literal))
        {
            return Err(Error::Forbidden);
        }
        Ok(ValidatedDestination {
            host: target.host.clone(),
            addresses,
            port: target.port,
        })
    }
}
impl ValidatedDestination {
    pub fn tls_hostname(&self) -> &str {
        &self.host
    }
    pub fn addresses(&self) -> &[IpAddr] {
        &self.addresses
    }
    pub fn sockets(&self) -> Result<Vec<SocketAddr>> {
        let port = self.port.ok_or(Error::InvalidInput)?;
        Ok(self
            .addresses
            .iter()
            .map(|ip| SocketAddr::new(*ip, port))
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn ips(text: &[&str]) -> Vec<IpAddr> {
        text.iter().map(|s| s.parse().unwrap()).collect()
    }
    #[test]
    fn all_tcp_grants_are_compact_independent_and_preserve_hard_denials() {
        let make = |cidr: &str, ping| Rule {
            cidr: cidr.into(),
            host: None,
            ports: vec![],
            all_tcp_ports: true,
            ping,
        };
        let policy = Policy::new(vec![
            make("192.168.1.0/24", true),
            make("10.172.73.1/32", true),
        ])
        .unwrap();
        for host in ["192.168.1.1", "192.168.1.254", "10.172.73.1"] {
            for port in [1, 65535] {
                assert!(
                    policy
                        .authorize(&Target::tcp(host, port).unwrap(), &ips(&[host]))
                        .is_ok()
                );
            }
            assert!(
                policy
                    .authorize(&Target::ping(host).unwrap(), &ips(&[host]))
                    .is_ok()
            );
        }
        for host in ["192.168.1.0", "192.168.1.255", "192.168.2.1", "10.172.73.2"] {
            assert!(
                policy
                    .authorize(&Target::tcp(host, 1).unwrap(), &ips(&[host]))
                    .is_err()
            );
        }
        let no_ping = Policy::new(vec![make("192.168.1.0/24", false)]).unwrap();
        assert!(
            no_ping
                .authorize(
                    &Target::ping("192.168.1.1").unwrap(),
                    &ips(&["192.168.1.1"])
                )
                .is_err()
        );
        let mut conflict = make("10.0.0.0/8", false);
        conflict.ports.push(443);
        assert!(Policy::new(vec![conflict]).is_err());
        assert!(Target::tcp("192.168.1.1", 0).is_err());
        for cidr in ["0.0.0.0/0", "::/0", "fd00:ec2::254/128"] {
            let broad = Policy::new(vec![make(cidr, true)]).unwrap();
            for forbidden in [
                "169.254.169.254",
                "fd00:ec2::254",
                "100.100.100.200",
                "168.63.129.16",
                "::ffff:169.254.169.254",
            ] {
                assert!(
                    broad
                        .authorize(&Target::ping(forbidden).unwrap(), &ips(&[forbidden]))
                        .is_err()
                );
            }
        }
    }
    #[test]
    fn aws_ipv6_metadata_is_denied_before_any_grant() {
        let metadata = "fd00:ec2::254".parse::<IpAddr>().unwrap();
        let ordinary = "fd00:ec2::253".parse::<IpAddr>().unwrap();
        let public = "2606:4700:4700::1111".parse::<IpAddr>().unwrap();
        for cidr in ["fd00::/8", "fc00::/7", "::/0", "fd00:ec2::254/128"] {
            let policy = Policy::new(vec![Rule {
                cidr: cidr.into(),
                host: None,
                ports: vec![80, 443],
                all_tcp_ports: false,
                ping: true,
            }])
            .unwrap();
            for target in [
                Target::http("http://[fd00:ec2::254]/latest/meta-data/").unwrap(),
                Target::http("https://[FD00:0EC2:0:0:0:0:0:0254]/").unwrap(),
                Target::tcp("fd00:ec2::254", 80).unwrap(),
                Target::ping("fd00:ec2::254").unwrap(),
                Target::http("http://metadata.invalid/latest/meta-data/").unwrap(),
                Target::tcp("metadata.invalid", 80).unwrap(),
                Target::ping("metadata.invalid").unwrap(),
            ] {
                assert!(
                    matches!(
                        policy.authorize(&target, &[metadata]),
                        Err(Error::Forbidden)
                    ),
                    "grant {cidr}"
                );
            }
            let hostname = Target::http("http://metadata.invalid/").unwrap();
            for answers in [
                [public, metadata],
                [metadata, public],
                [ordinary, metadata],
                [metadata, ordinary],
            ] {
                assert!(
                    matches!(policy.authorize(&hostname, &answers), Err(Error::Forbidden)),
                    "mixed set under {cidr}"
                );
            }
            // The endpoint exception must preserve intentionally grantable ULA.
            if cidr != "fd00:ec2::254/128" {
                assert!(policy.authorize(&hostname, &[ordinary]).is_ok());
            }
        }
        let host_grant = Policy::new(vec![Rule {
            cidr: "fd00::/8".into(),
            host: Some("metadata.invalid".into()),
            ports: vec![80],
            all_tcp_ports: false,
            ping: true,
        }])
        .unwrap();
        assert!(matches!(
            host_grant.authorize(&Target::tcp("metadata.invalid", 80).unwrap(), &[metadata]),
            Err(Error::Forbidden)
        ));
    }
    #[test]
    fn rejects_special_mixed_rebound_and_mapped_addresses() {
        let p = Policy::new(vec![]).unwrap();
        let t = Target::tcp("example.invalid", 443).unwrap();
        for bad in [
            "0.0.0.0",
            "10.0.0.1",
            "127.0.0.1",
            "169.254.169.254",
            "100.100.100.200",
            "168.63.129.16",
            "224.0.0.1",
            "255.255.255.255",
            "::",
            "::1",
            "fe80::1",
            "ff02::1",
            "::ffff:127.0.0.1",
            "64:ff9b::a00:1",
            "2002:0a00:0001::1",
            "2001:db8::1",
        ] {
            assert!(p.authorize(&t, &ips(&[bad])).is_err(), "{bad}");
            assert!(p.authorize(&t, &ips(&["8.8.8.8", bad])).is_err());
        }
        let capability = p
            .authorize(&t, &ips(&["8.8.8.8", "2606:4700:4700::1111"]))
            .unwrap();
        assert_eq!(capability.tls_hostname(), "example.invalid");
        assert_eq!(capability.sockets().unwrap().len(), 2);
        assert!(p.authorize(&t, &ips(&["10.0.0.1"])).is_err()); // changed second resolution fails
        assert!(p.authorize(&t, &[]).is_err());
        assert!(
            p.authorize(&t, &vec!["8.8.8.8".parse().unwrap(); 33])
                .is_err()
        );
    }
    #[test]
    fn private_grants_bind_host_port_ping_and_narrow_loopback() {
        let p = Policy::new(vec![
            Rule {
                cidr: "10.1.0.0/24".into(),
                host: Some("home.invalid".into()),
                ports: vec![443],
                all_tcp_ports: false,
                ping: false,
            },
            Rule {
                cidr: "127.0.0.1/32".into(),
                host: None,
                ports: vec![8080],
                all_tcp_ports: false,
                ping: true,
            },
        ])
        .unwrap();
        assert!(
            p.authorize(
                &Target::tcp("home.invalid", 443).unwrap(),
                &ips(&["::ffff:10.1.0.2"])
            )
            .is_ok()
        );
        assert!(
            p.authorize(
                &Target::tcp("other.invalid", 443).unwrap(),
                &ips(&["10.1.0.2"])
            )
            .is_err()
        );
        assert!(
            p.authorize(
                &Target::tcp("home.invalid", 80).unwrap(),
                &ips(&["10.1.0.2"])
            )
            .is_err()
        );
        assert!(
            p.authorize(&Target::ping("home.invalid").unwrap(), &ips(&["10.1.0.2"]))
                .is_err()
        );
        assert!(
            p.authorize(&Target::ping("127.0.0.1").unwrap(), &ips(&["127.0.0.1"]))
                .is_ok()
        );
        assert!(
            p.authorize(&Target::tcp("8.8.8.8", 443).unwrap(), &ips(&["1.1.1.1"]))
                .is_err()
        );
        assert!(
            Policy::new(vec![Rule {
                cidr: "127.0.0.0/8".into(),
                host: None,
                ports: vec![80],
                all_tcp_ports: false,
                ping: false
            }])
            .is_err()
        );
        let all = Policy::new(vec![Rule {
            cidr: "0.0.0.0/0".into(),
            host: None,
            ports: vec![80],
            all_tcp_ports: false,
            ping: true,
        }])
        .unwrap();
        assert!(
            all.authorize(
                &Target::ping("169.254.169.254").unwrap(),
                &ips(&["169.254.169.254"])
            )
            .is_err()
        );
        let ipv6 = Policy::new(vec![
            Rule {
                cidr: "fd12::/64".into(),
                host: None,
                ports: vec![443],
                all_tcp_ports: false,
                ping: true,
            },
            Rule {
                cidr: "::1/128".into(),
                host: None,
                ports: vec![8080],
                all_tcp_ports: false,
                ping: true,
            },
        ])
        .unwrap();
        assert!(
            ipv6.authorize(
                &Target::tcp("home.invalid", 443).unwrap(),
                &ips(&["fd12::2"])
            )
            .is_ok()
        );
        assert!(
            ipv6.authorize(&Target::ping("::1").unwrap(), &ips(&["::1"]))
                .is_ok()
        );
        assert!(
            p.authorize(
                &Target::tcp("home.invalid", 443).unwrap(),
                &ips(&["10.1.0.255"])
            )
            .is_err()
        );
        assert!(
            Policy::new(vec![Rule {
                cidr: "::ffff:10.0.0.0/120".into(),
                host: None,
                ports: vec![80],
                all_tcp_ports: false,
                ping: false
            }])
            .is_err()
        );
    }
    #[test]
    fn targets_reject_credentials_injection_and_ambiguous_authorities() {
        for url in [
            "http://user:secret@example.invalid/",
            "http://example.invalid/#x",
            "ftp://example.invalid/",
            "http://127.1/",
            "http://2130706433/",
            "http://0x7f000001/",
            "http://example.invalid\\@evil.invalid/",
            "http://example.invalid:0/",
            "http://example.invalid/\n",
        ] {
            assert!(Target::http(url).is_err(), "{url}");
        }
        assert!(Target::http("https://example.invalid/path?private=value").is_ok());
        assert!(Target::http("http://[::1]:8080/").is_ok());
        for host in [
            "localhost:80",
            "user@host",
            "-host",
            "host.",
            "127.1",
            "host/path",
            "a\n",
        ] {
            assert!(Target::tcp(host, 80).is_err());
        }
    }
}
