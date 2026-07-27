import logoUrl from "./assets/graphowl-logo.png";

/** The header mark: an inline SVG owl-in-a-graph-node, drawn to the brand's
 *  navy/teal pair. The raster lockup is 1.6MB at source and must never reach
 *  the bundle at header size. */
export function GraphOwlMark({ size = 26, dark = false }: { size?: number; dark?: boolean }) {
  // The disc is navy on light and teal-tinted on dark — a navy disc on a navy
  // header is invisible, which is exactly what happened first time.
  const disc = dark ? "#0C3B47" : "#041D50";
  const node = dark ? "#2BC4C9" : "#03A7AD";
  return (
    <svg width={size} height={size} viewBox="0 0 32 32" role="img" aria-label="GraphOwl">
      <circle cx="16" cy="16" r="15" fill={disc} />
      {/* graph edges */}
      <g stroke="#27C4D4" strokeWidth="1.4" opacity={dark ? 0.9 : 0.75}>
        <line x1="7" y1="10" x2="16" y2="20" />
        <line x1="25" y1="10" x2="16" y2="20" />
        <line x1="7" y1="10" x2="25" y2="10" />
      </g>
      <g fill={node}>
        <circle cx="7" cy="10" r="2.4" />
        <circle cx="25" cy="10" r="2.4" />
        <circle cx="16" cy="20" r="2.4" />
      </g>
      {/* the owl's two eyes, which is what makes the node read as a face */}
      <circle cx="7" cy="10" r="0.9" fill={disc} />
      <circle cx="25" cy="10" r="0.9" fill={disc} />
    </svg>
  );
}

/** The full lockup — owl, wordmark, tagline. Used where there is room for it
 *  and a first-run user benefits from seeing what the product is called. */
export function GraphOwlLockup({ width = 260 }: { width?: number }) {
  return (
    <img
      src={logoUrl}
      width={width}
      alt="GraphOwl — see the connections, know the truth"
      style={{ maxWidth: "100%", height: "auto" }}
    />
  );
}

/** Connector marks, drawn rather than taken from the reference's bundled
 *  service icons — those are third-party trademarks under its licence. */
export function PostgresMark({ size = 28 }: { size?: number }) {
  return (
    <svg width={size} height={size} viewBox="0 0 32 32" role="img" aria-label="PostgreSQL">
      <circle cx="16" cy="16" r="15" fill="#336791" />
      <ellipse cx="16" cy="10.5" rx="8.5" ry="3.4" fill="#fff" opacity="0.92" />
      <path d="M7.5 10.5v11c0 1.9 3.8 3.4 8.5 3.4s8.5-1.5 8.5-3.4v-11"
            stroke="#fff" strokeWidth="2.2" fill="none" opacity="0.92" />
      <path d="M7.5 16c0 1.9 3.8 3.4 8.5 3.4s8.5-1.5 8.5-3.4"
            stroke="#fff" strokeWidth="2.2" fill="none" opacity="0.92" />
    </svg>
  );
}

export function GenericSourceMark({ size = 28, tint = "#6B7BA3" }: { size?: number; tint?: string }) {
  return (
    <svg width={size} height={size} viewBox="0 0 32 32" role="img" aria-label="Source">
      <rect x="1" y="1" width="30" height="30" rx="8" fill={tint} opacity="0.16" />
      <ellipse cx="16" cy="11" rx="8" ry="3.2" fill={tint} />
      <path d="M8 11v10c0 1.8 3.6 3.2 8 3.2s8-1.4 8-3.2V11"
            stroke={tint} strokeWidth="2.2" fill="none" />
    </svg>
  );
}
