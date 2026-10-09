//! DNS-SD (mDNS/Bonjour) printer records → [`DiscoveredPrinter`].
//!
//! The browse itself is platform glue — CUPS has `ippfind`, the Windows backend
//! runs its own mDNS browser — but turning a resolved `_ipp._tcp` /
//! `_ipps._tcp` service into the row the UI shows, and deciding whether it is
//! already installed, is plain data work. It lives here so it is tested on every
//! host, not only on the Windows machine that runs it.
//!
//! The record shapes follow the IPP Everywhere / AirPrint TXT conventions
//! (PWG 5100.14 §4): `rp` is the resource path (`ipp/print`), `ty` the
//! make-and-model, `product` the parenthesized product name.

use std::net::IpAddr;

use crate::ipc::printers::DiscoveredPrinter;
use crate::platform::infer_kind;

/// The two service types a network printer advertises IPP on.
pub const IPP_SERVICE: &str = "_ipp._tcp.local.";
pub const IPPS_SERVICE: &str = "_ipps._tcp.local.";

/// One resolved service instance, decoupled from any mDNS library's types.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolved {
    /// Full instance name, e.g. `EPSON L15160 Series._ipp._tcp.local.`.
    pub fullname: String,
    /// The service type it was found under ([`IPP_SERVICE`] or [`IPPS_SERVICE`]).
    pub service_type: String,
    /// SRV target host, e.g. `EPSON1A2B3C.local.`.
    pub host: String,
    pub port: u16,
    pub addresses: Vec<IpAddr>,
    /// TXT `rp` (resource path), without a leading slash.
    pub rp: Option<String>,
    /// TXT `ty` (make and model).
    pub ty: Option<String>,
}

/// The URI scheme for a service type: `ipps` for `_ipps._tcp`, else `ipp`.
pub fn scheme_for(service_type: &str) -> &'static str {
    if service_type.starts_with("_ipps.") {
        "ipps"
    } else {
        "ipp"
    }
}

/// The human instance label: the full name minus its `.<service type>` tail.
/// DNS-SD instance names may contain dots and spaces, so only the known tail is
/// cut, never "everything after the first dot".
pub fn instance_label(fullname: &str, service_type: &str) -> String {
    let tail = format!(".{service_type}");
    fullname
        .strip_suffix(&tail)
        .unwrap_or(fullname)
        .replace("\\032", " ")
        .replace("\\.", ".")
        .trim()
        .to_string()
}

/// The host to put in the URI. The mDNS host name is preferred over an
/// address because DHCP moves addresses but not names, and Windows 10+/macOS
/// resolve `.local` names themselves. Falls back to the first IPv4 address,
/// then any address (IPv6 bracketed), when the record has no usable name.
pub fn uri_host(r: &Resolved) -> Option<String> {
    let name = r.host.trim().trim_end_matches('.');
    if !name.is_empty() {
        return Some(name.to_string());
    }
    let v4 = r.addresses.iter().find(|a| a.is_ipv4());
    match v4.or_else(|| r.addresses.first()) {
        Some(IpAddr::V4(a)) => Some(a.to_string()),
        Some(IpAddr::V6(a)) => Some(format!("[{a}]")),
        None => None,
    }
}

/// `ipp[s]://host:port/rp` for a resolved record, or `None` when it names no
/// reachable host or port.
pub fn uri_for(r: &Resolved) -> Option<String> {
    let host = uri_host(r)?;
    if r.port == 0 {
        return None;
    }
    let path =
        r.rp.as_deref()
            .map(|p| p.trim().trim_start_matches('/'))
            .filter(|p| !p.is_empty())
            .unwrap_or("ipp/print");
    Some(format!(
        "{}://{host}:{}/{path}",
        scheme_for(&r.service_type),
        r.port
    ))
}

/// Whether `r` is a device that is already installed.
///
/// Matched loosely, as the CUPS backend does: an installed queue whose name is
/// the instance label (Windows names a queue after the advertised name), or
/// whose port / device URI mentions the record's host name or one of its
/// addresses. Comparison is case-insensitive and ignores the trailing dot.
pub fn is_installed(r: &Resolved, installed: &[(String, String)]) -> bool {
    let label = instance_label(&r.fullname, &r.service_type).to_lowercase();
    let host = r.host.trim().trim_end_matches('.').to_lowercase();
    let host_short = host.strip_suffix(".local").unwrap_or(&host).to_string();
    let addrs: Vec<String> = r.addresses.iter().map(|a| a.to_string()).collect();
    installed.iter().any(|(name, port)| {
        let name = name.to_lowercase();
        let port = port.to_lowercase();
        (!label.is_empty() && name == label)
            || (!host_short.is_empty() && port.contains(&host_short))
            || addrs.iter().any(|a| contains_host(&port, a))
    })
}

/// `haystack` mentions `addr` as a whole host, not as part of a longer
/// address (`10.0.0.5` must match neither `10.0.0.50` nor `110.0.0.5`).
fn contains_host(haystack: &str, addr: &str) -> bool {
    let part_of_address = |c: char| c.is_ascii_digit() || c == '.';
    let mut from = 0;
    while let Some(i) = haystack[from..].find(addr) {
        let start = from + i;
        let end = start + addr.len();
        let prev = haystack[..start].chars().next_back();
        let next = haystack[end..].chars().next();
        if !prev.is_some_and(part_of_address) && !next.is_some_and(part_of_address) {
            return true;
        }
        from = end;
    }
    false
}

