import { describe, it, expect } from 'vitest';

import {
  buildPrinterUri,
  DEFAULT_PORTS,
  isValidHost,
  type PrinterProtocol,
} from './printerAddress';

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
