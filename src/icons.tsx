// Inline stroke icons. All are decorative: the button that holds one carries the label.

const MARK =
  "M15.38 0L1.96 8.88C1.49 9.19 0.87 9.03 0.59 8.54C0.3 8.05 0.49 7.43 1 7.2L15.38 0ZM28.56 22.3L15.58 31.43C14.91 31.9 13.96 31.72 13.54 30.97C13.13 30.25 13.41 29.32 14.16 28.98L28.56 22.3ZM28.56 18.55L15.58 27.69C14.91 28.15 13.96 27.97 13.54 27.23C13.13 26.5 13.41 25.57 14.16 25.24L28.56 18.55ZM28.56 14.81L15.58 23.95C14.91 24.41 13.96 24.23 13.54 23.48C13.13 22.76 13.41 21.83 14.16 21.5L28.56 14.81ZM28.56 11.1L15.58 20.23C14.91 20.7 13.96 20.52 13.54 19.77C13.13 19.05 13.41 18.12 14.16 17.78L28.56 11.1ZM28.3 7.48L15.56 16.46C14.89 16.95 13.93 16.75 13.52 16.03C13.11 15.3 13.39 14.37 14.16 14.04L28.33 7.48H28.3ZM25.08 5.63L11.66 14.48C11.2 14.79 10.58 14.63 10.29 14.14C10.01 13.65 10.19 13.03 10.71 12.8L25.08 5.63ZM21.83 3.74L8.41 12.59C7.94 12.9 7.32 12.75 7.04 12.26C6.76 11.77 6.94 11.15 7.45 10.92L21.83 3.74ZM18.6 1.88L5.18 10.74C4.72 11.05 4.1 10.89 3.81 10.4C3.53 9.91 3.71 9.29 4.23 9.06L18.6 1.88ZM13.52 27.23L10.19 29.68C9.96 29.83 9.65 29.78 9.52 29.52C9.39 29.29 9.49 28.98 9.72 28.88L13.49 27.23H13.52ZM13.52 23.51L6.99 27.85C6.76 28 6.45 27.92 6.29 27.69C6.16 27.46 6.24 27.15 6.5 27.02C6.5 27.02 13.52 23.51 13.52 23.54V23.51ZM13.52 19.77L3.74 25.99C3.51 26.14 3.2 26.06 3.04 25.81C2.89 25.57 2.99 25.24 3.25 25.11C3.25 25.11 13.52 19.74 13.52 19.77ZM13.52 16.03L0.77 23.97C0.54 24.13 0.2 24.05 0.07 23.79C-0.08 23.56 0.02 23.23 0.28 23.1L13.54 16.03H13.52ZM10.29 14.14L0.77 20.23C0.54 20.39 0.23 20.31 0.07 20.05C-0.08 19.82 0.02 19.48 0.28 19.35L10.29 14.14ZM7.04 12.28L0.74 16.49C0.51 16.65 0.2 16.57 0.05 16.34H0.07C-0.06 16.1 0.02 15.79 0.28 15.66L7.07 12.31L7.04 12.28ZM3.81 10.43L0.74 12.72C0.54 12.88 0.2 12.83 0.07 12.59C-0.06 12.36 0.05 12.05 0.28 11.95L3.81 10.43Z";

const markPath = <path transform="translate(4.39 2.92) scale(0.82)" fill="#FFFFFF" d={MARK} />;

/** The instabox mark on its own, for the header. */
export const Mark = () => (
  <svg width="18" height="18" viewBox="0 0 32 32" aria-hidden="true">
    {markPath}
  </svg>
);

/** The mark on its black tile, for the first-run screen. */
export const Logo = () => (
  <svg width="44" height="44" viewBox="0 0 32 32" role="img" aria-label="instabox">
    <rect width="32" height="32" rx="7" fill="#000000" stroke="rgba(255,255,255,0.16)" strokeWidth="0.6" />
    {markPath}
  </svg>
);

const stroke = { fill: "none", stroke: "currentColor", strokeLinecap: "round", strokeLinejoin: "round" } as const;

