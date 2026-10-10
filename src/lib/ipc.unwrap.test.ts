import { describe, expect, it } from 'vitest';

import { setActiveLocale } from '@/i18n';
import { unwrap } from '@/lib/ipc';
import { IpcFailure } from '@/lib/ipcError';

describe('unwrap (FR-092)', () => {
  it("FR-092: a bare string in the error branch (the Tauri layer's own rejection) reads as the translated Internal sentence", async () => {
    // The generated bindings hand a non-Error rejection such as "command
    // not allowed" over as the error branch.
    setActiveLocale('es-ES');
    try {
      const err = await unwrap(
        Promise.resolve({
          status: 'error' as const,
          error: 'command print_test_page not allowed' as never,
        }),
      ).catch((e: unknown) => e);
      expect(err).toBeInstanceOf(IpcFailure);
      const failure = err as IpcFailure;
      expect(failure.code).toBe('Internal');
      expect(failure.message).toBe(
        'Algo ha fallado dentro de Pluryprint. La carpeta de registros tiene los detalles.',
      );
      expect(failure.detail).toBe('command print_test_page not allowed');
    } finally {
      setActiveLocale('en-US');
    }
  });

  it('FR-092: a command-returned error is localized from its code and reason', async () => {
    const err = await unwrap(
      Promise.resolve({
        status: 'error' as const,
        error: {
          code: 'InvalidState' as const,
          reason: 'job_not_paused',
          message: 'only a paused job can be resumed',
        },
      }),
    ).catch((e: unknown) => e);
    expect((err as Error).message).toBe('Only a paused job can be resumed.');
  });
});
