/**
 * The page a mail message's HTML is shown in. The Host has sanitized the HTML
 * already (`src-tauri/src/mail/sanitize.rs`); this is the second wall. The page
 * goes into an iframe sandboxed with no scripts and no same-origin access, so
 * it can't reach the app, its storage or the Host. Its own content policy
 * also loads nothing: no scripts, frames, fonts or forms, and images only from
 * the message itself unless the user asked for remote ones.
 *
 * Mail is written for a white page, so it gets one, in light and dark themes
 * alike, as other mail apps do.
 */

/** The sandbox the frame runs in. Popups are allowed only so that a link can
 * ask for a new window, which the app turns into the OS browser (`lib.rs`). */
export const FRAME_SANDBOX = "allow-popups allow-popups-to-escape-sandbox";

export function policy(images: boolean): string {
  return [
    "default-src 'none'",
    `img-src data:${images ? " https: http:" : ""}`,
    "style-src 'unsafe-inline'",
    "form-action 'none'",
    "base-uri 'none'",
    "frame-src 'none'",
  ].join("; ");
}

const BASE_STYLE = `
html { background: #fff; color: #1f2328; color-scheme: light; }
body { margin: 0; padding: 18px 22px; font: 14px/1.55 -apple-system, BlinkMacSystemFont, "Segoe UI", system-ui, sans-serif; overflow-wrap: anywhere; }
img { max-width: 100%; height: auto; }
table { max-width: 100%; }
a { color: #0969da; }
pre { white-space: pre-wrap; }
blockquote { margin: 0 0 0 0.4em; padding-left: 0.9em; border-left: 3px solid #d0d7de; color: #57606a; }
`;

/** The frame's whole document, around HTML that is already sanitized. */
export function frameDocument(html: string, images: boolean): string {
  return (
    "<!doctype html><html><head><meta charset=\"utf-8\">" +
    `<meta http-equiv="Content-Security-Policy" content="${policy(images)}">` +
    '<meta name="referrer" content="no-referrer">' +
    `<style>${BASE_STYLE}</style></head><body>${html}</body></html>`
  );
}
