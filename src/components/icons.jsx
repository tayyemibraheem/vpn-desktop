import React from 'react';

const base = { width: 17, height: 17, viewBox: '0 0 24 24', fill: 'none', stroke: 'currentColor', strokeWidth: 2, strokeLinecap: 'round', strokeLinejoin: 'round' };

export const HomeIcon = () => (
  <svg {...base}><path d="M3 11.5 12 4l9 7.5" /><path d="M5 10v9a1 1 0 0 0 1 1h4v-6h4v6h4a1 1 0 0 0 1-1v-9" /></svg>
);
export const ServerIcon = () => (
  <svg {...base}><rect x="3" y="4" width="18" height="7" rx="2" /><rect x="3" y="13" width="18" height="7" rx="2" /><path d="M7 7.5h.01M7 16.5h.01" /></svg>
);
export const SplitIcon = () => (
  <svg {...base}><circle cx="6" cy="6" r="2.5" /><circle cx="6" cy="18" r="2.5" /><circle cx="18" cy="12" r="2.5" /><path d="M8.2 6.9 15.8 11M8.2 17.1 15.8 13" /></svg>
);
export const FileServerIcon = () => (
  <svg {...base}><rect x="3" y="3" width="18" height="6" rx="1.5" /><rect x="3" y="15" width="18" height="6" rx="1.5" /><path d="M7 6h.01M7 18h.01" /><path d="M12 9v6" /></svg>
);
export const SettingsIcon = () => (
  <svg {...base}><circle cx="12" cy="12" r="3" /><path d="M19.4 15a1.7 1.7 0 0 0 .34 1.87l.06.06a2 2 0 1 1-2.83 2.83l-.06-.06a1.7 1.7 0 0 0-1.87-.34 1.7 1.7 0 0 0-1 1.55V21a2 2 0 0 1-4 0v-.09a1.7 1.7 0 0 0-1-1.55 1.7 1.7 0 0 0-1.87.34l-.06.06a2 2 0 1 1-2.83-2.83l.06-.06a1.7 1.7 0 0 0 .34-1.87 1.7 1.7 0 0 0-1.55-1H3a2 2 0 0 1 0-4h.09a1.7 1.7 0 0 0 1.55-1 1.7 1.7 0 0 0-.34-1.87l-.06-.06a2 2 0 1 1 2.83-2.83l.06.06a1.7 1.7 0 0 0 1.87.34H9a1.7 1.7 0 0 0 1-1.55V3a2 2 0 0 1 4 0v.09a1.7 1.7 0 0 0 1 1.55 1.7 1.7 0 0 0 1.87-.34l.06-.06a2 2 0 1 1 2.83 2.83l-.06.06a1.7 1.7 0 0 0-.34 1.87V9a1.7 1.7 0 0 0 1.55 1H21a2 2 0 0 1 0 4h-.09a1.7 1.7 0 0 0-1.55 1Z" /></svg>
);
export const PowerIcon = ({ size = 34 }) => (
  <svg width={size} height={size} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth={2.2} strokeLinecap="round" strokeLinejoin="round">
    <path d="M12 2v9" /><path d="M18.4 6.6a9 9 0 1 1-12.8 0" />
  </svg>
);
export const MinusIcon = () => (<svg {...base} width={13} height={13}><path d="M5 12h14" /></svg>);
export const SquareIcon = () => (<svg {...base} width={11} height={11}><rect x="4" y="4" width="16" height="16" rx="1.5" /></svg>);
export const XIcon = () => (<svg {...base} width={13} height={13}><path d="M6 6l12 12M18 6 6 18" /></svg>);
export const CopyIcon = () => (<svg {...base} width={14} height={14}><rect x="8" y="8" width="12" height="12" rx="2" /><path d="M4 16V6a2 2 0 0 1 2-2h10" /></svg>);
export const ChevronRight = () => (<svg {...base} width={16} height={16}><path d="M9 6l6 6-6 6" /></svg>);
export const DeviceIcon = () => (
  <svg {...base}><rect x="5" y="2" width="9" height="14" rx="1.5" /><path d="M8.5 13.5h2" /><rect x="14" y="8" width="7" height="10" rx="1.5" /><path d="M16.7 15.5h1.6" /></svg>
);
