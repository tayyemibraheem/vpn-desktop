import React, { useEffect, useState } from 'react';
import QRCode from 'qrcode';
import { api } from '../api.js';
import TitleBar from './TitleBar.jsx';

const PLATFORMS = [
  { value: 'IOS', label: 'iPhone / iPad' },
  { value: 'ANDROID', label: 'Android' },
  { value: 'MACOS', label: 'Mac' },
  { value: 'WINDOWS', label: 'Windows PC' },
  { value: 'LINUX', label: 'Linux' },
];

function platformLabel(value) {
  return PLATFORMS.find((p) => p.value === value)?.label || value;
}

export default function DevicesPage() {
  const [devices, setDevices] = useState([]);
  const [subscription, setSubscription] = useState(null);
  const [loading, setLoading] = useState(true);
  const [loadError, setLoadError] = useState(null);

  const [formOpen, setFormOpen] = useState(false);
  const [deviceName, setDeviceName] = useState('');
  const [platform, setPlatform] = useState('IOS');
  const [requesting, setRequesting] = useState(false);
  const [requestError, setRequestError] = useState(null);

  // { action: 'ENROLL'|'REVOKE', maskedEmail, deviceName, platform?, deviceId? }
  const [pending, setPending] = useState(null);
  const [code, setCode] = useState('');
  const [confirming, setConfirming] = useState(false);
  const [confirmError, setConfirmError] = useState(null);

  const [enrolled, setEnrolled] = useState(null); // { configText, qrDataUrl, deviceName }

  async function refresh() {
    setLoadError(null);
    try {
      const [deviceList, sub] = await Promise.all([api.listDevices(), api.getSubscription()]);
      setDevices(deviceList);
      setSubscription(sub);
    } catch (err) {
      setLoadError(err?.message || String(err));
    } finally {
      setLoading(false);
    }
  }

  useEffect(() => {
    refresh();
  }, []);

  const atLimit = subscription && devices.length >= subscription.maxDevices;

  async function onRequestEnroll(e) {
    e.preventDefault();
    const name = deviceName.trim();
    if (!name) return;
    setRequesting(true);
    setRequestError(null);
    try {
      const result = await api.requestDeviceVerification({ action: 'ENROLL', deviceName: name, platform });
      setPending({ action: 'ENROLL', deviceName: name, platform, maskedEmail: result.maskedEmail });
      setFormOpen(false);
      setDeviceName('');
      setCode('');
    } catch (err) {
      setRequestError(err?.message || String(err));
    } finally {
      setRequesting(false);
    }
  }

  async function onRequestRevoke(device) {
    setRequestError(null);
    setRequesting(true);
    try {
      const result = await api.requestDeviceVerification({ action: 'REVOKE', deviceId: device.id });
      setPending({ action: 'REVOKE', deviceId: device.id, deviceName: device.deviceName, maskedEmail: result.maskedEmail });
      setCode('');
    } catch (err) {
      setRequestError(err?.message || String(err));
    } finally {
      setRequesting(false);
    }
  }

  async function onConfirm(e) {
    e.preventDefault();
    if (!code.trim()) return;
    setConfirming(true);
    setConfirmError(null);
    try {
      const result = await api.confirmDeviceVerification(code.trim());
      if (pending.action === 'ENROLL') {
        const qrDataUrl = await QRCode.toDataURL(result.configText, { width: 260, margin: 1 });
        setEnrolled({ ...result, qrDataUrl });
      }
      setPending(null);
      setCode('');
      await refresh();
    } catch (err) {
      setConfirmError(err?.message || String(err));
    } finally {
      setConfirming(false);
    }
  }

  function downloadConfig() {
    const blob = new Blob([enrolled.configText], { type: 'text/plain' });
    const url = URL.createObjectURL(blob);
    const a = document.createElement('a');
    a.href = url;
    a.download = `${enrolled.deviceName.replace(/[^a-z0-9-_]+/gi, '_') || 'device'}.conf`;
    a.click();
    URL.revokeObjectURL(url);
  }

  async function copyConfig() {
    try {
      await navigator.clipboard.writeText(enrolled.configText);
    } catch {
      // Clipboard access can be unavailable in some webview contexts — the download/QR options still work.
    }
  }

  return (
    <>
      <TitleBar />
      <div className="page">
        <div className="page-header">
          <h2>Devices</h2>
          <p>Add a phone or another computer to your VPN by scanning a QR code — no separate WireGuard app needed.</p>
        </div>

        {loading && <p className="empty-hint">Loading your devices…</p>}
        {loadError && <p className="error-text">{loadError}</p>}

        {!loading && !loadError && !pending && (
          <>
            {subscription && (
              <p className="empty-hint" style={{ fontStyle: 'normal' }}>
                {devices.length} of {subscription.maxDevices} devices used
              </p>
            )}

            <div className="card" style={{ maxWidth: 460, display: 'flex', flexDirection: 'column', gap: 10 }}>
              <div className="list">
                {devices.length === 0 && <p className="empty-hint">No devices yet.</p>}
                {devices.map((d) => (
                  <div className="list-row" key={d.id}>
                    <div>
                      <div style={{ fontWeight: 700 }}>{d.deviceName}</div>
                      <div style={{ fontSize: 11.5, color: 'var(--text-faint)' }}>
                        {platformLabel(d.platform)} · {d.assignedIp}
                      </div>
                    </div>
                    <button onClick={() => onRequestRevoke(d)} disabled={requesting} title="Remove">
                      ×
                    </button>
                  </div>
                ))}
              </div>

              {requestError && <p className="error-text">{requestError}</p>}

              {!formOpen ? (
                <button className="btn btn-ghost" onClick={() => setFormOpen(true)} disabled={atLimit} style={{ alignSelf: 'flex-start' }}>
                  + Add Device
                </button>
              ) : (
                <form onSubmit={onRequestEnroll} style={{ display: 'flex', flexDirection: 'column', gap: 10 }}>
                  <div className="field">
                    <label>Device name</label>
                    <input value={deviceName} onChange={(e) => setDeviceName(e.target.value)} placeholder="My iPhone" autoFocus required />
                  </div>
                  <div className="field">
                    <label>Device type</label>
                    <select value={platform} onChange={(e) => setPlatform(e.target.value)}>
                      {PLATFORMS.map((p) => (
                        <option key={p.value} value={p.value}>{p.label}</option>
                      ))}
                    </select>
                  </div>
                  <div style={{ display: 'flex', gap: 8 }}>
                    <button className="btn btn-primary" type="submit" disabled={requesting}>
                      {requesting ? 'Sending code…' : 'Send verification code'}
                    </button>
                    <button className="btn btn-ghost" type="button" onClick={() => setFormOpen(false)}>Cancel</button>
                  </div>
                </form>
              )}
              {atLimit && !formOpen && (
                <p className="empty-hint">You've reached your device limit. Remove a device to add another.</p>
              )}
            </div>
          </>
        )}

        {pending && (
          <div className="card" style={{ maxWidth: 420, display: 'flex', flexDirection: 'column', gap: 12 }}>
            <div className="section-title">
              {pending.action === 'ENROLL' ? `Confirm adding "${pending.deviceName}"` : `Confirm removing "${pending.deviceName}"`}
            </div>
            <p className="empty-hint" style={{ margin: 0 }}>
              We sent a 6-digit code to {pending.maskedEmail}. Enter it below to confirm.
            </p>
            <form onSubmit={onConfirm} style={{ display: 'flex', flexDirection: 'column', gap: 10 }}>
              <div className="field">
                <label>Verification code</label>
                <input
                  value={code}
                  onChange={(e) => setCode(e.target.value)}
                  placeholder="123456"
                  inputMode="numeric"
                  maxLength={6}
                  autoFocus
                  required
                />
              </div>
              {confirmError && <p className="error-text">{confirmError}</p>}
              <div style={{ display: 'flex', gap: 8 }}>
                <button className="btn btn-primary" type="submit" disabled={confirming}>
                  {confirming ? 'Confirming…' : 'Confirm'}
                </button>
                <button className="btn btn-ghost" type="button" onClick={() => { setPending(null); setCode(''); setConfirmError(null); }}>
                  Cancel
                </button>
              </div>
            </form>
          </div>
        )}

        {enrolled && (
          <div className="card" style={{ maxWidth: 460, display: 'flex', flexDirection: 'column', gap: 12, alignItems: 'center' }}>
            <div className="section-title" style={{ alignSelf: 'flex-start' }}>Scan on {enrolled.deviceName}</div>
            <img src={enrolled.qrDataUrl} alt="WireGuard config QR code" width={220} height={220} style={{ borderRadius: 8 }} />
            <p className="empty-hint" style={{ textAlign: 'center' }}>
              Open the WireGuard app, tap "+" → "Scan from QR code". This config is shown only once — save it now if you need it later.
            </p>
            <div style={{ display: 'flex', gap: 8 }}>
              <button className="btn btn-ghost" onClick={downloadConfig}>Download .conf</button>
              <button className="btn btn-ghost" onClick={copyConfig}>Copy config</button>
              <button className="btn btn-ghost" onClick={() => setEnrolled(null)}>Done</button>
            </div>
          </div>
        )}
      </div>
    </>
  );
}
