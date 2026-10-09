import { describe, expect, it } from 'vitest';

import { IpcFailure, localizeIpcError } from './ipcError';

describe('localizeIpcError', () => {
  it('FR-092: translates a known code + reason with its params', () => {
    const payload = {
      code: 'UpdateCheckFailed',
      reason: 'bad_status',
      params: { status: '502' },
      message: 'github.com returned an unexpected status (502)',
    } as const;
    expect(localizeIpcError(payload, 'es-ES')).toBe(
      'github.com devolvió un estado inesperado (502).',
    );
    expect(localizeIpcError(payload, 'en-US')).toBe(
      'github.com returned an unexpected status (502).',
    );
  });

  it('FR-092: a profile name clash is fully translated', () => {
    expect(
      localizeIpcError(
        {
          code: 'InvalidRequest',
          reason: 'name_taken',
          params: { name: 'Oficina' },
          message: 'a profile named "Oficina" already exists',
        },
        'es-ES',
      ),
    ).toBe('Ya existe un perfil llamado «Oficina».');
  });

  it('FR-092: a known code without a known reason keeps the detail in a translated frame', () => {
    expect(
      localizeIpcError(
        { code: 'PrintFailed', reason: 'brand_new', message: 'lp: boom' },
        'es-ES',
      ),
    ).toBe('Error al imprimir: lp: boom');
    expect(
      localizeIpcError({ code: 'JobNotFound', message: '0192-…' }, 'es-ES'),
    ).toBe('Ese trabajo ya no está en la cola.');
  });

  it('FR-092: falls back to the raw message only for an unknown code', () => {
    expect(
      localizeIpcError(
        { code: 'SomethingNew' as never, message: 'raw detail' },
        'es-ES',
      ),
    ).toBe('raw detail');
  });

  it('IpcFailure keeps the English detail and the stable fields', () => {
    const err = new IpcFailure(
      {
        code: 'DependencyMissing',
        reason: 'ippfind_missing',
        message: '`ippfind` is not installed',
      },
      'es-ES',
    );
    expect(err).toBeInstanceOf(Error);
    expect(err.message).toMatch(/herramientas cliente de CUPS/);
    expect(err.detail).toBe('`ippfind` is not installed');
    expect(err.code).toBe('DependencyMissing');
    expect(err.reason).toBe('ippfind_missing');
  });
});
