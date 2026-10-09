/**
 * Format a size in kibibytes into a human-friendly string.
 * Ported from `claude design/maeclyprint/project/data.jsx:57`.
 */
export function fmtKb(kb: number): string {
  if (kb < 1024) return `${kb} KB`;
  return `${(kb / 1024).toFixed(1)} MB`;
}

/**
 * When a job finished, in the app's locale: the time alone when it was today,
 * the date and time otherwise — recent activity survives restarts, so a bare
 * "14:03:12" from yesterday would read as this afternoon. `null` for an
 * unparseable stamp.
 */
export function formatFinished(
  iso: string,
  locale: string,
  now: Date = new Date(),
): string | null {
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return null;
  const sameDay =
    d.getFullYear() === now.getFullYear() &&
    d.getMonth() === now.getMonth() &&
    d.getDate() === now.getDate();
  return new Intl.DateTimeFormat(
    locale,
    sameDay
      ? { hour: '2-digit', minute: '2-digit', second: '2-digit' }
      : { dateStyle: 'medium', timeStyle: 'short' },
  ).format(d);
}

/** Which shell a copied command line is written for. */
export type ShellFlavor = 'posix' | 'powershell';

/** Characters that need no quoting in either shell. */
const SHELL_SAFE = /^[A-Za-z0-9_\-./=:@%+,]+$/;

/**
 * Quote one argument so the shell hands it to the program verbatim.
 *
 * Single quotes in both flavours, because they are the only ones that turn
 * expansion off entirely (`$HOME`, backticks, globs): POSIX escapes an inner
 * `'` as `'\''`, PowerShell doubles it. A shell-safe word is left bare so the
 * common case stays readable.
 */
export function quoteShellArg(arg: string, flavor: ShellFlavor): string {
  if (SHELL_SAFE.test(arg)) return arg;
  const inner =
    flavor === 'powershell'
      ? arg.replace(/'/g, "''")
      : arg.replace(/'/g, `'\\''`);
  return `'${inner}'`;
}

/**
 * The `print` command the Home screen offers to copy (FR-008).
 *
 * It names the running executable by its full path because nothing puts
 * `maeclyprint` on `PATH` yet (FR-006): a bare `maeclyprint` pasted into a
 * terminal would only answer "command not found". On Windows the line is for
 * PowerShell, where a quoted program path needs the `&` call operator to run.
 * The glob is quoted on purpose — the CLI expands patterns itself, and a shell
 * that expanded it first would hand over a different argument list.
 */
export function cliPrintCommand({
  executable,
  os,
  profile,
}: {
  executable: string | null | undefined;
  os: string | undefined;
  profile: string;
}): string {
  const flavor: ShellFlavor = os === 'windows' ? 'powershell' : 'posix';
  let program = 'maeclyprint';
  if (executable) {
    program = quoteShellArg(executable, flavor);
    if (flavor === 'powershell' && program.startsWith("'")) {
      program = `& ${program}`;
    }
  }
  return `${program} print --files ${quoteShellArg('*.pdf', flavor)} --profile ${quoteShellArg(profile, flavor)}`;
}

/**
 * Pluralize a count by appending "s" when not 1.
 */
export function pluralize(n: number, singular: string, plural?: string) {
  return n === 1 ? singular : (plural ?? `${singular}s`);
}
