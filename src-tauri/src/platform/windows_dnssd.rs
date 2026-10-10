//! Network printer discovery on Windows through the OS DNS-SD client
//! (`DnsServiceBrowse` / `DnsServiceResolve`, Windows 10 1809+).
//!
//! # Why the OS resolver and not our own mDNS socket
//!
//! Printers answer an mDNS query by **multicasting** to `224.0.0.251:5353`,
//! which is inbound traffic. Windows Defender Firewall's built-in
//! "mDNS (UDP-In)" rule only admits it for the DNS Client service
//! (`svchost.exe`, service `dnscache`), not for `pluryprint.exe`; the per-user
//! installer adds no rule of its own (no admin), and the first listening
//! socket raises a prompt a standard user cannot approve. An in-process mDNS
//! browser therefore heard nothing and "Scan network" reported an empty
//! network. These APIs run the query inside `dnscache`, so the answers come
//! back through the rule that already exists.
//!
//! The DNS-SD callbacks run on system threads with an opaque context pointer.
//! Nothing they could outlive is passed: the context is a plain id, and the
//! results live in a process-wide table keyed by it. A callback that fires
//! after its browse was cancelled (and its entry removed) finds nothing to
//! write to and only frees what the API handed it.

use std::collections::HashMap;
use std::ffi::c_void;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use windows::core::{PCWSTR, PWSTR};
use windows::Win32::NetworkManagement::Dns::{
    DnsFree, DnsFreeRecordList, DnsServiceBrowse, DnsServiceBrowseCancel, DnsServiceFreeInstance,
    DnsServiceResolve, DnsServiceResolveCancel, DNS_QUERY_REQUEST_VERSION1, DNS_RECORDW,
    DNS_SERVICE_BROWSE_REQUEST, DNS_SERVICE_BROWSE_REQUEST_0, DNS_SERVICE_CANCEL,
    DNS_SERVICE_INSTANCE, DNS_SERVICE_RESOLVE_REQUEST, DNS_TYPE_PTR,
};

use super::dnssd;

/// `DNS_REQUEST_PENDING`: what an asynchronous DNS-SD call returns when it
/// was started (winerror.h; not exported by the `windows` crate).
const DNS_REQUEST_PENDING: i32 = 9506;

/// How long the resolves of the instances found may take, after the browse.
/// Answers usually come straight from the cache the browse filled.
const RESOLVE_WINDOW: Duration = Duration::from_secs(2);

/// What the callbacks deliver, keyed by the request's context id.
#[derive(Default)]
struct Table {
    /// Browse id → instance names seen so far.
    browses: HashMap<usize, Vec<String>>,
    /// Resolve id → `None` while pending, `Some(result)` once answered
    /// (`Some(None)` when the instance could not be resolved).
    resolves: HashMap<usize, Option<Option<Instance>>>,
}

/// One resolved instance, copied out of `DNS_SERVICE_INSTANCE`.
#[derive(Debug, Clone)]
struct Instance {
    name: String,
    host: String,
    port: u16,
    addresses: Vec<IpAddr>,
    txt: Vec<(String, String)>,
}

fn table() -> &'static Mutex<Table> {
    static TABLE: OnceLock<Mutex<Table>> = OnceLock::new();
    TABLE.get_or_init(Mutex::default)
}

fn lock() -> std::sync::MutexGuard<'static, Table> {
    table().lock().unwrap_or_else(|e| e.into_inner())
}

