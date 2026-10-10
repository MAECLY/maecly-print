import { toast } from 'sonner';
import { afterEach, describe, expect, it, vi } from 'vitest';

import { CATALOGS, translate } from '@/i18n';
import type { TFunction } from '@/i18n/I18nProvider';
import type { DependencyReport, InstallOutcome } from '@/ipc/bindings';
import { IpcFailure } from '@/lib/ipcError';
import type { InstallEnd } from '@/stores/installStore';
import { mkJob } from '@/test/fixtures';

import { announceInstallEnd, resumeJobsWaitingOn } from './install';

const tEn: TFunction<'printJob'> = (key, params) =>
  translate(CATALOGS['en-US'], 'printJob', key, params);
const tEs: TFunction<'printJob'> = (key, params) =>
  translate(CATALOGS['es-ES'], 'printJob', key, params);

function waitingJob(id: string, dep = 'libre-office') {
  return mkJob({
    id,
    state: 'paused',
    file_name: `${id}.docx`,
    error: { code: 'dependency_missing', reason: dep, message: 'missing' },
  });
}

const office: DependencyReport = {
  id: 'libre-office',
  name: 'LibreOffice',
  applicable: true,
  found: true,
  path: null,
  found_in: null,
  blocks: [],
  remediation: null,
  install: [],
};

function installed(): InstallEnd {
  const outcome: InstallOutcome = {
    status: 'installed',
    exit_code: 0,
    elevated: false,
    report: office,
  };
  return { id: 'libre-office', name: 'LibreOffice', seq: 1, outcome };
}

afterEach(() => {
  vi.restoreAllMocks();
});

describe('resumeJobsWaitingOn', () => {
  it('FR-058: resumes only the jobs paused on that dependency, through resume_job', async () => {
    const resumeJob = vi.fn(() => Promise.resolve());
    const tally = await resumeJobsWaitingOn('libre-office', {
      queueStatus: () =>
        Promise.resolve({
          queue_paused: false,
          jobs: [
            waitingJob('a'),
            waitingJob('b'),
            waitingJob('c', 'heif-codecs'),
            mkJob({ id: 'd', state: 'paused', file_name: 'd.pdf' }),
            mkJob({ id: 'e', state: 'queued', file_name: 'e.pdf' }),
          ],
        } as never),
      resumeJob,
    });
    expect(resumeJob.mock.calls).toEqual([['a'], ['b']]);
    expect(tally).toEqual({ resumed: 2, failed: 0, checked: true });
  });

  it('FR-058: a job that moved on is not a failure; a refusal is', async () => {
    const resumeJob = vi.fn((id: string) => {
      if (id === 'a')
        return Promise.reject(
          new IpcFailure({ code: 'InvalidState', message: 'not paused' }),
        );
      if (id === 'b') return Promise.reject(new Error('disk full'));
      return Promise.resolve();
    });
    const tally = await resumeJobsWaitingOn('libre-office', {
      queueStatus: () =>
        Promise.resolve({
          queue_paused: false,
          jobs: [waitingJob('a'), waitingJob('b'), waitingJob('c')],
        } as never),
      resumeJob,
    });
    expect(tally).toEqual({ resumed: 1, failed: 1, checked: true });
  });
});

describe('announceInstallEnd', () => {
  it('FR-058: says how many queue jobs the store resumed (EN and ES)', () => {
    const success = vi.spyOn(toast, 'success').mockReturnValue('id');
    const end = {
      ...installed(),
      resumed: { resumed: 3, failed: 0, checked: true },
    };
    announceInstallEnd(tEn, end, false, true);
    announceInstallEnd(tEs, end, false, true);
    expect(success.mock.calls[0]).toEqual([
      'Install LibreOffice',
      {
        description:
          'LibreOffice is installed. The 3 jobs in the queue that needed it are resuming.',
      },
    ]);
    expect(success.mock.calls[1]![1]).toEqual({
      description:
        'LibreOffice está instalado. Los 3 trabajos de la cola que lo necesitaban se reanudan.',
    });
  });

  it('FR-058: an install found after a reload that unblocked nothing does not claim files can print now', () => {
    const success = vi.spyOn(toast, 'success').mockReturnValue('id');
    announceInstallEnd(
      tEn,
      { ...installed(), resumed: { resumed: 0, failed: 0, checked: true } },
      false,
      true,
    );
    expect(success).toHaveBeenCalledWith('Install LibreOffice', {
      description: 'LibreOffice is installed.',
    });
  });

  it('FR-058: jobs that could not be resumed, or a queue that could not be read, are a warning that says so', () => {
    const warning = vi.spyOn(toast, 'warning').mockReturnValue('id');
    announceInstallEnd(
      tEn,
      { ...installed(), resumed: { resumed: 1, failed: 1, checked: true } },
      false,
      true,
    );
    announceInstallEnd(
      tEn,
      { ...installed(), resumed: { resumed: 0, failed: 0, checked: false } },
      false,
      true,
    );
    expect(warning.mock.calls[0]![1]).toEqual({
      description:
        'LibreOffice is installed, but the job in the queue that needed it could not be resumed. Resume it from the queue.',
    });
    expect(warning.mock.calls[1]![1]).toEqual({
      description:
        'LibreOffice is installed, but Pluryprint could not read the queue to resume the jobs that needed it. Resume them from the queue.',
    });
  });

  it('FR-058: an end nobody saw, with the dependency found by a new probe, is announced as installed', () => {
    const success = vi.spyOn(toast, 'success').mockReturnValue('id');
    const error = vi.spyOn(toast, 'error').mockReturnValue('id');
    announceInstallEnd(
      tEn,
      {
        id: 'libre-office',
        name: 'LibreOffice',
        seq: 2,
        failure: 'unknown',
        resumed: { resumed: 1, failed: 0, checked: true },
      },
      false,
      true,
    );
    expect(error).not.toHaveBeenCalled();
    expect(success).toHaveBeenCalledWith('Install LibreOffice', {
      description:
        'LibreOffice is installed. The job in the queue that needed it is resuming.',
    });
  });

  it('FR-058: an "opened" end no screen asked about names where Pluryprint looks again (EN and ES)', () => {
    const warning = vi.spyOn(toast, 'warning').mockReturnValue('id');
    const opened: InstallEnd = {
      id: 'libre-office',
      name: 'LibreOffice',
      seq: 3,
      outcome: {
        status: 'opened',
        exit_code: null,
        elevated: false,
        report: { ...office, found: false },
      },
    };
    announceInstallEnd(tEn, opened, false, true);
    announceInstallEnd(tEs, opened, false, true);
    announceInstallEnd(tEn, opened);
    expect(warning.mock.calls[0]![1]).toEqual({
      description:
        'Finish the installation in the window that opened. Pluryprint looks for LibreOffice again when you open Print Job, and the jobs in the queue waiting for it resume when you come back to Pluryprint with the Queue open.',
    });
    expect(warning.mock.calls[1]![1]).toEqual({
      description:
        'Termina la instalación en la ventana que se ha abierto. Pluryprint vuelve a buscar LibreOffice cuando abres Trabajo de impresión, y los trabajos de la cola que lo esperan se reanudan cuando vuelves a Pluryprint con la Cola abierta.',
    });
    expect(warning.mock.calls[2]![1]).toEqual({
      description:
        'Finish the installation in the window that opened. Pluryprint checks again when you come back to Print Job.',
    });
  });
});
