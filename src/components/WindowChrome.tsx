import { useEffect, useRef, useState } from 'react';
import { invoke, isTauri } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { Copy, Minus, Square, X } from 'lucide-react';

export function WindowChrome() {
  const native = isTauri();
  const [maximized, setMaximized] = useState(false);
  const [error, setError] = useState('');
  const choice = useRef<HTMLDialogElement | null>(null);
  const closeButton = useRef<HTMLButtonElement | null>(null);

  useEffect(() => {
    if (!native) return;
    const current = getCurrentWindow();
    let active = true;
    let unlisten: (() => void) | undefined;
    const update = async () => {
      try { const value = await current.isMaximized(); if (active) setMaximized(value); }
      catch (reason) { if (active) setError(String(reason)); }
    };
    void update();
    void current.onResized(() => void update()).then(stop => { if (active) unlisten = stop; else stop(); }).catch(reason => { if (active) setError(String(reason)); });
    return () => { active = false; unlisten?.(); };
  }, [native]);

  // A close press is a question, not an order: the engine holds the close and
  // sends it here, so the window, Alt+F4 and the taskbar all ask the same thing.
  // It arrives again if the press is repeated while the dialog is already open.
  useEffect(() => {
    if (!native) return;
    let active = true;
    let stop: (() => void) | undefined;
    listen('audiorium:close-requested', () => {
      if (active && !choice.current?.open) choice.current?.showModal();
    })
      .then(unlisten => { if (active) stop = unlisten; else unlisten(); })
      .catch(reason => { if (active) setError(String(reason)); });
    return () => { active = false; stop?.(); };
  }, [native]);

  const act = async (action: 'minimize' | 'toggleMaximize' | 'close') => {
    try { setError(''); await getCurrentWindow()[action](); }
    catch (reason) { setError(String(reason)); }
  };

  /** Both answers are the engine's own commands, so the tray menu and this dialog
   * quit the same way. */
  const answer = async (what: 'tray' | 'quit') => {
    choice.current?.close();
    try { setError(''); await invoke(what === 'tray' ? 'hide_to_tray' : 'quit_audiorium'); }
    catch (reason) { setError(String(reason)); }
  };

  return <header className="window-chrome" aria-label="Application title bar">
    <div className="window-drag" onMouseDown={event => {
      if (!native || event.button !== 0) return;
      if (event.detail === 2) { void act('toggleMaximize'); return; }
      void getCurrentWindow().startDragging().catch(reason => setError(String(reason)));
    }} title={native ? 'Drag to move · double-click to maximize' : undefined} />
    {error && <span className="window-error" role="alert" title={error}>{error}</span>}
    {native ? <div className="window-controls"><button aria-label="Minimize window" title="Minimize" onClick={() => void act('minimize')}><Minus size={14} /></button><button aria-label={maximized ? 'Restore window' : 'Maximize window'} title={maximized ? 'Restore' : 'Maximize'} onClick={() => void act('toggleMaximize')}>{maximized ? <Copy size={12} /> : <Square size={12} />}</button><button ref={closeButton} aria-label="Close window" title="Close" onClick={() => void act('close')}><X size={16} /></button></div> : <span className="preview-chrome-label">Browser preview</span>}
    <dialog
      ref={choice}
      className="close-choice glass-panel"
      aria-labelledby="close-choice-title"
      aria-describedby="close-choice-note"
      onClose={() => closeButton.current?.focus()}
    >
      <h2 id="close-choice-title">Close AUDIORIUM?</h2>
      <p id="close-choice-note" className="close-choice-note">The tray keeps it playing; closing quits the app and its output.</p>
      <div className="close-choice-actions">
        <button className="page-button" onClick={() => void answer('tray')}>Minimize to tray</button>
        <button className="page-button" onClick={() => void answer('quit')}>Close app</button>
        <button className="page-button ghost" onClick={() => choice.current?.close()}>Cancel</button>
      </div>
    </dialog>
  </header>;
}
