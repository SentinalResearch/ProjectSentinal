const KEYS = [
  ["Ctrl-C", [3]],
  ["Esc", [27]],
  ["Tab", [9]],
  ["↑", [27, 91, 65]],
  ["↓", [27, 91, 66]],
  ["y", [121]],
  ["n", [110]],
] as const;

/** Touch-friendly terminal keys that are awkward on mobile keyboards. */
export function KeyBar({ onBytes }: { onBytes: (bytes: Uint8Array) => void }) {
  return (
    <div className="key-bar" aria-label="Terminal keys">
      {KEYS.map(([label, bytes]) => (
        <button
          key={label}
          type="button"
          onClick={() => onBytes(new Uint8Array(bytes))}
        >
          {label}
        </button>
      ))}
    </div>
  );
}
