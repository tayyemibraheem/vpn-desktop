import React, { useState } from 'react';
import { api } from '../api.js';
import TitleBar from './TitleBar.jsx';

export default function LoginScreen({ onLoggedIn }) {
  const [usernameOrEmail, setUsernameOrEmail] = useState('');
  const [password, setPassword] = useState('');
  const [remember, setRemember] = useState(true);
  const [error, setError] = useState(null);
  const [submitting, setSubmitting] = useState(false);

  async function onSubmit(e) {
    e.preventDefault();
    setError(null);
    setSubmitting(true);
    try {
      const result = await api.login(usernameOrEmail, password, remember);
      if (!result.ok) {
        setError(result.error);
        return;
      }
      onLoggedIn({ username: result.username, email: result.email });
    } finally {
      setSubmitting(false);
    }
  }

  return (
    <div className="login-shell">
      <TitleBar />
      <form className="login-form card" onSubmit={onSubmit}>
        <div className="login-brand">
          <div className="sidebar-brand-mark">T</div>
          <h1>Tayyem<span>VPN</span></h1>
        </div>
        <div className="field">
          <label>Username or email</label>
          <input value={usernameOrEmail} onChange={(e) => setUsernameOrEmail(e.target.value)} autoFocus required />
        </div>
        <div className="field">
          <label>Password</label>
          <input type="password" value={password} onChange={(e) => setPassword(e.target.value)} required />
        </div>
        <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center' }}>
          <label style={{ display: 'flex', alignItems: 'center', gap: 6, fontSize: 12.5, color: 'var(--text-muted)', cursor: 'pointer' }}>
            <input type="checkbox" checked={remember} onChange={(e) => setRemember(e.target.checked)} style={{ width: 'auto' }} />
            Keep me logged in
          </label>
          <button type="button" className="link-button" onClick={() => api.openForgotPassword()}>
            Forgot password?
          </button>
        </div>
        {error && <p className="error-text">{error}</p>}
        <button className="btn btn-primary" type="submit" disabled={submitting}>
          {submitting ? 'Signing in…' : 'Sign in'}
        </button>
      </form>
    </div>
  );
}
