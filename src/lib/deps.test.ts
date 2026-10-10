import { describe, expect, it } from 'vitest';

import { CATALOGS, translate } from '@/i18n';
import { missingDependency, remediationKey } from '@/lib/deps';

describe('remediationKey (FR-058)', () => {
  it('FR-058: LibreOffice names RTF on Windows only — macOS prints RTF without it', () => {
    for (const locale of ['en-US', 'es-ES'] as const) {
      const catalog = CATALOGS[locale];
      const win = translate(
        catalog,
        'common',
        remediationKey('libre-office', { mac: false, locale }),
      );
      const mac = translate(
        catalog,
        'common',
        remediationKey('libre-office', { mac: true, locale }),
      );
      expect(win).toContain('RTF');
      expect(mac).not.toContain('RTF');
      expect(mac).toContain('OpenDocument');
    }
  });

  it('FR-058: a dependency without a macOS variant keeps its one sentence', () => {
    expect(remediationKey('sumatra-pdf', { mac: true })).toBe(
      'deps.sumatra-pdf.remediation',
    );
  });

  it('FR-058: the codec remediation points at Resume, the action a paused job has', () => {
    for (const locale of ['en-US', 'es-ES'] as const) {
      for (const id of ['heif-codec', 'hevc-codec']) {
        const text = translate(
          CATALOGS[locale],
          'common',
          remediationKey(id, { mac: false, locale }),
        );
        expect(text).not.toMatch(/retry|vuelve a intentar/i);
        expect(text).toMatch(/resumed|reanudar/);
      }
    }
  });

  it('FR-058: missingDependency reads the dependency of a paused job only', () => {
    const errorInfo = {
      code: 'dependency_missing',
      message: 'x',
      reason: 'libre-office',
    };
    expect(missingDependency({ status: 'paused', errorInfo })).toBe(
      'libre-office',
    );
    expect(missingDependency({ status: 'failed', errorInfo })).toBeNull();
    expect(
      missingDependency({
        status: 'paused',
        errorInfo: { ...errorInfo, code: 'printer_unavailable' },
      }),
    ).toBeNull();
  });
});
