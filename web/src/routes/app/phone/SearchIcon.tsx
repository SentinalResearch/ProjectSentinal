/** A square-capped magnifier; the ⌕ character renders too small in the UI font. */
export function SearchIcon({ size = 20 }: { size?: number }) {
  return (
    <svg width={size} height={size} viewBox="0 0 20 20" fill="none" aria-hidden="true">
      <circle cx="8.5" cy="8.5" r="5.75" stroke="currentColor" strokeWidth="2" />
      <path d="M12.8 12.8L17.5 17.5" stroke="currentColor" strokeWidth="2" strokeLinecap="square" />
    </svg>
  );
}
