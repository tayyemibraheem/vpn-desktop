import React, { useEffect, useState } from 'react';
import { api } from './api.js';
import LoginScreen from './components/LoginScreen.jsx';
import Sidebar from './components/Sidebar.jsx';
import HomePage from './components/HomePage.jsx';
import ServersPage from './components/ServersPage.jsx';
import SplitTunnelingPage from './components/SplitTunnelingPage.jsx';
import DevicesPage from './components/DevicesPage.jsx';
import FileServerPage from './components/FileServerPage.jsx';
import SettingsPage from './components/SettingsPage.jsx';

export default function App() {
  const [session, setSession] = useState(null);
  const [restoring, setRestoring] = useState(true);
  const [page, setPage] = useState('home');
  const [status, setStatus] = useState({ state: 'disconnected', detail: null });

  // On launch, only a session saved with "keep me logged in" checked comes back here — the
  // backend transparently refreshes an expired token, or reports not-ok if that also fails.
  useEffect(() => {
    api
      .restoreSession()
      .then((result) => {
        if (result?.ok) setSession({ username: result.username, email: result.email });
      })
      .finally(() => setRestoring(false));
  }, []);

  useEffect(() => {
    if (!session) return;
    api.getStatus().then(setStatus);
    let unlisten;
    api.onStatus((s) => setStatus(s)).then((fn) => (unlisten = fn));
    return () => unlisten && unlisten();
  }, [session]);

  if (restoring) {
    return <div className="app-shell loading" />;
  }

  if (!session) {
    return <LoginScreen onLoggedIn={setSession} />;
  }

  const pages = {
    home: <HomePage session={session} status={status} />,
    servers: <ServersPage status={status} />,
    split: <SplitTunnelingPage connected={status.state === 'connected'} />,
    devices: <DevicesPage />,
    files: <FileServerPage />,
    settings: (
      <SettingsPage
        session={session}
        onLogout={async () => {
          await api.disconnect();
          await api.logout();
          setSession(null);
        }}
      />
    ),
  };

  return (
    <div className="app-shell">
      <Sidebar page={page} onNavigate={setPage} session={session} />
      <main className="content">{pages[page]}</main>
    </div>
  );
}
