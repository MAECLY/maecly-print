import { describe, expect, it } from 'vitest';

import { CATALOGS } from '@/i18n';
import type { Job } from '@/lib/ipc';
import { toUiQueueJob } from '@/lib/ipc';

import { localizeJobError } from './jobError';

/** Every code the dispatcher (queue/dispatcher.rs, queue/persist.rs) stores. */
const BACKEND_CODES = [
  'conversion_timeout',
  'conversion_failed',
  'print_failed',
  'unsupported_setting',
  'dependency_missing',
  'unsupported_format',
  'cancelled',
  'print_timeout',
  'print_unconfirmed',
  'printer_unavailable',
  'printer_disabled',
  'os_cancelled',
  'print_aborted',
  'interrupted_by_shutdown',
] as const;

function job(over: Partial<Job>): Job {
  return {
    id: 'j1',
    batch_id: 'b1',
    input_path: '/docs/a.pdf',
    file_name: 'a.pdf',
    input_ext: 'pdf',
    input_bytes: 2048,
    pages: 1,
    settings: { printer: null } as unknown as Job['settings'],
    profile_name: 'Quick',
    state: 'failed',
    error: null,
    progress: null,
    attempts: 0,
    submitted_at: '2026-01-01T10:00:00Z',
    started_at: null,
    finished_at: null,
    printer_resolved: null,
    cache_pdf: null,
    ...over,
  } as Job;
}

describe('localizeJobError (FR-092)', () => {
  it('FR-092: every backend job error code has an English and a Spanish sentence', () => {
    for (const locale of ['en-US', 'es-ES'] as const) {
      const codes = CATALOGS[locale].queue.jobErrors.codes as Record<
        string,
        string
      >;
      for (const code of BACKEND_CODES) {
        expect(codes[code], `${locale} ${code}`).toBeTruthy();
      }
    }
  });

  it('FR-092: says a stored error in Spanish, filled with its params', () => {
    expect(
      localizeJobError(
        {
          code: 'print_timeout',
          message: 'the printer did not finish within 90s — …',
          params: { secs: '90' },
        },
        { locale: 'es-ES' },
      ),
    ).toBe(
      'La impresora no terminó en 90 s. El trabajo sigue en la cola de impresión del sistema: revisa la impresora antes de reintentarlo.',
    );
  });

  it('FR-058/FR-092: a missing dependency says the shared remediation, in Spanish', () => {
    const missing = {
      code: 'dependency_missing',
      reason: 'libre-office',
      message: 'LibreOffice is not installed',
      params: { detail: 'LibreOffice is not installed' },
    };
    const es = localizeJobError(missing, { locale: 'es-ES' });
    expect(es).toBe(CATALOGS['es-ES'].common.deps['libre-office'].remediation);
    expect(es).not.toMatch(/is not installed/);
    // A job stored before reasons existed keeps the family sentence.
    expect(
      localizeJobError(
        {
          code: 'dependency_missing',
          message: 'x',
          params: { detail: 'lp' },
        },
        { locale: 'es-ES' },
      ),
    ).toBe('Falta un programa necesario: lp');
  });

  it('FR-092: an engine reason is said without its English detail', () => {
    const text = localizeJobError(
      {
        code: 'conversion_failed',
        reason: 'converter_failed',
        message: 'conversion failed: LibreOffice failed (exit status: 1)',
        params: {
          tool: 'LibreOffice',
          detail: 'LibreOffice failed (exit status: 1)',
        },
      },
      { locale: 'es-ES' },
    );
    expect(text).toBe('LibreOffice no pudo convertir este archivo a PDF.');
  });

  it('FR-092: a reason picks the variant sentence', () => {
    expect(
      localizeJobError(
        {
          code: 'unsupported_setting',
          message: "the driver for 'Lab' does not support duplex",
          reason: 'duplex',
          params: { printer: 'Lab' },
        },
        { locale: 'es-ES' },
      ),
    ).toBe('El controlador de «Lab» no puede imprimir a doble cara.');
    expect(
      localizeJobError(
        {
          code: 'printer_unavailable',
          message: 'no printer is configured',
          reason: 'no_printer',
        },
        { locale: 'en-US' },
      ),
    ).toBe('No printer is configured. Choose a printer first.');
  });

  it('FR-092: the printer param is shown by its display name when known', () => {
    expect(
      localizeJobError(
        {
          code: 'printer_disabled',
          message: 'x',
          params: { printer: 'EPSON_L15160' },
        },
        {
          locale: 'en-US',
          printerName: (id) =>
            id === 'EPSON_L15160' ? 'EPSON L15160 Series' : undefined,
        },
      ),
    ).toContain('“EPSON L15160 Series”');
  });

  it('FR-092: falls back to the English message for an unknown code or missing params', () => {
    expect(
      localizeJobError(
        { code: 'something_new', message: 'Raw detail' },
        { locale: 'es-ES' },
      ),
    ).toBe('Raw detail');
    // A job persisted before params existed: no sentence with a hole in it.
    expect(
      localizeJobError(
        { code: 'print_timeout', message: 'old English text' },
        { locale: 'es-ES' },
      ),
    ).toBe('old English text');
    // A code without placeholders needs no params at all.
    expect(
      localizeJobError(
        { code: 'os_cancelled', message: 'x' },
        { locale: 'es-ES' },
      ),
    ).toBe('El trabajo se canceló en la cola de impresión del sistema.');
  });
});

describe('toUiQueueJob error notes', () => {
  const shutdown = {
    code: 'interrupted_by_shutdown',
    message: 'Pluryprint was closed while this job was in progress.',
  };

  it('a paused job interrupted by a shutdown offers to resume', () => {
    const ui = toUiQueueJob(job({ state: 'paused', error: shutdown }));
    expect(ui.noteKey).toBe('interruptedByShutdown');
    expect(ui.error).toBeUndefined();
  });

  it('a cancelled job no longer says "resume to continue"', () => {
    const ui = toUiQueueJob(job({ state: 'cancelled', error: shutdown }));
    expect(ui.noteKey).toBeUndefined();
    expect(ui.error).toBeUndefined();
  });

  it('FR-092: keeps the stored error so views can re-translate it', () => {
    const error = {
      code: 'print_aborted',
      message: 'the printer aborted the job — check the printer, then retry',
    };
    const ui = toUiQueueJob(job({ state: 'failed', error }));
    expect(ui.errorInfo).toEqual(error);
    expect(ui.error).toBe(
      'The printer aborted the job. Check the printer, then retry.',
    );
  });
});
