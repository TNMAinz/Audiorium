import { useCallback, useEffect, useLayoutEffect, useRef, useState } from 'react';
import type { KeyboardEvent as ReactKeyboardEvent, PointerEvent as ReactPointerEvent } from 'react';
import type { FileScan } from '../model';

interface Props {
  scan: FileScan;
  /** "3:04 · 8.8 MB": what the file is, so the picture has a subject. */
  caption: string;
}

/** The loudness plot's floor: below this a curve is flattened, not exaggerated. */
const LOUDNESS_FLOOR = -50;
const WIDTH = 1000;
const WAVE_HEIGHT = 120;
const LOUDNESS_HEIGHT = 100;
/** One spectrogram column on screen. The file is scanned at a resolution worth
 *  reading, so the columns are drawn at a readable width and the picture is moved
 *  sideways instead of being squeezed into one card width. */
const COLUMN_PX = 4;

/**
 * A whole file drawn in three registers: its peak envelope, its loudness as the
 * meter's 3 s window advances, and a coarse spectrogram. The three share one
 * timeline, which is wider than the card and dragged sideways, so a four-minute
 * file can be inspected a few seconds at a time. The figures the master is judged
 * by are not read off this — it is a picture of where the energy and the level sit.
 */
export function FileScanView({ scan, caption }: Props) {
  const spectrogram = useRef<HTMLCanvasElement>(null);
  const track = useRef<HTMLDivElement>(null);
  const held = useRef<{ x: number; left: number } | null>(null);
  const [pan, setPan] = useState({ left: 0, view: 0, total: 1 });

  const sync = useCallback(() => {
    const element = track.current;
    if (!element) return;
    setPan({ left: element.scrollLeft, view: element.clientWidth, total: element.scrollWidth });
  }, []);

  // The rail describes the picture the card actually holds: the view is the card,
  // the total is the timeline's own width, which changes with the scan.
  useLayoutEffect(() => {
    const element = track.current;
    if (!element) return;
    sync();
    const box = new ResizeObserver(sync);
    box.observe(element);
    return () => box.disconnect();
  }, [sync, scan]);

  useEffect(() => {
    const canvas = spectrogram.current;
    if (!canvas || scan.columns < 1 || scan.bands < 1) return;
    const context = canvas.getContext('2d');
    if (!context) return;
    canvas.width = scan.columns;
    canvas.height = scan.bands;
    const image = context.createImageData(scan.columns, scan.bands);
    for (let column = 0; column < scan.columns; column += 1) {
      for (let band = 0; band < scan.bands; band += 1) {
        const value = Math.min(1, Math.max(0, scan.spectrum[column * scan.bands + band] ?? 0));
        // Low bands at the bottom, the way a spectrogram is read.
        const at = ((scan.bands - 1 - band) * scan.columns + column) * 4;
        image.data[at] = 120 + 135 * Math.min(1, value * 1.4);
        image.data[at + 1] = 20 + 190 * Math.max(0, value - 0.2);
        image.data[at + 2] = 30 + 225 * Math.max(0, 0.55 - value);
        image.data[at + 3] = 40 + 215 * value;
      }
    }
    context.putImageData(image, 0, 0);
  }, [scan]);

  const envelope = scan.envelope;
  const middle = WAVE_HEIGHT / 2;
  const reach = middle - 4;
  const at = (index: number) => (index / Math.max(1, envelope.length - 1)) * WIDTH;
  const height = (value: number) => Math.min(1, Math.max(0, value)) * reach;
  // Across the top left to right, then back along the bottom: one closed shape.
  const wave = envelope.length
    ? [
        ...envelope.map((value, index) => `${index === 0 ? 'M' : 'L'}${at(index)},${middle - height(value)}`),
        ...envelope.map((_, index) => `L${at(envelope.length - 1 - index)},${middle + height(envelope[envelope.length - 1 - index])}`),
        'Z',
      ].join(' ')
    : '';

  const loudness = scan.loudness
    .map((value, index) => {
      const x = (index / Math.max(1, scan.loudness.length - 1)) * WIDTH;
      const share = Math.min(1, Math.max(0, (value - LOUDNESS_FLOOR) / (0 - LOUDNESS_FLOOR)));
      return `${index === 0 ? 'M' : 'L'}${x},${LOUDNESS_HEIGHT - share * (LOUDNESS_HEIGHT - 6) - 3}`;
    })
    .join(' ');

  const scrollable = pan.total > pan.view + 1;
  const thumb = Math.max(16, (pan.view / Math.max(1, pan.total)) * 100);
  const thumbLeft = (pan.left / Math.max(1, pan.total)) * 100;

  const startPan = (event: ReactPointerEvent<HTMLDivElement>) => {
    const element = track.current;
    if (!element || !scrollable) return;
    element.setPointerCapture(event.pointerId);
    held.current = { x: event.clientX, left: element.scrollLeft };
  };
  const movePan = (event: ReactPointerEvent<HTMLDivElement>) => {
    const element = track.current;
    if (!element || !held.current) return;
    element.scrollLeft = held.current.left - (event.clientX - held.current.x);
  };
  const endPan = (event: ReactPointerEvent<HTMLDivElement>) => {
    if (held.current && event.currentTarget.hasPointerCapture(event.pointerId)) {
      event.currentTarget.releasePointerCapture(event.pointerId);
    }
    held.current = null;
  };
  // The arrow keys move the picture the way the rail does, and the rail is a
  // mirror of the drawing rather than a control of its own.
  const key = (event: ReactKeyboardEvent<HTMLDivElement>) => {
    const element = track.current;
    if (!element || !scrollable) return;
    const step = Math.max(40, Math.round(element.clientWidth * 0.8));
    if (event.key === 'ArrowRight') { element.scrollLeft += step; event.preventDefault(); }
    if (event.key === 'ArrowLeft') { element.scrollLeft -= step; event.preventDefault(); }
  };

  return <div className="file-scan">
    <div className="file-scan-legend">
      <span className="mono">{caption}</span>
      <span className="file-scan-keys">
        <span>Peaks</span>
        <span>Loudness</span>
        <span>Spectrum</span>
      </span>
    </div>
    <div
      className={`file-scan-track ${scrollable ? 'scrollable' : ''}`}
      ref={track}
      role="group"
      aria-label={`Whole file: peak envelope, loudness and spectrogram over ${scan.seconds.toFixed(1)} seconds${scrollable ? '. Drag sideways or use the arrow keys to inspect it' : ''}`}
      title={scrollable ? 'Drag, shift-wheel or the arrow keys move through the file' : undefined}
      tabIndex={scrollable ? 0 : undefined}
      onScroll={sync}
      onPointerDown={startPan}
      onPointerMove={movePan}
      onPointerUp={endPan}
      onPointerCancel={endPan}
      onKeyDown={key}
    >
      <div className="file-scan-time" style={{ width: `${Math.max(1, scan.columns) * COLUMN_PX}px` }}>
        <div className="file-scan-plot file-scan-wave">
          <svg
            viewBox={`0 0 ${WIDTH} ${WAVE_HEIGHT}`}
            preserveAspectRatio="none"
            role="img"
            aria-label={`Peak envelope over ${scan.seconds.toFixed(1)} seconds`}
          >
            <line x1={0} y1={middle} x2={WIDTH} y2={middle} className="file-scan-axis" />
            {wave && <path d={wave} className="file-scan-wave-fill" />}
          </svg>
        </div>
        <div className="file-scan-plot file-scan-loudness">
          <svg
            viewBox={`0 0 ${WIDTH} ${LOUDNESS_HEIGHT}`}
            preserveAspectRatio="none"
            role="img"
            aria-label="Loudness over time, in LUFS"
          >
            {[1, 2, 3].map(step => <line
              key={step}
              x1={0}
              y1={(LOUDNESS_HEIGHT * step) / 4}
              x2={WIDTH}
              y2={(LOUDNESS_HEIGHT * step) / 4}
              className="file-scan-axis"
            />)}
            {loudness && <path d={loudness} className="file-scan-loudness-line" />}
          </svg>
        </div>
        <div className="file-scan-plot file-scan-spectrum">
          <canvas
            ref={spectrogram}
            role="img"
            aria-label={`Spectrogram, ${scan.columns} columns of ${scan.bands} bands`}
          />
        </div>
      </div>
    </div>
    {scrollable && <div className="file-scan-rail" aria-hidden="true">
      <i style={{ width: `${thumb}%`, left: `${thumbLeft}%` }} />
    </div>}
  </div>;
}
