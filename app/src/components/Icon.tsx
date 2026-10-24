import type { SVGProps } from "react";

const paths = {
  player: (
    <>
      <rect x="6" y="2.75" width="12" height="18.5" rx="2.25" />
      <rect x="8.25" y="5" width="7.5" height="9" rx="0.75" />
      <circle cx="12" cy="17.6" r="1.35" />
    </>
  ),
  grid: (
    <>
      <rect x="3.75" y="3.75" width="7" height="7" rx="1.25" />
      <rect x="13.25" y="3.75" width="7" height="7" rx="1.25" />
      <rect x="3.75" y="13.25" width="7" height="7" rx="1.25" />
      <rect x="13.25" y="13.25" width="7" height="7" rx="1.25" />
    </>
  ),
  dial: (
    <>
      <circle cx="12" cy="12" r="8.25" />
      <circle cx="12" cy="12" r="3.25" />
      <path d="M12 3.75v3M12 17.25v3M3.75 12h3M17.25 12h3" />
    </>
  ),
  refresh: (
    <>
      <path d="M19.25 12a7.25 7.25 0 1 1-2.12-5.13" />
      <path d="M19.5 4.25v4.5H15" />
    </>
  ),
  chevron: <path d="M9.5 6.5 15 12l-5.5 5.5" />,
  back: (
    <>
      <path d="M8.75 13.75 4.5 9.5l4.25-4.25" />
      <path d="M4.75 9.5h9.5a5 5 0 0 1 0 10h-3.5" />
    </>
  ),
  computer: (
    <>
      <rect x="4.25" y="5" width="15.5" height="10.75" rx="1.5" />
      <path d="M2.75 19h18.5" />
    </>
  ),
  receive: (
    <>
      <path d="M12 3.75v10.5" />
      <path d="M7.75 10 12 14.25 16.25 10" />
      <path d="M4.25 15.5v2.75c0 1.1.9 2 2 2h11.5c1.1 0 2-.9 2-2V15.5" />
    </>
  ),
  sort: <path d="M4.5 7h15M7 12h10M9.5 17h5" />,
  usb: (
    <>
      <path d="M12 20.5V5" />
      <path d="M9.5 7.25 12 3.5l2.5 3.75" />
      <path d="M12 16.25 7.25 13.5V11" />
      <circle cx="7.25" cy="9.75" r="1.25" />
      <path d="M12 13.75 16.75 11V9" />
      <rect x="15.75" y="6.75" width="2" height="2.25" rx="0.4" />
      <circle cx="12" cy="20.25" r="0.6" />
    </>
  ),
  battery: (
    <>
      <rect x="3.25" y="7.25" width="15.5" height="9.5" rx="2" />
      <path d="M21 10.5v3" />
      <path d="M6.25 10.25v3.5M9.25 10.25v3.5" />
    </>
  ),
  card: (
    <>
      <path d="M7.25 3.25h7.5l4 4v12.5a1 1 0 0 1-1 1H7.25a1 1 0 0 1-1-1V4.25a1 1 0 0 1 1-1Z" />
      <path d="M9.5 3.5v3M12 3.5v3M14.5 3.5v3" />
    </>
  ),
  headphones: (
    <>
      <path d="M4.25 15v-3a7.75 7.75 0 0 1 15.5 0v3" />
      <rect x="3.25" y="14" width="4" height="6.25" rx="1.25" />
      <rect x="16.75" y="14" width="4" height="6.25" rx="1.25" />
    </>
  ),
  chip: (
    <>
      <rect x="6.25" y="6.25" width="11.5" height="11.5" rx="1.5" />
      <rect x="9.5" y="9.5" width="5" height="5" rx="0.5" />
      <path d="M9.5 3.5v2.75M14.5 3.5v2.75M9.5 17.75v2.75M14.5 17.75v2.75M3.5 9.5h2.75M3.5 14.5h2.75M17.75 9.5h2.75M17.75 14.5h2.75" />
    </>
  ),
  alert: (
    <>
      <path d="M12 3.75 21 19.5H3Z" />
      <path d="M12 9.75v4.5" />
      <circle cx="12" cy="16.9" r="0.55" />
    </>
  ),
  eye: (
    <>
      <path d="M2.75 12S6.25 5.75 12 5.75 21.25 12 21.25 12 17.75 18.25 12 18.25 2.75 12 2.75 12Z" />
      <circle cx="12" cy="12" r="2.75" />
    </>
  ),
  eyeOff: (
    <>
      <path d="M4 4l16 16" />
      <path d="M9.9 6.05A9.6 9.6 0 0 1 12 5.75c5.75 0 9.25 6.25 9.25 6.25a17 17 0 0 1-2.7 3.4M6.55 7.6A16.5 16.5 0 0 0 2.75 12S6.25 18.25 12 18.25c1.6 0 3.02-.48 4.25-1.17" />
      <path d="M10.05 10.1a2.75 2.75 0 0 0 3.87 3.87" />
    </>
  ),
  plug: (
    <>
      <path d="M9 3.5v4M15 3.5v4" />
      <path d="M6.75 7.5h10.5v3.25a5.25 5.25 0 0 1-10.5 0Z" />
      <path d="M12 16v4.5" />
    </>
  ),
  link: (
    <>
      <path d="M10 14a4 4 0 0 0 5.66 0l3.17-3.17a4 4 0 0 0-5.66-5.66l-1.1 1.1" />
      <path d="M14 10a4 4 0 0 0-5.66 0L5.17 13.17a4 4 0 0 0 5.66 5.66l1.1-1.1" />
    </>
  ),
  shield: (
    <>
      <path d="M12 3.25 19.25 6v5.5c0 4.3-3.05 7.85-7.25 9.25-4.2-1.4-7.25-4.95-7.25-9.25V6Z" />
      <path d="m9 12 2.1 2.1L15.25 10" />
    </>
  ),
  lockOpen: (
    <>
      <rect x="5.25" y="10.75" width="13.5" height="9.5" rx="1.75" />
      <path d="M8.25 10.75V7.5a3.75 3.75 0 0 1 7.2-1.45" />
    </>
  ),
  check: <path d="m5.5 12.5 4.25 4.25 8.75-9.5" />,
  library: (
    <>
      <rect x="3.75" y="4.75" width="4" height="14.5" rx="0.75" />
      <path d="M11 19.25V4.75" />
      <circle cx="16.25" cy="12" r="5" />
      <circle cx="16.25" cy="12" r="1.1" />
    </>
  ),
  folder: (
    <path d="M3.75 7.25a1.5 1.5 0 0 1 1.5-1.5h4l2 2.25h7.5a1.5 1.5 0 0 1 1.5 1.5v8.25a1.5 1.5 0 0 1-1.5 1.5H5.25a1.5 1.5 0 0 1-1.5-1.5Z" />
  ),
  plus: <path d="M12 5.25v13.5M5.25 12h13.5" />,
  close: <path d="m6.5 6.5 11 11M17.5 6.5l-11 11" />,
  search: (
    <>
      <circle cx="10.75" cy="10.75" r="6" />
      <path d="m15.25 15.25 4.5 4.5" />
    </>
  ),
  send: (
    <>
      <path d="M4.25 12h11.5" />
      <path d="m11.75 7.5 4.5 4.5-4.5 4.5" />
      <rect x="17.75" y="4.25" width="3" height="15.5" rx="1" />
    </>
  ),
  note: (
    <>
      <path d="M9.25 17.25V5.5l10-1.75v11.5" />
      <circle cx="6.75" cy="17.25" r="2.5" />
      <circle cx="16.75" cy="15.25" r="2.5" />
    </>
  ),
  lyrics: (
    <>
      <path d="M5 6.25h14M5 10.25h9M5 14.25h11M5 18.25h6" />
    </>
  ),
  wave: <path d="M3 12h2.5l2-5.5 3 11 3-13 3 10 2-2.5H21" />,
  firmware: (
    <>
      <path d="M12 3.5 20.5 8 12 12.5 3.5 8Z" />
      <path d="m3.5 12 8.5 4.5 8.5-4.5" />
      <path d="m3.5 16 8.5 4.5 8.5-4.5" />
    </>
  ),
  download: (
    <>
      <path d="M12 4v10.5" />
      <path d="m7.5 10 4.5 4.5 4.5-4.5" />
      <path d="M4.75 19.25h14.5" />
    </>
  ),
  external: (
    <>
      <path d="M13.5 4.75h5.75v5.75" />
      <path d="m19.25 4.75-8.5 8.5" />
      <path d="M17.25 13.75v4a1.5 1.5 0 0 1-1.5 1.5h-9.5a1.5 1.5 0 0 1-1.5-1.5v-9.5a1.5 1.5 0 0 1 1.5-1.5h4" />
    </>
  ),
  info: (
    <>
      <circle cx="12" cy="12" r="8.75" />
      <path d="M12 11v5.5" />
      <circle cx="12" cy="7.9" r="0.55" />
    </>
  ),
} as const;

export type IconName = keyof typeof paths;

export function Icon({ name, size = 18, ...rest }: { name: IconName; size?: number } & SVGProps<SVGSVGElement>) {
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth={1.5}
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
      {...rest}
    >
      {paths[name]}
    </svg>
  );
}
