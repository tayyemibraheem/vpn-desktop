import React, { useEffect, useState } from 'react';
import { api } from '../api.js';
import TitleBar from './TitleBar.jsx';

export default function SplitTunnelingPage({ connected }) {
  const [tab, setTab] = useState('apps');
  const [apps, setApps] = useState([]);
  const [destinations, setDestinations] = useState([]);
  const [candidateApps, setCandidateApps] = useState([]);
  const [appPickerOpen, setAppPickerOpen] = useState(false);
  const [newDestination, setNewDestination] = useState('');
  const [saving, setSaving] = useState(false);
  const [pickerLoading, setPickerLoading] = useState(false);
  const [pickerError, setPickerError] = useState(null);
  const [saveError, setSaveError] = useState(null);

  useEffect(() => {
    api.getSplitTunnelConfig().then((cfg) => {
      setApps(cfg.apps || []);
      setDestinations(cfg.destinations || []);
    });
  }, []);

  async function persist(nextApps, nextDestinations) {
    setSaving(true);
    setSaveError(null);
    try {
      await api.setSplitTunnelConfig({ apps: nextApps, destinations: nextDestinations });
    } catch (err) {
      setSaveError(err?.message || String(err));
    } finally {
      setSaving(false);
    }
  }

  function removeApp(name) {
    const next = apps.filter((a) => a !== name);
    setApps(next);
    persist(next, destinations);
  }

  async function openAppPicker() {
    setAppPickerOpen(true);
    setPickerLoading(true);
    setPickerError(null);
    try {
      const list = await api.listCandidateApps();
      setCandidateApps(list.filter((c) => !apps.includes(c.name)));
    } catch (err) {
      setPickerError(err?.message || String(err));
    } finally {
      setPickerLoading(false);
    }
  }

  function addApp(name) {
    const next = [...apps, name];
    setApps(next);
    setAppPickerOpen(false);
    persist(next, destinations);
  }

  function addDestination() {
    const value = newDestination.trim();
    if (!value || destinations.includes(value)) return;
    const next = [...destinations, value];
    setDestinations(next);
    setNewDestination('');
    persist(apps, next);
  }

  function removeDestination(value) {
    const next = destinations.filter((d) => d !== value);
    setDestinations(next);
    persist(apps, next);
  }

  return (
    <>
      <TitleBar />
      <div className="page">
        <div className="page-header">
          <h2>Split Tunneling</h2>
          <p>Choose apps or destinations that bypass the VPN entirely.</p>
        </div>

        <div className="tabs">
          <button className={tab === 'apps' ? 'active' : ''} onClick={() => setTab('apps')}>Applications</button>
          <button className={tab === 'destinations' ? 'active' : ''} onClick={() => setTab('destinations')}>Websites / IPs</button>
        </div>

        {tab === 'apps' && (
          <div className="card" style={{ maxWidth: 460, display: 'flex', flexDirection: 'column', gap: 10 }}>
            <div className="list">
              {apps.length === 0 && <p className="empty-hint">No apps excluded — everything goes through the tunnel.</p>}
              {apps.map((name) => (
                <div className="list-row" key={name}>
                  <span>{name}</span>
                  <button onClick={() => removeApp(name)} title="Remove">×</button>
                </div>
              ))}
            </div>
            {!appPickerOpen ? (
              <button className="btn btn-ghost" onClick={openAppPicker}>+ Add Application</button>
            ) : (
              <div className="list" style={{ maxHeight: 220, overflowY: 'auto' }}>
                {pickerLoading && <p className="empty-hint">Loading running apps…</p>}
                {pickerError && <p className="error-text">{pickerError}</p>}
                {!pickerLoading && !pickerError && candidateApps.length === 0 && <p className="empty-hint">No other running apps found.</p>}
                {candidateApps.map((c) => (
                  <div className="list-row pickable" key={c.name} onClick={() => addApp(c.name)}>
                    <span>{c.name}</span>
                    <span>+</span>
                  </div>
                ))}
              </div>
            )}
          </div>
        )}

        {tab === 'destinations' && (
          <div className="card" style={{ maxWidth: 460, display: 'flex', flexDirection: 'column', gap: 10 }}>
            <div className="list">
              {destinations.length === 0 && <p className="empty-hint">No IPs, subnets, or domains excluded.</p>}
              {destinations.map((d) => (
                <div className="list-row" key={d}>
                  <span>{d}</span>
                  <button onClick={() => removeDestination(d)} title="Remove">×</button>
                </div>
              ))}
            </div>
            <div className="inline-add">
              <input
                placeholder="IP, CIDR, or domain (e.g. 192.168.1.0/24)"
                value={newDestination}
                onChange={(e) => setNewDestination(e.target.value)}
                onKeyDown={(e) => e.key === 'Enter' && addDestination()}
              />
              <button className="btn btn-ghost" onClick={addDestination}>Add</button>
            </div>
          </div>
        )}

        {saveError && <p className="error-text">{saveError}</p>}
        <p className="empty-hint">
          {connected ? (saving ? 'Applying…' : 'Changes apply immediately while connected.') : 'Changes take effect on your next connect.'}
        </p>
      </div>
    </>
  );
}
