import React, { useEffect, useRef, useState } from 'react';
import { api } from '../api.js';
import TitleBar from './TitleBar.jsx';
import { FileServerIcon } from './icons.jsx';

function formatBytes(bytes) {
  if (!bytes) return '0 B';
  const units = ['B', 'KB', 'MB', 'GB', 'TB'];
  const i = Math.floor(Math.log(bytes) / Math.log(1024));
  return `${(bytes / Math.pow(1024, i)).toFixed(i === 0 ? 0 : 1)} ${units[i]}`;
}

function formatDate(iso) {
  return new Date(iso).toLocaleDateString(undefined, { year: 'numeric', month: 'short', day: 'numeric' });
}

function readFileAsBase64(file) {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => resolve(String(reader.result).split(',')[1] ?? '');
    reader.onerror = () => reject(reader.error);
    reader.readAsDataURL(file);
  });
}

function base64ToBlob(base64, contentType) {
  const byteChars = atob(base64);
  const byteNumbers = new Array(byteChars.length);
  for (let i = 0; i < byteChars.length; i++) byteNumbers[i] = byteChars.charCodeAt(i);
  return new Blob([new Uint8Array(byteNumbers)], { type: contentType || 'application/octet-stream' });
}

export default function FileServerPage() {
  const [path, setPath] = useState('/');
  const [listing, setListing] = useState(null);
  const [usage, setUsage] = useState(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState(null);
  const [notProvisioned, setNotProvisioned] = useState(false);

  const [newFolderOpen, setNewFolderOpen] = useState(false);
  const [newFolderName, setNewFolderName] = useState('');
  const [uploadingName, setUploadingName] = useState(null);
  const [downloadingId, setDownloadingId] = useState(null);
  const [armedDelete, setArmedDelete] = useState(null);

  const fileInputRef = useRef(null);

  async function refresh(targetPath) {
    setError(null);
    setNotProvisioned(false);
    try {
      const [folderListing, usageResult] = await Promise.all([api.listFiles(targetPath), api.getFileUsage()]);
      setListing(folderListing);
      setUsage(usageResult);
    } catch (err) {
      const message = err?.message || String(err);
      if (message.includes('No file storage')) setNotProvisioned(true);
      else setError(message);
    } finally {
      setLoading(false);
    }
  }

  useEffect(() => {
    setLoading(true);
    setArmedDelete(null);
    refresh(path);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [path]);

  function enterFolder(name) {
    setPath((p) => (p.endsWith('/') ? `${p}${name}/` : `${p}/${name}/`));
  }

  const breadcrumbs = path.split('/').filter(Boolean);

  function goToBreadcrumb(index) {
    setPath(`/${breadcrumbs.slice(0, index + 1).join('/')}/`);
  }

  async function onCreateFolder(e) {
    e.preventDefault();
    const name = newFolderName.trim();
    if (!name) return;
    const target = path.endsWith('/') ? `${path}${name}/` : `${path}/${name}/`;
    try {
      await api.createFolder(target);
      setNewFolderOpen(false);
      setNewFolderName('');
      setPath(target);
    } catch (err) {
      setError(err?.message || String(err));
    }
  }

  async function onFilePicked(e) {
    const file = e.target.files?.[0];
    e.target.value = '';
    if (!file) return;
    setError(null);
    setUploadingName(file.name);
    try {
      const base64 = await readFileAsBase64(file);
      await api.uploadFile(path, file.name, file.type || 'application/octet-stream', base64);
      await refresh(path);
    } catch (err) {
      setError(err?.message || String(err));
    } finally {
      setUploadingName(null);
    }
  }

  function armOrRun(key, action) {
    if (armedDelete === key) {
      setArmedDelete(null);
      action();
    } else {
      setArmedDelete(key);
    }
  }

  async function onDeleteFolder(name) {
    const target = path.endsWith('/') ? `${path}${name}/` : `${path}/${name}/`;
    try {
      await api.deleteFolder(target);
      await refresh(path);
    } catch (err) {
      setError(err?.message || String(err));
    }
  }

  async function onDeleteFile(file) {
    try {
      await api.deleteFile(file.id);
      await refresh(path);
    } catch (err) {
      setError(err?.message || String(err));
    }
  }

  async function onDownload(file) {
    setDownloadingId(file.id);
    setError(null);
    try {
      const base64 = await api.downloadFile(file.id);
      const blob = base64ToBlob(base64, file.contentType);
      const url = URL.createObjectURL(blob);
      const a = document.createElement('a');
      a.href = url;
      a.download = file.name;
      a.click();
      URL.revokeObjectURL(url);
    } catch (err) {
      setError(err?.message || String(err));
    } finally {
      setDownloadingId(null);
    }
  }

  const usedPercent = usage && usage.quotaBytes > 0 ? Math.min(100, (usage.usedBytes / usage.quotaBytes) * 100) : 0;

  return (
    <>
      <TitleBar />
      <div className="page">
        <div className="page-header">
          <h2>Files</h2>
          <p>Your Tayyem file storage — browse, upload, and download, right from the app.</p>
        </div>

        {notProvisioned ? (
          <div className="placeholder-card">
            <div className="icon"><FileServerIcon /></div>
            <h3>File storage isn't set up yet</h3>
            <p>Your account doesn't have file storage access yet. Contact an admin, or check back shortly.</p>
          </div>
        ) : (
          <>
            {usage && (
              <div className="card" style={{ display: 'flex', flexDirection: 'column', gap: 8 }}>
                <div style={{ display: 'flex', justifyContent: 'space-between', fontSize: 12.5 }}>
                  <span style={{ fontWeight: 700 }}>Storage used</span>
                  <span style={{ color: 'var(--text-faint)' }}>
                    {formatBytes(usage.usedBytes)} of {formatBytes(usage.quotaBytes)}
                  </span>
                </div>
                <div style={{ height: 6, borderRadius: 999, background: 'var(--surface-2)', overflow: 'hidden' }}>
                  <div
                    style={{
                      height: '100%',
                      width: `${usedPercent}%`,
                      background: usedPercent > 90 ? 'var(--danger)' : 'var(--accent)',
                    }}
                  />
                </div>
              </div>
            )}

            <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', gap: 10, flexWrap: 'wrap' }}>
              <div style={{ display: 'flex', alignItems: 'center', gap: 6, fontSize: 13 }}>
                <button
                  type="button"
                  className="btn-ghost"
                  style={{ background: 'none', border: 'none', padding: 0, fontWeight: 700, cursor: 'pointer', color: breadcrumbs.length === 0 ? 'var(--text)' : 'var(--accent)' }}
                  onClick={() => setPath('/')}
                >
                  Home
                </button>
                {breadcrumbs.map((segment, i) => (
                  <span key={i} style={{ display: 'flex', alignItems: 'center', gap: 6 }}>
                    <span style={{ color: 'var(--text-faint)' }}>/</span>
                    <button
                      type="button"
                      style={{ background: 'none', border: 'none', padding: 0, fontWeight: 700, cursor: 'pointer', color: i === breadcrumbs.length - 1 ? 'var(--text)' : 'var(--accent)' }}
                      onClick={() => goToBreadcrumb(i)}
                    >
                      {segment}
                    </button>
                  </span>
                ))}
              </div>

              <div style={{ display: 'flex', gap: 8 }}>
                <button className="btn btn-ghost" type="button" onClick={() => setNewFolderOpen((v) => !v)}>
                  + New folder
                </button>
                <button className="btn btn-primary" type="button" onClick={() => fileInputRef.current?.click()} disabled={uploadingName != null}>
                  {uploadingName ? `Uploading ${uploadingName}…` : 'Upload'}
                </button>
                <input ref={fileInputRef} type="file" onChange={onFilePicked} style={{ display: 'none' }} />
              </div>
            </div>

            {newFolderOpen && (
              <form onSubmit={onCreateFolder} style={{ display: 'flex', gap: 8 }}>
                <input
                  autoFocus
                  value={newFolderName}
                  onChange={(e) => setNewFolderName(e.target.value)}
                  placeholder="Folder name"
                  style={{ flex: 1 }}
                />
                <button className="btn btn-primary" type="submit">Create</button>
                <button className="btn btn-ghost" type="button" onClick={() => { setNewFolderOpen(false); setNewFolderName(''); }}>
                  Cancel
                </button>
              </form>
            )}

            {error && <p className="error-text">{error}</p>}

            {loading ? (
              <p className="empty-hint">Loading…</p>
            ) : (
              <div className="list">
                {listing?.subfolders.length === 0 && listing?.files.length === 0 && (
                  <p className="empty-hint">This folder is empty.</p>
                )}
                {listing?.subfolders.map((name) => {
                  const key = `folder:${name}`;
                  return (
                    <div className="list-row" key={key}>
                      <button
                        type="button"
                        className="pickable"
                        style={{ flex: 1, display: 'flex', alignItems: 'center', gap: 10, background: 'none', border: 'none', textAlign: 'left', cursor: 'pointer', padding: 0 }}
                        onClick={() => enterFolder(name)}
                      >
                        <span style={{ fontWeight: 700 }}>📁 {name}</span>
                      </button>
                      <button onClick={() => armOrRun(key, () => onDeleteFolder(name))} title={armedDelete === key ? 'Click again to confirm' : 'Delete folder'}>
                        {armedDelete === key ? 'Confirm?' : '×'}
                      </button>
                    </div>
                  );
                })}
                {listing?.files.map((f) => {
                  const key = `file:${f.id}`;
                  return (
                    <div className="list-row" key={key}>
                      <div style={{ flex: 1, minWidth: 0 }}>
                        <div style={{ fontWeight: 700, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>{f.name}</div>
                        <div style={{ fontSize: 11.5, color: 'var(--text-faint)' }}>
                          {formatBytes(f.sizeBytes)} · {formatDate(f.createdAt)}
                        </div>
                      </div>
                      <div style={{ display: 'flex', gap: 6 }}>
                        <button onClick={() => onDownload(f)} disabled={downloadingId === f.id} title="Download">
                          {downloadingId === f.id ? '…' : '⬇'}
                        </button>
                        <button onClick={() => armOrRun(key, () => onDeleteFile(f))} title={armedDelete === key ? 'Click again to confirm' : 'Delete'}>
                          {armedDelete === key ? 'Confirm?' : '×'}
                        </button>
                      </div>
                    </div>
                  );
                })}
              </div>
            )}
          </>
        )}
      </div>
    </>
  );
}
