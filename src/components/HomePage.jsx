import React, { useEffect, useState } from 'react';
import { api } from '../api.js';
import TitleBar from './TitleBar.jsx';
import { PowerIcon } from './icons.jsx';

function formatElapsed(ms) {
  const totalSeconds = Math.floor(ms / 1000);
  const h = String(Math.floor(totalSeconds / 3600)).padStart(2, '0');
  const m = String(Math.floor((totalSeconds % 3600) / 60)).padStart(2, '0');
  const s = String(totalSeconds % 60).padStart(2, '0');
  return `${h}:${m}:${s}`;
}

export default function HomePage({ session, status }) {
  const [busy, setBusy] = useState(false);
  const [connectedAt, setConnectedAt] = useState(null);
  const [elapsed, setElapsed] = useState(0);
  const [splitApps, setSplitApps] = useState([]);
  const [connectError, setConnectError] = useState(null);
  const [logs, setLogs] = useState([]);
  const [showLogs, setShowLogs] = useState(false);

  const [servers, setServers] = useState([]);
  const [currentServer, setCurrentServer] = useState(null);
  const [switchingId, setSwitchingId] = useState(null);
  const [serverError, setServerError] = useState(null);

  async function refreshServers() {
    try {
      const [subscription, current] = await Promise.all([api.getSubscription(), api.getCurrentServer()]);
      setServers(subscription.allowedServers || []);
      setCurrentServer(current);
    } catch (err) {
      setServerError(err?.message || String(err));
    }
  }

  useEffect(() => {
    refreshServers();
  }, []);

  useEffect(() => {
    let unlisten;
    api.onLog((line) => setLogs((prev) => [...prev.slice(-199), line])).then((fn) => (unlisten = fn));
    return () => unlisten && unlisten();
  }, []);

  useEffect(() => {
    if (status.state === 'connected' && !connectedAt) {
      setConnectedAt(Date.now());
    } else if (status.state !== 'connected' && connectedAt) {
      setConnectedAt(null);
    }
  }, [status.state]); // eslint-disable-line react-hooks/exhaustive-deps

  useEffect(() => {
    if (!connectedAt) {
      setElapsed(0);
      return;
    }
    const id = setInterval(() => setElapsed(Date.now() - connectedAt), 1000);
    return () => clearInterval(id);
  }, [connectedAt]);

  useEffect(() => {
    api.getSplitTunnelConfig().then((cfg) => setSplitApps(cfg.apps || []));
  }, []);

  async function toggleConnection() {
    setBusy(true);
    setConnectError(null);
    try {
      if (status.state === 'disconnected') {
        const result = await api.connect();
        if (!result?.ok) {
          setConnectError(result?.error || 'Failed to connect');
        }
      } else {
        await api.disconnect();
      }
    } catch (err) {
      setConnectError(err?.message || String(err));
    } finally {
      setBusy(false);
    }
  }

  async function onPickServer(server) {
    if (switchingId || server.id === currentServer?.serverId || status.state === 'connecting') return;
    setSwitchingId(server.id);
    setServerError(null);
    try {
      const result = await api.setServer(server.id);
      if (!result?.ok) throw new Error(result?.error || 'Could not switch servers');
      await refreshServers();
    } catch (err) {
      setServerError(err?.message || String(err));
    } finally {
      setSwitchingId(null);
    }
  }

  const ringClass = `status-ring ${status.state}`;
  const stateLabel =
    status.state === 'connected' ? 'Connected' :
    status.state === 'connecting' ? (status.detail || 'Connecting…') :
    'Disconnected';

  const hour = new Date().getHours();
  const greeting = hour < 12 ? 'Good morning' : hour < 18 ? 'Good afternoon' : 'Good evening';

  return (
    <>
      <TitleBar />
      <div className="page">
        <div className="page-header">
          <h2>{greeting}, {session.username}</h2>
          <p>Stay private, stay in control.</p>
        </div>

        <div className="home-grid">
          <div style={{ display: 'flex', flexDirection: 'column', gap: 18 }}>
            <div className="card connect-card">
              <div className={ringClass}>
                <button onClick={toggleConnection} disabled={busy || status.state === 'connecting'} title={stateLabel}>
                  <PowerIcon />
                </button>
              </div>
              <div className={`status-text ${status.state}`}>
                <div className="state">{stateLabel}</div>
                {status.state === 'connected' && <div className="timer">{formatElapsed(elapsed)}</div>}
              </div>

              <div className="server-row">
                <div style={{ display: 'flex', alignItems: 'center', gap: 10 }}>
                  <div>
                    <div className="name">{currentServer?.serverName || 'No server selected'}</div>
                    {status.state === 'connected' && <div className="sub">Connected through this server</div>}
                  </div>
                </div>
              </div>

              <button
                className={status.state === 'connected' ? 'btn btn-danger' : 'btn btn-primary'}
                style={{ width: '100%', maxWidth: 340 }}
                onClick={toggleConnection}
                disabled={busy || status.state === 'connecting'}
              >
                {status.state === 'connected' ? 'Disconnect' : status.state === 'connecting' ? 'Connecting…' : 'Connect'}
              </button>
              {connectError && <p className="error-text">{connectError}</p>}
              {!connectError && status.state === 'disconnected' && status.detail && (
                <p className="error-text">{status.detail}</p>
              )}
            </div>

            <div className="card" style={{ display: 'flex', flexDirection: 'column', gap: 12 }}>
              <div className="side-card">
                <div className="section-title">Servers</div>
              </div>
              {serverError && <p className="error-text">{serverError}</p>}
              {servers.length === 0 && !serverError && (
                <p className="empty-hint" style={{ margin: 0 }}>No servers available on your plan yet.</p>
              )}
              <div className="quick-connect-grid">
                {servers.map((s) => {
                  const isCurrent = s.id === currentServer?.serverId;
                  const isSwitching = switchingId === s.id;
                  return (
                    <button
                      key={s.id}
                      type="button"
                      onClick={() => onPickServer(s)}
                      disabled={switchingId != null || status.state === 'connecting'}
                      className={`quick-connect-item ${isCurrent ? 'active' : ''}`}
                      style={{ opacity: 1, cursor: isCurrent ? 'default' : 'pointer', font: 'inherit', color: 'inherit' }}
                    >
                      <span className="name">{s.name}</span>
                      <span className="city">{isCurrent ? 'Selected' : isSwitching ? 'Switching…' : 'Tap to use'}</span>
                    </button>
                  );
                })}
              </div>
            </div>
          </div>

          <div style={{ display: 'flex', flexDirection: 'column', gap: 18 }}>
            <div className="card side-card">
              <div className="section-title">Your Connection</div>
              <div className="ip-row">
                <div>
                  <span style={{ fontSize: 11, color: 'var(--text-faint)' }}>Status</span>
                  <div className="value" style={{ fontSize: 14 }}>
                    {status.state === 'connected' ? 'Traffic tunneled' : 'Traffic direct'}
                  </div>
                </div>
              </div>
              <p className="empty-hint" style={{ margin: 0 }}>
                Public IP / location lookup isn't wired up yet — placeholder for a follow-up.
              </p>
              <button className="log-toggle" onClick={() => setShowLogs((v) => !v)}>
                {showLogs ? 'Hide connection log' : 'Show connection log'}
              </button>
              {showLogs && (
                <div className="log-box">
                  {logs.length ? logs.join('\n') : 'No log output yet.'}
                </div>
              )}
            </div>

            <div className="card side-card">
              <div className="toggle-row">
                <div className="section-title" style={{ marginBottom: 0 }}>Split Tunneling</div>
                <span className={`toggle ${splitApps.length ? 'on' : ''}`} style={{ pointerEvents: 'none' }}>
                  <span className="knob" />
                </span>
              </div>
              <div className="mini-app-list">
                {splitApps.length === 0 && <p className="empty-hint" style={{ margin: 0 }}>No apps bypass the VPN yet.</p>}
                {splitApps.slice(0, 4).map((name) => (
                  <div className="mini-app-row" key={name}>
                    <span className="name">{name}</span>
                  </div>
                ))}
              </div>
            </div>
          </div>
        </div>
      </div>
    </>
  );
}