fn next_id() -> usize {
    static NEXT: AtomicUsize = AtomicUsize::new(1);
    NEXT.fetch_add(1, Ordering::Relaxed)
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Read a (possibly null) wide string the API owns.
fn read(p: PWSTR) -> String {
    if p.is_null() {
        String::new()
    } else {
        // SAFETY: the API hands out NUL-terminated strings that stay valid
        // until the record list / instance is freed, which happens after this.
        unsafe { p.to_string() }.unwrap_or_default()
    }
}

/// `DnsServiceBrowse` callback: collect the PTR targets (instance names).
unsafe extern "system" fn on_browse(status: u32, ctx: *const c_void, records: *const DNS_RECORDW) {
    let mut names = Vec::new();
    let mut r = records;
    while !r.is_null() {
        // SAFETY: a non-null record in the list the API passed us.
        let rec = unsafe { &*r };
        // A TTL of 0 is a "goodbye": the instance is leaving, not arriving.
        if rec.wType == DNS_TYPE_PTR.0 && rec.dwTtl > 0 {
            // SAFETY: PTR records carry the PTR arm of the data union.
            names.push(read(unsafe { rec.Data.PTR.pNameHost }));
        }
        r = rec.pNext;
    }
    if !records.is_null() {
        // SAFETY: the callback owns the record list it is given.
        unsafe { DnsFree(Some(records.cast()), DnsFreeRecordList) };
    }
    if status == 0 {
        if let Some(found) = lock().browses.get_mut(&(ctx as usize)) {
            found.extend(names.into_iter().filter(|n| !n.is_empty()));
        }
    }
}

/// `DnsServiceResolve` completion: copy the instance out and free it.
unsafe extern "system" fn on_resolve(
    status: u32,
    ctx: *const c_void,
    instance: *const DNS_SERVICE_INSTANCE,
) {
    let copied = if status == 0 && !instance.is_null() {
        // SAFETY: a non-null instance the API passed us.
        Some(unsafe { copy_instance(&*instance) })
    } else {
        None
    };
    if !instance.is_null() {
        // SAFETY: the completion owns the instance it is given.
        unsafe { DnsServiceFreeInstance(instance) };
    }
    if let Some(slot) = lock().resolves.get_mut(&(ctx as usize)) {
        *slot = Some(copied);
    }
}

/// # Safety
/// `i` must be a live instance returned by the DNS-SD API.
unsafe fn copy_instance(i: &DNS_SERVICE_INSTANCE) -> Instance {
    let mut addresses = Vec::new();
    if !i.ip4Address.is_null() {
        // IP4_ADDRESS is stored in network byte order.
        // SAFETY: non-null pointer into the instance.
        let raw = unsafe { *i.ip4Address };
        addresses.push(IpAddr::V4(Ipv4Addr::from(raw.to_ne_bytes())));
    }
    if !i.ip6Address.is_null() {
        // SAFETY: non-null pointer into the instance; every arm is 16 bytes.
        let bytes = unsafe { (*i.ip6Address).IP6Byte };
        addresses.push(IpAddr::V6(Ipv6Addr::from(bytes)));
    }
    let mut txt = Vec::new();
    if !i.keys.is_null() && !i.values.is_null() {
        for n in 0..i.dwPropertyCount as usize {
            // SAFETY: `keys`/`values` hold `dwPropertyCount` entries.
            let (k, v) = unsafe { (*i.keys.add(n), *i.values.add(n)) };
            txt.push((read(k), read(v)));
        }
    }
    Instance {
        name: read(i.pszInstanceName),
        host: read(i.pszHostName),
        port: i.wPort,
        addresses,
        txt,
    }
}

/// `_ipp._tcp.local.` → `_ipp._tcp.local`, the form the API takes.
fn query_name(service_type: &str) -> String {
    service_type.trim_end_matches('.').to_string()
}

/// A resolved instance as the shared [`dnssd::Resolved`] record. The API
/// reports names without the trailing root dot; it is put back so the
/// platform-neutral code sees the same shapes on every OS.
fn to_resolved(i: Instance, service_type: &str) -> dnssd::Resolved {
    let fullname = if i.name.ends_with('.') {
        i.name
    } else {
        format!("{}.", i.name)
    };
    let get = |key: &str| {
        i.txt
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(key))
            .map(|(_, v)| v.clone())
    };
    dnssd::Resolved {
        fullname,
        service_type: service_type.to_string(),
        host: i.host.clone(),
        port: i.port,
        addresses: i.addresses.clone(),
        rp: get("rp"),
        ty: get("ty"),
    }
}

