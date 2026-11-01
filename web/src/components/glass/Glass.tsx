import type { ComponentPropsWithoutRef, ElementType, ReactNode } from "react";

import type { SessionState } from "../../protocol/SessionState";
import styles from "./Glass.module.css";

type GlassProps<T extends ElementType> = {
  as?: T;
  className?: string;
  children?: ReactNode;
} & Omit<ComponentPropsWithoutRef<T>, "as" | "className" | "children">;

function join(...names: Array<string | false | undefined>): string {
  return names.filter(Boolean).join(" ");
}

/** Floating glass chrome (island, prompt): strongest blur, 24px radius. */
export function GlassPill<T extends ElementType = "div">({
  as,
  className,
  strong,
  ...rest
}: GlassProps<T> & { strong?: boolean }) {
  const Tag = (as ?? "div") as ElementType;
  return <Tag className={join(styles.pill, strong && styles.strong, className)} {...rest} />;
}

/** A glass panel (machine lane, feed, metric tile). */
export function GlassPanel<T extends ElementType = "section">({
  as,
  className,
  tile,
  ...rest
}: GlassProps<T> & { tile?: boolean }) {
  const Tag = (as ?? "section") as ElementType;
  return <Tag className={join(styles.panel, tile && styles.tile, className)} {...rest} />;
}

/** A card tinted with its Session state's colour; never blurred. */
export function AttentionCard<T extends ElementType = "article">({
  as,
  className,
  state,
  ...rest
}: GlassProps<T> & { state: SessionState }) {
  const Tag = (as ?? "article") as ElementType;
  return <Tag className={join(styles.attention, className)} data-state={state} {...rest} />;
}
