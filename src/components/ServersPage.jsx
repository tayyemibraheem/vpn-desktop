import React, { useEffect, useState } from 'react';
import { api } from '../api.js';
import TitleBar from './TitleBar.jsx';

export default function ServersPage({ status }) {
  const [servers, setServers] = useState(null);
  const [current, setCurrent] = useState(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState(null);
  const [switchingId, setSwitchingId] = useState(null);
  const [justSwitched, setJustSwitched] = useState(false);

  async function refresh() {
    setError(null);
    try {
      const [subscription, currentServer] = await Promise.all([api.getSubscription(), api.getCurrentServer()]);
      setServers(subscription.allowedServers || []);
      setCurrent(currentServer);
    } catch (err) {
      setError(err?.message || String(err));
    } finally {
      setLoading(false);
    }
  }

  useEffect(() => {
    refresh();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  async function onPick(server) {
    if (switchingId || server.id === current?.serverId) return;
    setSwitchingId(server.id);
    setError(null);
    setJustSwitched(false);
    try {
      const result = await api.setServer(server.id);
      if (!result.ok) throw new Error(result.error || 'Could not switch servers');
      await refresh();
      setJustSwitched(true);
    } catch (err) {
      setError(err?.message || String(err));
    } finally {
      setSwitchingId(null);
    }
  }

  return (
    <>
      <TitleBar />
      <div className="page">
        <div className="page-header">
          <h2>Servers</h2>
          <p>Pick which location your PC connects through. Switching disconnects you first — click Connect again afterward.</p>
        </div>

        {loading && <p className="empty-hint">Loading servers…</p>}
        {error && <p className="error-text">{error}</p>}
        {justSwitched && !error && (
          <p className="empty-hint" style={{ fontStyle: 'normal', color: 'var(--success)' }}>
            Switched. Click Connect to use the new server.
          </p>
        )}

        {!loading && (
          <div className="list" style={{ maxWidth: 460 }}>
            {(servers ?? []).length === 0 && <p className="empty-hint">No servers available on your plan yet.</p>}
            {(servers ?? []).map((s) => {
              const isCurrent = s.id === current?.serverId;
              const isSwitching = switchingId === s.id;
              return (
                <button
                  key={s.id}
                  type="button"
                  onClick={() => onPick(s)}
                  disabled={switchingId != null}
                  className="list-row"
                  style={{
                    padding: '12px 14px',
                    width: '100%',
                    border: 'none',
                    background: 'none',
                    textAlign: 'left',
                    cursor: isCurrent || switchingId != null ? 'default' : 'pointer',
                    font: 'inherit',
                    color: 'inherit',
                  }}
                >
                  <div>
                    <div style={{ fontWeight: 700 }}>{s.name}</div>
                    <div style={{ fontSize: 11.5, color: 'var(--text-faint)' }}>
                      {isCurrent ? 'Selected' : isSwitching ? 'Switching…' : 'Click to select'}
                    </div>
                  </div>
                  {isCurrent && status.state === 'connected' && (
                    <span style={{ fontSize: 11, fontWeight: 700, color: 'var(--success)' }}>Connected</span>
                  )}
                </button>
              );
            })}
          </div>
        )}
      </div>
    </>
  );
}
