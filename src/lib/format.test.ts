import { describe, expect, it } from 'vitest';

import { cliPrintCommand, quoteShellArg } from './format';

describe('quoteShellArg', () => {
  it('leaves shell-safe words alone', () => {
    expect(quoteShellArg('Quick', 'posix')).toBe('Quick');
    expect(quoteShellArg('builtin-quick', 'powershell')).toBe('builtin-quick');
  });

  it('FR-008: single-quotes anything a POSIX shell would split or expand', () => {
    expect(quoteShellArg('Costs $HOME', 'posix')).toBe("'Costs $HOME'");
    expect(quoteShellArg('*.pdf', 'posix')).toBe("'*.pdf'");
    // A quote inside the value closes, escapes and reopens.
    expect(quoteShellArg(`Office 'A'`, 'posix')).toBe(`'Office '\\''A'\\'''`);
    expect(quoteShellArg('Office "A"', 'posix')).toBe(`'Office "A"'`);
  });

  it('FR-008: doubles single quotes for PowerShell', () => {
    expect(quoteShellArg(`Bob's`, 'powershell')).toBe(`'Bob''s'`);
    expect(quoteShellArg('Costs $HOME', 'powershell')).toBe(`'Costs $HOME'`);
  });
});

describe('cliPrintCommand', () => {
  it('FR-006/FR-008: names the real executable, because it is not on PATH', () => {
    expect(
      cliPrintCommand({
        executable: '/Applications/MaeclyPrint.app/Contents/MacOS/maeclyprint',
        os: 'macos',
        profile: 'Quick',
      }),
    ).toBe(
      "/Applications/MaeclyPrint.app/Contents/MacOS/maeclyprint print --files '*.pdf' --profile Quick",
    );
  });

  it('FR-008: a Windows path with spaces is invoked with the PowerShell call operator', () => {
    expect(
      cliPrintCommand({
        executable:
          'C:\\Users\\Ana\\AppData\\Local\\Programs\\Maecly Print\\maeclyprint.exe',
        os: 'windows',
        profile: `Ana's "draft"`,
      }),
    ).toBe(
      `& 'C:\\Users\\Ana\\AppData\\Local\\Programs\\Maecly Print\\maeclyprint.exe' print --files '*.pdf' --profile 'Ana''s "draft"'`,
    );
  });

  it('falls back to the bare command name when the executable is unknown', () => {
    expect(
      cliPrintCommand({ executable: null, os: undefined, profile: 'Quick' }),
    ).toBe("maeclyprint print --files '*.pdf' --profile Quick");
  });
});
