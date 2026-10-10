import { describe, it, expect } from 'vitest';

import {
  buildPrinterUri,
  DEFAULT_PORTS,
  isValidDisplayName,
  isValidHost,
  isValidPort,
  type PrinterProtocol,
} from './printerAddress';

describe('isValidPort', () => {
  it.each([
    ['631', true],
    ['1', true],
    ['65535', true],
    [' 9100 ', true],
    ['0', false],
    ['-5', false],
    ['70000', false],
    ['631.5', false],
    ['1e3', false],
    ['', false],
    ['abc', false],
  ])('FR-044: isValidPort(%j) === %s', (input, expected) => {
    expect(isValidPort(input)).toBe(expected);
  });
});

describe('isValidDisplayName', () => {
  it.each([
    ["Ana's printer", true],
    ['Impresora de recepción', true],
    ['Printer?2', true],
    ['', false],
    ['   ', false],
    ['tab\there', false],
  ])('FR-044: isValidDisplayName(%j) === %s', (input, expected) => {
    expect(isValidDisplayName(input)).toBe(expected);
  });
});

describe('isValidHost', () => {
  it.each([
    ['10.0.0.41', true],
    ['192.168.1.255', true],
    ['0.0.0.0', true],
    ['255.255.255.255', true],
    ['printer.local', true],
    ['hp-laserjet.corp.example.com', true],
    ['localhost', true],
    ['[::1]', true],
    ['[2001:db8::1]', true],
  ])('accepts %s', (host, expected) => {
    expect(isValidHost(host)).toBe(expected);
  });

  it.each([
    ['', false],
    ['   ', false],
    ['10.0.0.41 ', true], // trimmed
    ['256.1.1.1', false], // octet out of range
    ['999.999.999.999', false],
    ['ipp://10.0.0.41', false], // protocol prefix
    ['http://printer', false],
    ['a b', false], // internal space
    ['10.0.0.41:631', false], // host must not carry a port
    ['-bad.example.com', false], // label starts with hyphen
    ['bad-.example.com', false], // label ends with hyphen
    ['[not:hex:zzzz]', false], // invalid IPv6 groups
  ])('rejects/handles %s', (host, expected) => {
    expect(isValidHost(host)).toBe(expected);
  });
});

describe('buildPrinterUri', () => {
  it('appends the default IPP queue path and port', () => {
    expect(buildPrinterUri('ipp', '10.0.0.41')).toBe(
      'ipp://10.0.0.41:631/ipp/print',
    );
  });

  it('uses the same default path for IPPS', () => {
    expect(buildPrinterUri('ipps', 'printer.local')).toBe(
      'ipps://printer.local:631/ipp/print',
    );
  });

  it('omits a queue path for raw socket and keeps its port', () => {
    expect(buildPrinterUri('socket', '10.0.0.41')).toBe(
      'socket://10.0.0.41:9100',
    );
  });

  it('omits a queue path for LPD', () => {
    expect(buildPrinterUri('lpd', '10.0.0.41')).toBe('lpd://10.0.0.41:515');
  });

  it('always includes an explicit port even when it equals the default', () => {
    expect(buildPrinterUri('ipp', '10.0.0.41', 631)).toContain(':631');
  });

  it('honours a custom port', () => {
    expect(buildPrinterUri('socket', '10.0.0.41', 9101)).toBe(
      'socket://10.0.0.41:9101',
    );
  });

  it('overrides the queue path and trims a leading slash', () => {
    expect(buildPrinterUri('ipp', '10.0.0.41', 631, '/printers/main')).toBe(
      'ipp://10.0.0.41:631/printers/main',
    );
  });

  it('falls back to the protocol default when the queue path is blank', () => {
    expect(buildPrinterUri('ipp', '10.0.0.41', 631, '   ')).toBe(
      'ipp://10.0.0.41:631/ipp/print',
    );
  });

  it('trims the host', () => {
    expect(buildPrinterUri('ipp', '  10.0.0.41  ')).toBe(
      'ipp://10.0.0.41:631/ipp/print',
    );
  });

  it('FR-044: keeps a bracketed IPv6 literal intact in the authority', () => {
    // The brackets are what separate the address's colons from the port —
    // dropping them would produce `ipp://fe80::1:631/…`, a different host.
    expect(buildPrinterUri('ipp', '[fe80::1]')).toBe(
      'ipp://[fe80::1]:631/ipp/print',
    );
    expect(buildPrinterUri('socket', '[2001:db8::1]', 9100)).toBe(
      'socket://[2001:db8::1]:9100',
    );
  });

  it('FR-042: only builds URIs the backend can install', () => {
    // Every scheme the form can emit is one the platform layer accepts
    // (`lpadmin -m everywhere` for ipp/ipps, `-m raw` for socket/lpd).
    const schemes = (['ipp', 'ipps', 'socket', 'lpd'] as PrinterProtocol[]).map(
      (p) => buildPrinterUri(p, 'host').split('://')[0],
    );
    expect(schemes).toEqual(['ipp', 'ipps', 'socket', 'lpd']);
  });

  it('resolves each protocol to its default port', () => {
    const ports: Record<PrinterProtocol, number> = {
      ipp: 631,
      ipps: 631,
      socket: 9100,
      lpd: 515,
    };
    for (const proto of Object.keys(ports) as PrinterProtocol[]) {
      expect(buildPrinterUri(proto, 'host')).toContain(`:${ports[proto]}`);
      expect(DEFAULT_PORTS[proto]).toBe(ports[proto]);
    }
  });
});
