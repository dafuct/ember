import type { SVGProps } from "react";

type EmberMarkProps = { size?: number | string } & SVGProps<SVGSVGElement>;

/**
 * Ember brand mark — a tilted envelope trailing three motion lines ("mail, sent").
 * Stroked in currentColor at lucide weight so it sits natively beside lucide-react
 * icons and inherits its color from `.brand-icon` / `.rail-brand`. This is the same
 * silhouette as the macOS app icon (src-tauri/icons/source/ember-icon.svg).
 */
export function EmberMark({ size = 24, ...props }: EmberMarkProps) {
  return (
    <svg
      xmlns="http://www.w3.org/2000/svg"
      width={size}
      height={size}
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth={2}
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
      {...props}
    >
      <path d="M2 8h4" />
      <path d="M1 12h5.4" />
      <path d="M2.4 16h3.6" />
      <g transform="rotate(-14 14.9 12)">
        <rect x="8.9" y="7.4" width="12" height="9.2" rx="2.2" />
        <path d="M20.7 9.1l-4.9 3.6a1.3 1.3 0 0 1-1.2 0L9.1 9.1" />
      </g>
    </svg>
  );
}
