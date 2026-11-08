/** An on/off setting with its name and a line of help; the whole row toggles it. */
export function Switch({
  checked,
  label,
  help,
  disabled = false,
  onChange,
}: {
  checked: boolean;
  label: string;
  help: string;
  disabled?: boolean;
  onChange: (checked: boolean) => void;
}) {
  return (
    <button
      className="app-switch"
      type="button"
      role="switch"
      aria-checked={checked}
      disabled={disabled}
      onClick={() => onChange(!checked)}
    >
      <span>
        <strong>{label}</strong>
        <small>{help}</small>
      </span>
      <span className="app-switch-track" aria-hidden="true" />
    </button>
  );
}
