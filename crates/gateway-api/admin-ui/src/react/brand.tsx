/** Relayna's gateway stem and outgoing relay form one compact R. */
export function RelaynaMark() {
  return (
    <svg
      className="relayna-mark"
      viewBox="0 0 48 48"
      width="48"
      height="48"
      aria-hidden="true"
      focusable="false"
    >
      <path
        className="relayna-mark-body"
        d="M8 40V13a6 6 0 0 1 6-6h11a14 14 0 0 1 0 28h-5v-8h5a6 6 0 0 0 0-12h-9v25Z"
      />
      <path className="relayna-mark-relay" d="M20 31h10l11 10H30Z" />
    </svg>
  );
}
