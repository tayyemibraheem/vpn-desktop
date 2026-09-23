import React, { useEffect, useState } from 'react';
import { api } from './api.js';
import LoginScreen from './components/LoginScreen.jsx';
import Sidebar from './components/Sidebar.jsx';
import HomePage from './components/HomePage.jsx';
import ServersPage from './components/ServersPage.jsx';
import SplitTunnelingPage from './components/SplitTunnelingPage.jsx';
import FileServerPage from './components/FileServerPage.jsx';
import SettingsPage from './components/SettingsPage.jsx';

export default function App() {
  // Always starts signed out (null), never auto-restored from a saved session. OpenVPN needs the
  // real password on every connect, not a session token, and passwords are deliberately never
  // written to disk — so a "remembered" login here would have no password to actually connect
  // with, which is exactly the "missing username and password" bug this replaced.
  const [session, setSession] = useState(null);
  const [page, setPage] = useState('home');
  const [status, setStatus] = useState({ state: 'disconnected', detail: null });

  useEffect(() => {
    if (!session) return;
    api.getStatus().then(setStatus);
    let unlisten;
    api.onStatus((s) => setStatus(s)).then((fn) => (unlisten = fn));
    return () => unlisten && unlisten();
  }, [session]);

  if (!session) {
    return <LoginScreen onLoggedIn={setSession} />;
  }

  const pages = {
    home: <HomePage session={session} status={status} />,
    servers: <ServersPage status={status} />,
    split: <SplitTunnelingPage connected={status.state === 'connected'} />,
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
