import { useState, type FormEvent } from "react";

import { Input } from "../../components/Input";
import { useAuth } from "../../context/AuthContext";
import { config } from "../../data/config";
import { AuthLayout } from "./AuthLayout";
import { AuthMark } from "./AuthMark";
import styles from "./SetupPage.module.css";

const FOOT = "Stored as an Argon2id hash on this machine. Keep the machine token safe for initial setup.";

export function SetupPage() {
  const { setup } = useAuth();
  const [bootstrapToken, setBootstrapToken] = useState("");
  const [pw, setPw] = useState("");
  const [pw2, setPw2] = useState("");
  const [busy, setBusy] = useState(false);
  const [ok, setOk] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const meetsLength = pw.length >= config.setupMinPasswordLength;
  const matches = pw.length > 0 && pw === pw2;
  const valid = bootstrapToken.length > 0 && meetsLength && matches;

  async function handleSubmit(e: FormEvent) {
    e.preventDefault();
    if (!valid || busy || ok) return;
    setBusy(true);
    const result = await setup(bootstrapToken, pw);
    setBusy(false);
    if (result.status === "signedIn") {
      setOk(true);
    } else {
      setError(result.message ?? "Could not configure the owner password.");
    }
  }

  return (
    <AuthLayout mode="setup" mark={<AuthMark mode="setup" busy={busy} ok={ok} />} foot={FOOT} hideOverflow={ok}>
      <form className={styles.form} onSubmit={handleSubmit}>
        <Input
          label="Machine token"
          type="password"
          value={bootstrapToken}
          onChange={(e) => {
            setBootstrapToken(e.target.value);
            setError(null);
          }}
          autoFocus
          autoComplete="off"
        />
        <Input
          label="New password"
          type="password"
          value={pw}
          onChange={(e) => setPw(e.target.value)}
          autoComplete="new-password"
        />
        <Input
          label="Confirm password"
          type="password"
          value={pw2}
          onChange={(e) => setPw2(e.target.value)}
          autoComplete="new-password"
        />
        <div className={styles.rules}>
          <span className={styles.rule} style={{ color: meetsLength ? "var(--s-running-fg)" : "var(--text-3)" }}>
            <span className={styles.ruleGlyph}>{meetsLength ? "✓" : "·"}</span>
            At least {config.setupMinPasswordLength} characters
          </span>
          <span className={styles.rule} style={{ color: matches ? "var(--s-running-fg)" : "var(--text-3)" }}>
            <span className={styles.ruleGlyph}>{matches ? "✓" : "·"}</span>
            Both fields match
          </span>
        </div>
        {error && (
          <span role="alert" className={styles.error}>
            {error}
          </span>
        )}
        <button
          type="submit"
          disabled={!valid}
          className={styles.submit}
          style={{
            background: valid ? "var(--accent)" : "var(--bg-2)",
            color: valid ? "var(--on-accent)" : "var(--text-3)",
          }}
        >
          {busy ? "Checking…" : "Create password and continue"}
        </button>
      </form>
    </AuthLayout>
  );
}
