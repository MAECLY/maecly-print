/**
 * Extensions the print engine accepts, in the engine's own order.
 *
 * This mirrors `SUPPORTED_EXTS` in `src-tauri/src/engine/mod.rs`, which is the
 * only authority — `HomeScreen.test.tsx` reads that Rust file and fails if the
 * two ever drift, so neither the drop zone copy nor an OS file-picker filter
 * can offer a format the engine rejects (or hide one it prints).
 *
 * Grouped by converter (see `docs/03-requirements/supported-formats.md`):
 * PDF passthrough; raster images and HEIC/HEIF; Office and OpenDocument, TXT
 * and RTF through LibreOffice; HTML through a headless browser; Markdown
 * through Pandoc and the browser. A format whose converter is missing is
 * still accepted — its job pauses with an install hint (FR-058).
 */
export const PRINTABLE_EXTENSIONS = [
  'pdf',
  'jpg',
  'jpeg',
  'png',
  'tif',
  'tiff',
  'bmp',
  'webp',
  'heic',
  'heif',
  'docx',
  'doc',
  'xlsx',
  'xls',
  'pptx',
  'ppt',
  'odt',
  'ods',
  'odp',
  'rtf',
  'txt',
  'html',
  'htm',
  'md',
  'markdown',
] as const;

export type PrintableExtension = (typeof PRINTABLE_EXTENSIONS)[number];
