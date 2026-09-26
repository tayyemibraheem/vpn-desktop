import React, { useState } from 'react';
import TitleBar from './TitleBar.jsx';
import { checkForUpdate } from '../updater.js';

export default function SettingsPage({ session, onLogout }) {
  const [updateState, setUpdateState] = useState('idle'); // idle | checking | none | error
  const [updateError, setUpdateError] = useState(null);

  async function onCheckForUpdates() {
    setUpdateState('checking');
    setUpdateError(null);
    try {
      const result = await checkForUpdate();
      if (!result.available) setUpdateState('none');
      // If an update WAS available, checkForUpdate() relaunches the app and this line never runs.
    } catch (err) {
      setUpdateState('error');
      setUpdateError(err?.message || String(err));
    }
  }

  return (
    <>
      <TitleBar />
      <div className="page">
        <div className="page-header">
          <h2>Settings</h2>
        </div>

        <div className="card side-card" style={{ maxWidth: 420 }}>
          <div className="section-title">Account</div>
          <div>
            <div style={{ fontWeight: 700, fontSize: 14 }}>{session.username}</div>
            {session.email && <div style={{ fontSize: 12, color: 'var(--text-faint)' }}>{session.email}</div>}
          </div>
          <button className="btn btn-ghost" onClick={onLogout} style={{ alignSelf: 'flex-start' }}>Log out</button>
        </div>

        <div className="card side-card" style={{ maxWidth: 420 }}>
          <div className="section-title">Updates</div>
          <p className="empty-hint" style={{ margin: 0 }}>
            {updateState === 'idle' && 'Check whether a newer version is available.'}
            {updateState === 'checking' && 'Checking for updates…'}
            {updateState === 'none' && "You're on the latest version."}
            {updateState === 'error' && `Couldn't check for updates: ${updateError}`}
          </p>
          <button className="btn btn-ghost" onClick={onCheckForUpdates} disabled={updateState === 'checking'} style={{ alignSelf: 'flex-start' }}>
            Check for updates
          </button>
        </div>

        <div className="card side-card" style={{ maxWidth: 420 }}>
          <div className="section-title">About</div>
          <p className="empty-hint" style={{ margin: 0 }}>
            WireGuard is built directly into TayyemVPN — there's nothing else to install.
          </p>
        </div>
      </div>
    </>
  );
}
