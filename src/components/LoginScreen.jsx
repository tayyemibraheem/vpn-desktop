import React, { useState } from 'react';
import { api } from '../api.js';
import TitleBar from './TitleBar.jsx';

const METHOD_LABELS = { TOTP: 'Authenticator app', PIN: 'PIN code', EMAIL: 'Email code' };

export default function LoginScreen({ onLoggedIn }) {
  const [usernameOrEmail, setUsernameOrEmail] = useState('');
  const [password, setPassword] = useState('');
  const [remember, setRemember] = useState(true);
  const [error, setError] = useState(null);
  const [submitting, setSubmitting] = useState(false);

  // Set once auth_login comes back asking for a second factor; cleared once verified.
  const [challenge, setChallenge] = useState(null);
  const [selectedMethod, setSelectedMethod] = useState(null);
  const [code, setCode] = useState('');
  const [emailSent, setEmailSent] = useState(false);

  async function onSubmit(e) {
    e.preventDefault();
    setError(null);
    setSubmitting(true);
    try {
      const result = await api.login(usernameOrEmail, password, remember);
      if (result.mfaRequired) {
        setChallenge({ challengeToken: result.mfaChallengeToken, methods: result.mfaMethods });
        setSelectedMethod(result.mfaMethods.length === 1 ? result.mfaMethods[0] : null);
        return;
      }
      if (!result.ok) {
        setError(result.error);
        return;
      }
      onLoggedIn({ username: result.username, email: result.email });
    } finally {
      setSubmitting(false);
    }
  }

  async function onSendEmailCode() {
    setError(null);
    setSubmitting(true);
    try {
      const result = await api.sendMfaEmailCode(challenge.challengeToken);
      if (!result.ok) {
        setError(result.error);
        return;
      }
      setEmailSent(true);
    } finally {
      setSubmitting(false);
    }
  }

  async function onVerify(e) {
    e.preventDefault();
    setError(null);
    setSubmitting(true);
    try {
      const result = await api.verifyMfa(challenge.challengeToken, selectedMethod, code, remember);
      if (!result.ok) {
        setError(result.error);
        return;
      }
      onLoggedIn({ username: result.username, email: result.email });
    } finally {
      setSubmitting(false);
    }
  }

  if (challenge) {
    return (
      <div className="login-shell">
        <TitleBar />
        <form className="login-form card" onSubmit={selectedMethod ? onVerify : (e) => e.preventDefault()}>
          <div className="login-brand">
            <div className="sidebar-brand-mark">T</div>
            <h1>Tayyem<span>VPN</span></h1>
          </div>
          <p style={{ fontSize: 13, color: 'var(--text-muted)', margin: 0 }}>Verify it's you to finish signing in.</p>

          {!selectedMethod ? (
            <div style={{ display: 'flex', flexDirection: 'column', gap: 8 }}>
              {challenge.methods.map((m) => (
                <button
                  key={m}
                  type="button"
                  className="btn"
                  onClick={() => setSelectedMethod(m)}
                >
                  {METHOD_LABELS[m] || m}
                </button>
              ))}
            </div>
          ) : (
            <>
              <p style={{ fontSize: 12.5, color: 'var(--text-muted)', margin: 0 }}>
                {selectedMethod === 'TOTP' && 'Enter the code from your authenticator app.'}
                {selectedMethod === 'PIN' && 'Enter your PIN.'}
                {selectedMethod === 'EMAIL' && "We'll email you a code."}
              </p>

              {selectedMethod === 'EMAIL' && !emailSent ? (
                <button type="button" className="btn btn-primary" disabled={submitting} onClick={onSendEmailCode}>
                  {submitting ? 'Sending…' : 'Send code'}
                </button>
              ) : (
                <>
                  <div className="field">
                    <label>Code</label>
                    <input
                      type={selectedMethod === 'PIN' ? 'password' : 'text'}
                      inputMode="numeric"
                      value={code}
                      onChange={(e) => setCode(e.target.value.replace(/\D/g, '').slice(0, 8))}
                      autoFocus
                      required
                    />
                  </div>
                  <button className="btn btn-primary" type="submit" disabled={submitting || code.length < 4}>
                    {submitting ? 'Verifying…' : 'Verify'}
                  </button>
                </>
              )}

              {error && <p className="error-text">{error}</p>}

              {challenge.methods.length > 1 && (
                <button
                  type="button"
                  className="link-button"
                  onClick={() => {
                    setSelectedMethod(null);
                    setCode('');
                    setEmailSent(false);
                    setError(null);
                  }}
                >
                  Use a different method
                </button>
              )}
            </>
          )}

          {selectedMethod === null && error && <p className="error-text">{error}</p>}
        </form>
      </div>
    );
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
