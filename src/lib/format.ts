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

/** Characters that need no quoting in a POSIX shell. */
const POSIX_SAFE = /^[A-Za-z0-9_\-./=:@%+,]+$/;

/**
 * Characters inert in PowerShell argument mode. Narrower than POSIX on
 * purpose: `@` splats, `,` builds an array, `%` and `+` can start expressions.
 */
const POWERSHELL_SAFE = /^[A-Za-z0-9_\-./=:\\]+$/;

/**
 * Every character PowerShell accepts as a single-quote delimiter: the ASCII
 * apostrophe and the typographic U+2018, U+2019, U+201A and U+201B. Matches
 * `POWERSHELL_QUOTES` in `src-tauri/src/engine/print/mod.rs`.
 */
const POWERSHELL_QUOTES = /['‘’‚‛]/g;

/**
 * Quote one argument so the shell hands it to the program verbatim.
 *
 * Single quotes in both flavours, because they are the only ones that turn
 * expansion off entirely (`$HOME`, backticks, globs): POSIX escapes an inner
 * `'` as `'\''`, PowerShell doubles every quote character it recognises,
 * typographic ones included (otherwise `’; calc; ’` closes the string). A
 * shell-safe word is left bare so the common case stays readable.
 */
export function quoteShellArg(arg: string, flavor: ShellFlavor): string {
  if (flavor === 'powershell') {
    if (POWERSHELL_SAFE.test(arg)) return arg;
    return `'${arg.replace(POWERSHELL_QUOTES, (q) => q + q)}'`;
  }
  if (POSIX_SAFE.test(arg)) return arg;
  return `'${arg.replace(/'/g, `'\\''`)}'`;
}

/** The console front end scripts run on Windows (`src/bin/pluryprint-cli.rs`). */
const CLI_PROGRAM_WINDOWS = 'pluryprint-cli';

/**
 * The command-line program's name on `os` (`'windows'`, `'macos'`, …):
 * `pluryprint-cli` on Windows, where the app itself is a GUI program a shell
 * does not wait for; the app's own `pluryprint` elsewhere.
 */
export function cliProgram(os: string | undefined): string {
  return os === 'windows' ? CLI_PROGRAM_WINDOWS : 'pluryprint';
}

/**
 * The `print` command the Home screen offers to copy (FR-008).
 *
 * It names the running executable by its full path because nothing puts
 * `pluryprint` on `PATH` yet (FR-006): a bare `pluryprint` pasted into a
 * terminal would only answer "command not found". On Windows the line is for
 * PowerShell, where a quoted program path needs the `&` call operator to run,
 * and it names the console-subsystem `pluryprint-cli.exe` installed next to
 * the app: `pluryprint.exe` is a GUI program, so PowerShell would neither wait
 * for it nor show its output or exit code (FR-009).
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
  let program = cliProgram(os);
  if (executable) {
    const path =
      os === 'windows'
        ? executable.replace(/[^\\/]*$/, `${CLI_PROGRAM_WINDOWS}.exe`)
        : executable;
    program = quoteShellArg(path, flavor);
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

/** Split a path into its directory segments (both `/` and `\` separators). */
function parentSegments(path: string): string[] {
  const parts = path.split(/[\\/]+/).filter((p) => p !== '');
  parts.pop(); // the file itself
  return parts;
}

/**
 * For files whose names collide, the shortest tail of their parent folders
 * that tells them apart (`Invoices`, `2025/Invoices`…); files with a unique
 * name get no entry. Names compare case-insensitively, as on Windows and the
 * default macOS volume. Keys are the input paths.
 */
export function distinguishingFolders(
  paths: readonly string[],
): Map<string, string> {
  const byName = new Map<string, string[]>();
  for (const p of paths) {
    const name = (p.split(/[\\/]/).pop() ?? p).toLowerCase();
    byName.set(name, [...(byName.get(name) ?? []), p]);
  }
  const out = new Map<string, string>();
  for (const group of byName.values()) {
    const unique = [...new Set(group)];
    if (unique.length < 2) continue;
    const segs = unique.map(parentSegments);
    const longest = Math.max(...segs.map((s) => s.length), 1);
    for (let depth = 1; depth <= longest; depth++) {
      const tails = segs.map((s) => s.slice(-depth).join('/'));
      const distinct =
        new Set(tails.map((x) => x.toLowerCase())).size === tails.length;
      if (distinct || depth === longest) {
        unique.forEach((p, i) => out.set(p, tails[i] || '/'));
        break;
      }
    }
  }
  return out;
}