/// Turn a set of resolved records into UI rows: skip the installed ones and
/// anything without a usable URI, de-duplicate by URI, and keep the order the
/// browser found them in.
pub fn to_discovered(
    records: &[Resolved],
    installed: &[(String, String)],
) -> Vec<DiscoveredPrinter> {
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for r in records {
        if is_installed(r, installed) {
            continue;
        }
        let Some(uri) = uri_for(r) else { continue };
        if !seen.insert(uri.clone()) {
            continue;
        }
        let label = instance_label(&r.fullname, &r.service_type);
        let name = if label.is_empty() {
            uri_host(r).unwrap_or_else(|| uri.clone())
        } else {
            label
        };
        let model = r.ty.clone().unwrap_or_default();
        let scheme = scheme_for(&r.service_type).to_uppercase();
        out.push(DiscoveredPrinter {
            id: uri.clone(),
            kind: infer_kind(&model, &name, Some(&uri)),
            connection: format!("{} ({scheme})", uri_host(r).unwrap_or_default()),
            name,
            via: format!("mDNS · {scheme}"),
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;

    fn rec(service_type: &str) -> Resolved {
        Resolved {
            fullname: format!("EPSON L15160 Series.{service_type}"),
            service_type: service_type.to_string(),
            host: "EPSON1A2B3C.local.".into(),
            port: 631,
            addresses: vec![IpAddr::V4(Ipv4Addr::new(10, 0, 0, 5))],
            rp: Some("ipp/print".into()),
            ty: Some("EPSON L15160 Series".into()),
        }
    }

    #[test]
    fn fr_printers_discovery_builds_ipp_and_ipps_uris() {
        assert_eq!(
            uri_for(&rec(IPP_SERVICE)).as_deref(),
            Some("ipp://EPSON1A2B3C.local:631/ipp/print")
        );
        assert_eq!(
            uri_for(&rec(IPPS_SERVICE)).as_deref(),
            Some("ipps://EPSON1A2B3C.local:631/ipp/print")
        );
        // No rp → the IPP Everywhere default path; a leading slash is dropped.
        let mut r = rec(IPP_SERVICE);
        r.rp = None;
        assert!(uri_for(&r).unwrap().ends_with(":631/ipp/print"));
        r.rp = Some("/printers/lab".into());
        assert!(uri_for(&r).unwrap().ends_with(":631/printers/lab"));
        // No port → not addressable.
        r.port = 0;
        assert!(uri_for(&r).is_none());
    }

    #[test]
    fn host_falls_back_to_addresses() {
        let mut r = rec(IPP_SERVICE);
        r.host = String::new();
        assert_eq!(uri_host(&r).as_deref(), Some("10.0.0.5"));
        r.addresses = vec!["fe80::1".parse().unwrap()];
        assert_eq!(uri_host(&r).as_deref(), Some("[fe80::1]"));
        r.addresses.clear();
        assert!(uri_for(&r).is_none());
    }

    #[test]
    fn instance_label_cuts_only_the_service_tail() {
        assert_eq!(
            instance_label("Lab v2.1 Printer._ipp._tcp.local.", IPP_SERVICE),
            "Lab v2.1 Printer"
        );
        assert_eq!(instance_label("odd", IPP_SERVICE), "odd");
    }

    #[test]
    fn fr_printers_discovery_excludes_installed_devices() {
        let r = rec(IPP_SERVICE);
        // By queue name (Windows names queues after the advertised instance).
        assert!(is_installed(
            &r,
            &[("epson l15160 series".into(), "WSD-1234".into())]
        ));
        // By host name in the port.
        assert!(is_installed(
            &r,
            &[(
                "Office".into(),
                "http://EPSON1A2B3C.local:631/ipp/print".into()
            )]
        ));
        // By address, as a whole host only.
        assert!(is_installed(&r, &[("Office".into(), "IP_10.0.0.5".into())]));
        assert!(is_installed(
            &r,
            &[("Office".into(), "http://10.0.0.5:631/ipp/print".into())]
        ));
        assert!(!is_installed(
            &r,
            &[("Office".into(), "IP_110.0.0.5".into())]
        ));
        assert!(!is_installed(
            &r,
            &[("Office".into(), "IP_10.0.0.50".into())]
        ));
        assert!(!is_installed(&r, &[("Other".into(), "USB001".into())]));
    }

    #[test]
    fn fr_printers_discovery_rows_are_deduplicated_and_typed() {
        let records = vec![rec(IPP_SERVICE), rec(IPP_SERVICE), rec(IPPS_SERVICE)];
        let rows = to_discovered(&records, &[]);
        assert_eq!(
            rows.len(),
            2,
            "same URI twice is one row; ipp and ipps are two"
        );
        assert_eq!(rows[0].name, "EPSON L15160 Series");
        assert_eq!(rows[0].via, "mDNS · IPP");
        assert_eq!(rows[1].via, "mDNS · IPPS");
        assert_eq!(rows[0].id, "ipp://EPSON1A2B3C.local:631/ipp/print");
        assert_eq!(rows[0].connection, "EPSON1A2B3C.local (IPP)");

        let installed = vec![("EPSON L15160 Series".to_string(), String::new())];
        assert!(to_discovered(&records, &installed).is_empty());
    }
}
