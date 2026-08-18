import logoUrl from "./assets/graphowl-logo.png";

/** The header mark: a colourful owl mascot with glasses, graph nodes, and a
 *  flowing cape — drawn to the brand's navy/teal/cyan/gold palette. */
export function GraphOwlMark({ size = 36, dark = false }: { size?: number; dark?: boolean }) {
  return (
    <svg width={size} height={size} viewBox="0 0 64 64" role="img" aria-label="GraphOwl">
      {/* background disc */}
      <circle cx="32" cy="32" r="31" fill={dark ? "#0C3B47" : "#041D50"} />

      {/* graph edges behind the owl */}
      <g stroke="#27C4D4" strokeWidth="1.2" opacity="0.45">
        <line x1="10" y1="14" x2="22" y2="10" />
        <line x1="22" y1="10" x2="42" y2="10" />
        <line x1="42" y1="10" x2="54" y2="14" />
        <line x1="22" y1="10" x2="32" y2="5" />
        <line x1="42" y1="10" x2="50" y2="5" />
        <line x1="10" y1="14" x2="32" y2="5" />
      </g>
      {/* graph nodes */}
      <g>
        <circle cx="10" cy="14" r="2" fill="#6C74D8" opacity="0.6" />
        <circle cx="22" cy="10" r="1.8" fill="#27C4D4" opacity="0.5" />
        <circle cx="42" cy="10" r="1.8" fill="#27C4D4" opacity="0.5" />
        <circle cx="54" cy="14" r="2" fill="#6C74D8" opacity="0.6" />
        <circle cx="32" cy="5" r="1.5" fill="#03A7AD" opacity="0.5" />
        <circle cx="50" cy="5" r="1.3" fill="#27C4D4" opacity="0.4" />
      </g>

      {/* cape — royal blue with cyan trim */}
      <path
        d="M20 30 Q16 42 18 52 L32 56 L46 52 Q48 42 44 30 Z"
        fill="#2C64C8"
        opacity="0.85"
      />
      <path
        d="M20 30 Q16 42 18 52 L32 56 L46 52 Q48 42 44 30"
        fill="none"
        stroke="#27C4D4"
        strokeWidth="1.2"
        opacity="0.6"
      />

      {/* owl body */}
      <ellipse cx="32" cy="38" rx="14" ry="15" fill="#2C64C8" />

      {/* face disc — cream */}
      <ellipse cx="32" cy="34" rx="11" ry="10" fill="#F5F0E6" />

      {/* ear tufts */}
      <path d="M21 28 L18 20 L25 27 Z" fill="#1B4FA0" />
      <path d="M43 28 L46 20 L39 27 Z" fill="#1B4FA0" />

      {/* eyes — large, expressive */}
      <circle cx="26" cy="32" r="4.5" fill="white" />
      <circle cx="38" cy="32" r="4.5" fill="white" />
      <circle cx="26.5" cy="32" r="2.8" fill="#1A1A2E" />
      <circle cx="38.5" cy="32" r="2.8" fill="#1A1A2E" />
      <circle cx="27.5" cy="30.5" r="1" fill="white" />
      <circle cx="39.5" cy="30.5" r="1" fill="white" />

      {/* glasses — round black frames */}
      <circle cx="26" cy="32" r="5.5" fill="none" stroke="#1A1A2E" strokeWidth="1.8" />
      <circle cx="38" cy="32" r="5.5" fill="none" stroke="#1A1A2E" strokeWidth="1.8" />
      <line x1="31.5" y1="31" x2="32.5" y2="31" stroke="#1A1A2E" strokeWidth="1.8" strokeLinecap="round" />

      {/* beak — gold */}
      <path d="M29 36 L32 40 L35 36 Z" fill="#E6B14A" />

      {/* left wing raised up */}
      <path
        d="M18 36 Q12 28 10 20 Q14 26 18 30 Z"
        fill="#2C64C8"
        stroke="#27C4D4"
        strokeWidth="0.6"
        opacity="0.8"
      />

      {/* right wing holding tablet */}
      <path
        d="M46 36 Q52 30 52 24"
        fill="none"
        stroke="#2C64C8"
        strokeWidth="2.5"
        strokeLinecap="round"
      />
      {/* tablet */}
      <rect x="48" y="20" width="7" height="9" rx="1" fill="#E8EEF8" stroke="#6B7BA3" strokeWidth="0.7" />
      {/* graph on tablet screen */}
      <circle cx="50.5" cy="23" r="0.8" fill="#27C4D4" />
      <circle cx="53.5" cy="23" r="0.8" fill="#03A7AD" />
      <circle cx="52" cy="26" r="0.8" fill="#6C74D8" />
      <line x1="50.5" y1="23" x2="53.5" y2="23" stroke="#27C4D4" strokeWidth="0.5" />
      <line x1="50.5" y1="23" x2="52" y2="26" stroke="#03A7AD" strokeWidth="0.5" />
      <line x1="53.5" y1="23" x2="52" y2="26" stroke="#6C74D8" strokeWidth="0.5" />

      {/* talons */}
      <path d="M27 52 L25 56 M29 52 L29 56 M31 52 L33 56" stroke="#E6B14A" strokeWidth="1.2" strokeLinecap="round" fill="none" />

      {/* medallion on chest — graph icon */}
      <circle cx="32" cy="42" r="3" fill="#041D50" stroke="#27C4D4" strokeWidth="0.7" />
      <circle cx="30.5" cy="41.5" r="0.7" fill="#27C4D4" />
      <circle cx="33.5" cy="41.5" r="0.7" fill="#03A7AD" />
      <circle cx="32" cy="43.5" r="0.7" fill="#6C74D8" />
      <line x1="30.5" y1="41.5" x2="33.5" y2="41.5" stroke="#27C4D4" strokeWidth="0.4" />
      <line x1="30.5" y1="41.5" x2="32" y2="43.5" stroke="#03A7AD" strokeWidth="0.4" />
      <line x1="33.5" y1="41.5" x2="32" y2="43.5" stroke="#6C74D8" strokeWidth="0.4" />
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

/** The "O" in GRAPHOWL — per brand spec.
 *  viewBox 200×200: outer circle stroke 16px, brow arch 14px stroke,
 *  two 30px-diameter eyes (42px apart), solid teal pupils,
 *  rounded inverted-teardrop beak 24px tall. All circles + arcs + béziers. */
export function GraphOwlIcon({ size = 48, color = "#18C4C8" }: { size?: number; color?: string }) {
  return (
    <svg width={size} height={size} viewBox="0 0 200 200" fill="none" role="img" aria-label="GraphOwl">
      {/* Outer circle — the "O" ring */}
      <circle cx="100" cy="100" r="84" stroke={color} strokeWidth="16" />

      {/* Eyebrow arch — smooth curve from 25% to 75% */}
      <path
        d="M 50 82 Q 100 52 150 82"
        stroke={color}
        strokeWidth="14"
        strokeLinecap="round"
        fill="none"
      />

      {/* Left eye — 30px diameter, solid fill */}
      <circle cx="79" cy="105" r="15" fill={color} />

      {/* Right eye — 30px diameter, solid fill */}
      <circle cx="121" cy="105" r="15" fill={color} />

      {/* Beak — rounded inverted teardrop, 24px tall, no sharp edges */}
      <path
        d="M 90 134 Q 90 124 100 124 Q 110 124 110 134 Q 110 152 100 158 Q 90 152 90 134 Z"
        fill={color}
      />
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