export const Gear = () => (
  <svg width="16" height="16" viewBox="0 0 24 24" strokeWidth="1.75" aria-hidden="true" {...stroke}>
    <path d="M12.22 2h-.44a2 2 0 0 0-2 2v.18a2 2 0 0 1-1 1.73l-.43.25a2 2 0 0 1-2 0l-.15-.08a2 2 0 0 0-2.73.73l-.22.38a2 2 0 0 0 .73 2.73l.15.1a2 2 0 0 1 1 1.72v.51a2 2 0 0 1-1 1.74l-.15.09a2 2 0 0 0-.73 2.73l.22.38a2 2 0 0 0 2.73.73l.15-.08a2 2 0 0 1 2 0l.43.25a2 2 0 0 1 1 1.73V20a2 2 0 0 0 2 2h.44a2 2 0 0 0 2-2v-.18a2 2 0 0 1 1-1.73l.43-.25a2 2 0 0 1 2 0l.15.08a2 2 0 0 0 2.73-.73l.22-.39a2 2 0 0 0-.73-2.73l-.15-.08a2 2 0 0 1-1-1.74v-.5a2 2 0 0 1 1-1.74l.15-.09a2 2 0 0 0 .73-2.73l-.22-.38a2 2 0 0 0-2.73-.73l-.15.08a2 2 0 0 1-2 0l-.43-.25a2 2 0 0 1-1-1.73V4a2 2 0 0 0-2-2z" />
    <circle cx="12" cy="12" r="3" />
  </svg>
);

export const Back = () => (
  <svg width="16" height="16" viewBox="0 0 24 24" strokeWidth="2" aria-hidden="true" {...stroke}>
    <path d="M15 18l-6-6 6-6" />
  </svg>
);

export const ExternalLink = () => (
  <svg width="16" height="16" viewBox="0 0 24 24" strokeWidth="1.75" aria-hidden="true" {...stroke}>
    <path d="M14 3h7v7M21 3l-10 10M10 3H5a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h14a2 2 0 0 0 2-2v-5" />
  </svg>
);

export const Dots = () => (
  <svg width="16" height="16" viewBox="0 0 24 24" fill="currentColor" aria-hidden="true">
    <circle cx="5" cy="12" r="1.6" />
    <circle cx="12" cy="12" r="1.6" />
    <circle cx="19" cy="12" r="1.6" />
  </svg>
);

export const Plus = () => (
  <svg width="14" height="14" viewBox="0 0 24 24" strokeWidth="2" aria-hidden="true" {...stroke}>
    <path d="M12 5v14M5 12h14" />
  </svg>
);

export const ChevronRight = () => (
  <svg width="14" height="14" viewBox="0 0 24 24" strokeWidth="2" aria-hidden="true" {...stroke}>
    <path d="M9 18l6-6-6-6" />
  </svg>
);

export const ChevronDown = () => (
  <svg width="14" height="14" viewBox="0 0 24 24" strokeWidth="2" aria-hidden="true" {...stroke}>
    <path d="M6 9l6 6 6-6" />
  </svg>
);

export const Check = () => (
  <svg width="14" height="14" viewBox="0 0 24 24" strokeWidth="2.25" aria-hidden="true" {...stroke}>
    <path d="M20 6L9 17l-5-5" />
  </svg>
);

export const Copy = () => (
  <svg width="14" height="14" viewBox="0 0 24 24" strokeWidth="2" aria-hidden="true" {...stroke}>
    <rect x="9" y="9" width="13" height="13" rx="2" />
    <path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1" />
  </svg>
);

export const Terminal = () => (
  <svg width="14" height="14" viewBox="0 0 24 24" strokeWidth="2" aria-hidden="true" {...stroke}>
    <path d="M4 17l6-6-6-6M12 19h8" />
  </svg>
);

export const Alert = () => (
  <svg width="16" height="16" viewBox="0 0 24 24" strokeWidth="2" aria-hidden="true" {...stroke}>
    <circle cx="12" cy="12" r="9" />
    <path d="M12 8v5M12 16h.01" />
  </svg>
);
