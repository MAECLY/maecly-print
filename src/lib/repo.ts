/**
 * Canonical project URLs. Centralized so the About screen, issue reporter, and
 * update-check links all point at the same repository without re-typing it.
 * Every URL here is under `github.com/maecly`, which the Rust `open_external`
 * allow-list permits.
 */

export const REPO_URL = 'https://github.com/maecly/maeclyprint';
export const RELEASES_URL = REPO_URL + '/releases';
export const LICENSE_URL = REPO_URL + '/blob/main/LICENSE';
export const DOCS_URL = REPO_URL + '/tree/main/docs';
