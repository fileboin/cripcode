/**
 * Sizing control (width / height / max-width / min-height). A single free-form
 * field that accepts a Tailwind keyword (`full`, `screen`, `auto`), a fraction
 * (`1/2`), a scale step (`64`), or any CSS length (`480px`, `clamp(…)` → `w-[…]`),
 * with a datalist of common presets for discoverability. Always prefers the named
 * token, falling back to an arbitrary value only when off-scale.
 */

import { useId, useState, type KeyboardEvent as ReactKeyboardEvent } from 'react';
import { ResettableLabel } from './ResettableLabel';
import {
  lengthValue,
  parseLengthInput,
  lengthResetSpec,
  readLayer,
  LENGTH_PRESETS,
  type LayerContext,
  type ResetSpec,
} from '../../lib/edit';

interface Props {
  label: string;
  prefix: string;
  css: string;
  currentClass: string;
  layer: LayerContext;
  onApplyEnum: (token: string, style: Record<string, string>) => void;
  onReset: (spec: ResetSpec) => void;
}

export function LengthControl({
  label,
  prefix,
  css,
  currentClass,
  layer,
  onApplyEnum,
  onReset,
}: Props) {
  const { value, definedAt } = readLayer(currentClass, layer, (s) => lengthValue(s, prefix));
  const display = value ?? '';
  const listId = useId();

  const [text, setText] = useState(display);
  const [lastDisplay, setLastDisplay] = useState(display);
  const [invalid, setInvalid] = useState(false);
  // Sync the field when the value changes externally (reselect, breakpoint switch).
  if (display !== lastDisplay && !invalid) {
    setLastDisplay(display);
    setText(display);
  }

  const commit = () => {
    if (text.trim() === '') return true; // empty = leave unset (no-op)
    const parsed = parseLengthInput(text, prefix, css);
    if (parsed.kind === 'invalid') {
      setInvalid(true);
      return false;
    }
    setInvalid(false);
    onApplyEnum(parsed.token, { [css]: parsed.css });
    return true;
  };

  /** ArrowUp/Down step a numeric value (bare scale step or number+unit, keeping
   *  the unit) and commit each press; Shift ×10, Alt fine (÷10 on unit values —
   *  the Tailwind scale stays on whole steps). Keywords (`full`, `auto`) and
   *  fractions (`1/2`) aren't steppable — the caret is left alone. */
  const onArrowStep = (e: ReactKeyboardEvent<HTMLInputElement>) => {
    const m = /^(-?\d*\.?\d+)\s*([a-z%]*)$/i.exec(text.trim());
    if (!m) return; // non-numeric — leave the caret alone
    const unit = m[2];
    const fine = unit ? 0.1 : 1;
    const step = e.shiftKey ? 10 : e.altKey ? fine : 1;
    const dir = e.key === 'ArrowUp' ? 1 : -1;
    const num = Math.max(0, Math.round((parseFloat(m[1]) + dir * step) * 100) / 100);
    const next = `${num}${unit}`;
    const parsed = parseLengthInput(next, prefix, css);
    if (parsed.kind === 'invalid') return;
    e.preventDefault();
    setText(next);
    setInvalid(false);
    onApplyEnum(parsed.token, { [css]: parsed.css });
  };

  return (
    <div className="ss-edit-panel__control">
      <ResettableLabel
        label={label}
        definedAt={definedAt}
        active={layer.bp}
        onReset={() => onReset(lengthResetSpec(prefix, css))}
      />
      <input
        className={`ss-edit-panel__text${invalid ? ' ss-edit-panel__num--invalid' : ''}`}
        inputMode="text"
        autoCorrect="off"
        autoCapitalize="off"
        spellCheck={false}
        list={listId}
        aria-label={label}
        aria-invalid={invalid}
        placeholder="auto"
        title={
          invalid ? 'Use a keyword (full, auto), fraction (1/2), or length (480px, 50%)' : label
        }
        value={text}
        onChange={(e) => {
          setText(e.target.value);
          if (invalid) setInvalid(false);
        }}
        onFocus={(e) => e.target.select()}
        onBlur={() => {
          if (!commit()) {
            setText(display);
            setInvalid(false);
          }
        }}
        onKeyDown={(e) => {
          if (e.key === 'Enter' && commit()) e.currentTarget.blur();
          else if (e.key === 'ArrowUp' || e.key === 'ArrowDown') onArrowStep(e);
        }}
      />
      <datalist id={listId}>
        {LENGTH_PRESETS.map((p) => (
          <option key={p} value={p} />
        ))}
      </datalist>
    </div>
  );
}
