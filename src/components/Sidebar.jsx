import React from 'react';
import { HomeIcon, ServerIcon, SplitIcon, DeviceIcon, FileServerIcon, SettingsIcon } from './icons.jsx';

const NAV_ITEMS = [
  { id: 'home', label: 'Home', icon: HomeIcon },
  { id: 'servers', label: 'Servers', icon: ServerIcon },
  { id: 'split', label: 'Split Tunneling', icon: SplitIcon },
  { id: 'devices', label: 'Devices', icon: DeviceIcon },
  { id: 'files', label: 'File Server', icon: FileServerIcon, badge: 'Soon' },
  { id: 'settings', label: 'Settings', icon: SettingsIcon },
];

export default function Sidebar({ page, onNavigate, session }) {
  return (
    <aside className="sidebar">
      <div className="sidebar-brand">
        <div className="sidebar-brand-mark">T</div>
        <div className="sidebar-brand-text">
          <h1>TayyemVPN</h1>
          <p>Your access. Your rules.</p>
        </div>
      </div>

      <nav className="sidebar-nav">
        {NAV_ITEMS.map(({ id, label, icon: Icon, badge }) => (
          <button key={id} className={page === id ? 'active' : ''} onClick={() => onNavigate(id)}>
            <Icon />
            <span style={{ flex: 1 }}>{label}</span>
            {badge && (
              <span style={{ fontSize: 9.5, fontWeight: 700, color: 'var(--text-faint)', background: 'var(--surface-2)', padding: '2px 6px', borderRadius: 999 }}>
                {badge}
              </span>
            )}
          </button>
        ))}
      </nav>

      <div className="sidebar-footer">
        <div className="sidebar-plan">
          <span className="dot" />
          <div>
            <div className="label">{session.username}</div>
            <div className="sub">VPN access active</div>
          </div>
        </div>
      </div>
    </aside>
  );
}