/// Browse every service type in `service_types` for `window`, then resolve
/// the instances found. A browse that cannot be started is an error (the UI
/// shows a scan error), never an empty network.
pub fn browse(service_types: &[&str], window: Duration) -> anyhow::Result<Vec<dnssd::Resolved>> {
    // ---- browse ---------------------------------------------------------
    let mut running = Vec::new();
    let mut started: anyhow::Result<()> = Ok(());
    for &service_type in service_types {
        let id = next_id();
        lock().browses.insert(id, Vec::new());
        let name = wide(&query_name(service_type));
        let request = DNS_SERVICE_BROWSE_REQUEST {
            Version: DNS_QUERY_REQUEST_VERSION1.0,
            InterfaceIndex: 0,
            QueryName: PCWSTR(name.as_ptr()),
            Anonymous: DNS_SERVICE_BROWSE_REQUEST_0 {
                pBrowseCallback: Some(on_browse),
            },
            pQueryContext: id as *mut c_void,
        };
        let mut cancel = DNS_SERVICE_CANCEL::default();
        // SAFETY: `request` and `name` outlive the call; `cancel` is a valid
        // out pointer.
        let rc = unsafe { DnsServiceBrowse(&request, &mut cancel) };
        if rc != DNS_REQUEST_PENDING {
            lock().browses.remove(&id);
            // FR-092: a scan that could not start is `discovery_failed`; the
            // DNS-SD code is the technical detail under that sentence.
            let detail =
                format!("could not browse for {service_type} printers (DNS-SD error {rc})");
            started = Err(crate::util::error::LocalizedError::new(
                crate::util::error::IpcErrorCode::Internal,
                "discovery_failed",
                detail.clone(),
            )
            .param("detail", detail)
            .into());
            break;
        }
        running.push((id, service_type, cancel, name));
    }
    if started.is_ok() {
        std::thread::sleep(window);
    }
    let mut found: Vec<(String, &str)> = Vec::new();
    for (id, service_type, cancel, _name) in &running {
        // SAFETY: a handle from a browse that was started.
        unsafe { DnsServiceBrowseCancel(cancel) };
        for name in lock().browses.remove(id).unwrap_or_default() {
            if !found.iter().any(|(n, _)| n.eq_ignore_ascii_case(&name)) {
                found.push((name, service_type));
            }
        }
    }
    started?;

    // ---- resolve --------------------------------------------------------
    let mut pending = Vec::new();
    for (name, service_type) in found {
        let id = next_id();
        lock().resolves.insert(id, None);
        let mut query = wide(&name);
        let request = DNS_SERVICE_RESOLVE_REQUEST {
            Version: DNS_QUERY_REQUEST_VERSION1.0,
            InterfaceIndex: 0,
            QueryName: PWSTR(query.as_mut_ptr()),
            pResolveCompletionCallback: Some(on_resolve),
            pQueryContext: id as *mut c_void,
        };
        let mut cancel = DNS_SERVICE_CANCEL::default();
        // SAFETY: as for the browse; `query` is kept alive below.
        let rc = unsafe { DnsServiceResolve(&request, &mut cancel) };
        if rc == DNS_REQUEST_PENDING {
            pending.push((id, service_type, cancel, query));
        } else {
            lock().resolves.remove(&id);
            tracing::debug!(event = "printer_discovery_resolve_failed", instance = %name, code = rc);
        }
    }
    let deadline = Instant::now() + RESOLVE_WINDOW;
    while Instant::now() < deadline {
        let table = lock();
        if pending
            .iter()
            .all(|(id, ..)| matches!(table.resolves.get(id), Some(Some(_)) | None))
        {
            break;
        }
        drop(table);
        std::thread::sleep(Duration::from_millis(50));
    }
    let mut records = Vec::new();
    for (id, service_type, cancel, _query) in &pending {
        let answered = lock().resolves.remove(id);
        match answered {
            Some(Some(Some(instance))) => records.push(to_resolved(instance, service_type)),
            Some(Some(None)) => {}
            // Still pending: stop it. A late completion finds no entry.
            _ => {
                // SAFETY: a handle from a resolve that was started.
                unsafe { DnsServiceResolveCancel(cancel) };
            }
        }
    }
    Ok(records)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn instances_map_onto_the_shared_record_shape() {
        let r = to_resolved(
            Instance {
                name: "Lab Printer._ipp._tcp.local".into(),
                host: "LAB-1.local".into(),
                port: 631,
                addresses: vec![IpAddr::V4(Ipv4Addr::new(10, 0, 0, 7))],
                txt: vec![
                    ("txtvers".into(), "1".into()),
                    ("RP".into(), "ipp/print".into()),
                    ("ty".into(), "Lab Laser".into()),
                ],
            },
            dnssd::IPP_SERVICE,
        );
        assert_eq!(r.fullname, "Lab Printer._ipp._tcp.local.");
        assert_eq!(
            dnssd::instance_label(&r.fullname, &r.service_type),
            "Lab Printer"
        );
        assert_eq!(r.rp.as_deref(), Some("ipp/print"));
        assert_eq!(r.ty.as_deref(), Some("Lab Laser"));
        assert_eq!(
            dnssd::uri_for(&r).as_deref(),
            Some("ipp://LAB-1.local:631/ipp/print")
        );
        assert_eq!(query_name(dnssd::IPPS_SERVICE), "_ipps._tcp.local");
    }

    /// The browse goes through the DNS Client service and must start (and
    /// return) on any Windows with a network stack, finding whatever is there.
    #[test]
    #[ignore = "waits for the browse window; run on the Windows VM"]
    fn browse_smoke() {
        let found = browse(
            &[dnssd::IPP_SERVICE, dnssd::IPPS_SERVICE],
            Duration::from_secs(3),
        )
        .expect("the OS DNS-SD browse starts");
        for r in &found {
            eprintln!("found {r:?}");
            assert!(r.fullname.ends_with('.'));
        }
    }
}
