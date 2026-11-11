import { useEffect, useId, useRef, type ReactNode } from 'react';
import { X } from 'lucide-react';

interface Props {
  title: string;
  /** The one line under the title: what the panel is set to right now. */
  meta?: string;
  open: boolean;
  onClose: () => void;
  children: ReactNode;
  className?: string;
}

/**
 * A panel that opens over the page, so the instrument's face stays one screen while
 * every control still has room to be read and turned. A native dialog is used on
 * purpose: it traps focus, closes on Escape and dims what is behind it without a
 * line of that behaviour being written here.
 */
export function Dialog({ title, meta, open, onClose, children, className = '' }: Props) {
  const node = useRef<HTMLDialogElement>(null);
  const opener = useRef<HTMLElement | null>(null);
  // A panel that closes itself has to say so: the native close event fires either
  // way, and the page would hear its own close as the user's and drop the panel it
  // was opening next.
  const selfClosing = useRef(false);
  const id = useId();

  useEffect(() => {
    const dialog = node.current;
    if (!dialog) return;
    if (open && !dialog.open) {
      selfClosing.current = false;
      opener.current = document.activeElement instanceof HTMLElement ? document.activeElement : null;
      dialog.showModal();
      // The panel itself takes the focus. Left to the browser the first control in the
      // body is focused, which draws a focus ring on the close button the moment the
      // panel opens, as if it had just been clicked.
      dialog.focus();
    } else if (!open && dialog.open) {
      selfClosing.current = true;
      dialog.close();
    }
  }, [open]);

  return <dialog
    ref={node}
    tabIndex={-1}
    className={`synth-dialog ${className}`}
    aria-labelledby={id}
    // Closing returns the focus to the control that opened the panel, which is what
    // a keyboard user expects after a dialog that is not a page of its own.
    onClose={() => {
      if (selfClosing.current) { selfClosing.current = false; return; }
      onClose();
      opener.current?.focus();
    }}
    onClick={event => { if (event.target === event.currentTarget) onClose(); }}
  >
    <div className="synth-dialog-head">
      <div className="synth-dialog-title">
        <h2 id={id}>{title}</h2>
        {meta && <p className="synth-dialog-meta mono">{meta}</p>}
      </div>
      <button type="button" className="icon-button" aria-label={`Close ${title}`} onClick={onClose}>
        <X size={15} aria-hidden="true" />
      </button>
    </div>
    <div className="synth-dialog-body">{children}</div>
  </dialog>;
}
