import { useId, type InputHTMLAttributes } from "react";

import styles from "./Input.module.css";

export interface InputProps extends InputHTMLAttributes<HTMLInputElement> {
  label?: string;
  hint?: string;
  error?: string;
  mono?: boolean;
}

export function Input({ label, hint, error, mono = false, id, className, ...rest }: InputProps) {
  const autoId = useId();
  const inputId = id ?? autoId;
  const classes = [styles.input, mono ? styles.mono : "", error ? styles.error : "", className ?? ""]
    .filter(Boolean)
    .join(" ");

  return (
    <label className={styles.field} htmlFor={label ? inputId : undefined}>
      {label && <span className={styles.label}>{label}</span>}
      <input id={inputId} className={classes} {...rest} />
      {(error ?? hint) && (
        <span className={`${styles.hint} ${error ? styles.error : ""}`}>{error ?? hint}</span>
      )}
    </label>
  );
}
