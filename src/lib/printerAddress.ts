/**
 * Pure helpers for the "By IP address" add-printer flow: validating a
 * user-typed host and composing a spooler device URI from protocol + host +
 * port + queue path. No Tauri imports — trivially unit-testable in isolation.
 */

export type PrinterProtocol = 'ipp' | 'ipps' | 'socket' | 'lpd';

/** TCP port assumed for each protocol when the user leaves the field blank. */
export const DEFAULT_PORTS: Record<PrinterProtocol, number> = {
  ipp: 631,
  ipps: 631,
  socket: 9100,
  lpd: 515,
};

/**
 * Resource path appended after the authority for driverless IPP/IPPS. Raw
 * socket (JetDirect) and LPD carry no path, so they are absent here.
 */
const DEFAULT_QUEUE_PATH: Partial<Record<PrinterProtocol, string>> = {
  ipp: 'ipp/print',
  ipps: 'ipp/print',
};

/** Permissive IPv6 check for the *inside* of a bracketed literal (`[…]`). */
function isIPv6(inner: string): boolean {
  if (!inner) return false;
  // Only hex digits, colons, and a possible embedded IPv4 tail.
  if (!/^[0-9a-fA-F:.]+$/.test(inner)) return false;
  // At most one "::" compression.
  const compressions = inner.match(/::/g);
  if (compressions && compressions.length > 1) return false;
  const groups = inner.split(':');
  if (groups.length < 3 || groups.length > 8) return false;
  return groups.every((g) => g === '' || /^[0-9a-fA-F]{1,4}$/.test(g));
}

/**
 * True when `s` looks like a routable host: a dotted-quad IPv4 with 0–255
 * octets, an RFC-1123 hostname, or a bracketed IPv6 literal. Deliberately
 * permissive, but rejects empty input, whitespace, and protocol prefixes
 * (`ipp://…`) so the URI builder never receives a malformed authority.
 */
export function isValidHost(s: string): boolean {
  const h = s.trim();
  if (!h) return false;
  if (/\s/.test(h)) return false;
  if (/:\/\//.test(h)) return false;

  // Bracketed IPv6, e.g. [2001:db8::1] or [::1].
  if (h.startsWith('[') && h.endsWith(']')) {
    return isIPv6(h.slice(1, -1));
  }

  // Dotted-quad IPv4 with each octet in 0–255.
  if (/^\d{1,3}(\.\d{1,3}){3}$/.test(h)) {
    return h.split('.').every((oct) => {
      const n = Number(oct);
      return n >= 0 && n <= 255;
    });
  }

  // RFC-1123 hostname: dot-separated labels, each 1–63 chars, alphanumeric or
  // hyphen, not starting/ending with a hyphen.
  const label = /^[a-zA-Z0-9]([a-zA-Z0-9-]{0,61}[a-zA-Z0-9])?$/;
  return h.split('.').every((l) => label.test(l));
}

/**
 * True when `s` is a TCP port: a plain integer from 1 to 65535. Rejects
 * signs, decimals, exponents and blanks so `buildPrinterUri` never emits
 * `ipp://host:-5/…` or `:70000`.
 */
export function isValidPort(s: string): boolean {
  const p = s.trim();
  if (!/^\d{1,5}$/.test(p)) return false;
  const n = Number(p);
  return n >= 1 && n <= 65535;
}

/**
 * True when `s` can be a printer's display name: not blank, and free of
 * control characters (the spooler stores it as the queue description). Any
 * other character is fine — the backend derives an `lpadmin`-legal queue id
 * from it and keeps this text verbatim for people to read.
 */
export function isValidDisplayName(s: string): boolean {
  const n = s.trim();
  // eslint-disable-next-line no-control-regex
  return n.length > 0 && !/[\u0000-\u001f\u007f]/.test(n);
}

/**
 * Compose a spooler device URI, e.g.
 * `buildPrinterUri('ipp', '10.0.0.41')` → `ipp://10.0.0.41:631/ipp/print`.
 *
 * The resolved port is always emitted for explicitness (never omitted, even
 * when it equals the protocol default). `queuePath` overrides the protocol's
 * default resource path; a leading slash is trimmed. Raw socket/LPD carry no
 * path unless one is supplied.
 */
export function buildPrinterUri(
  proto: PrinterProtocol,
  host: string,
  port?: number,
  queuePath?: string,
): string {
  const resolvedPort = port ?? DEFAULT_PORTS[proto];
  const explicit = queuePath?.trim();
  const rawPath =
    explicit && explicit.length > 0
      ? explicit
      : (DEFAULT_QUEUE_PATH[proto] ?? '');
  const path = rawPath.replace(/^\/+/, '');
  const base = `${proto}://${host.trim()}:${resolvedPort}`;
  return path ? `${base}/${path}` : base;
}
