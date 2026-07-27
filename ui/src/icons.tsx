/** Simple connector marks drawn as SVG.
 *
 *  Deliberately not the reference's `service-icon-*` assets: those are
 *  third-party trademarks bundled under its licence, and copying them is the
 *  one part of a UI that is unambiguously someone else's property. These are
 *  recognisable-by-shape rather than by logo. */
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

export function GenericSourceMark({ size = 28, tint = "#8c93a4" }: { size?: number; tint?: string }) {
  return (
    <svg width={size} height={size} viewBox="0 0 32 32" role="img" aria-label="Source">
      <rect x="1" y="1" width="30" height="30" rx="8" fill={tint} opacity="0.18" />
      <ellipse cx="16" cy="11" rx="8" ry="3.2" fill={tint} />
      <path d="M8 11v10c0 1.8 3.6 3.2 8 3.2s8-1.4 8-3.2V11"
            stroke={tint} strokeWidth="2.2" fill="none" />
    </svg>
  );
}
