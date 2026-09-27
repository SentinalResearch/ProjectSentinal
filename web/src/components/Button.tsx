import type { ButtonHTMLAttributes } from "react";

import styles from "./Button.module.css";

export interface ButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  variant?: "primary" | "secondary" | "destructive";
  loading?: boolean;
  loadingLabel?: string;
}

/**
 * Primary = accent fill (one per view for the forward action).
 * Secondary = 1.5px outline.
 * Destructive = red fill, only inside a confirm dialog — see ConfirmDialog.
 */
export function Button({
  variant = "secondary",
  loading = false,
  loadingLabel,
  disabled,
  className,
  children,
  ...rest
}: ButtonProps) {
  const classes = [
    styles.btn,
    styles[variant],
    loading ? styles.loading : "",
    className ?? "",
  ]
    .filter(Boolean)
    .join(" ");

  return (
    <button type="button" className={classes} disabled={disabled ?? loading} {...rest}>
      {loading && <span className={`${styles.spinner} tg-motion`} aria-hidden="true" />}
      {loading ? (loadingLabel ?? children) : children}
    </button>
  );
}
