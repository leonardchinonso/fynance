/**
 * Shared visual style for form controls in dialogs (native <select>, <textarea>,
 * custom dropdown triggers) so they match the <Input> component — same border,
 * radius, and background in both themes. Keeps every dialog field consistent.
 */
export const DIALOG_FIELD_CLASS =
  "w-full rounded-lg border border-input bg-transparent px-2.5 py-1.5 text-sm outline-none transition-colors focus-visible:border-ring focus-visible:ring-3 focus-visible:ring-ring/50 dark:bg-input/30"

/** Standard gap between a field label and its control. */
export const FIELD_GAP = "mt-1.5"
