import type { ReactNode } from 'react';
import { ScrollArea } from '../components/instrument/ScrollArea';

interface Props {
  title: string;
  /** One line of live values under the title: mode, format, device, count. */
  state: ReactNode;
  actions?: ReactNode;
  children: ReactNode;
  className?: string;
}

export function Page({ title, state, actions, children, className = '' }: Props) {
  return <section className={`workspace-page ${className}`} aria-labelledby="workspace-title">
    <header className="page-head">
      <div className="page-heading">
        <h1 id="workspace-title">{title}</h1>
        <p className="page-state" title={typeof state === 'string' ? state : undefined}>{state}</p>
      </div>
      {actions && <div className="page-actions">{actions}</div>}
    </header>
    {/* A page only scrolls when the window is too short for it, and then it scrolls under the same
        hairline rail as every list, never under a native bar that paints over the cards. */}
    <div className="page-body"><ScrollArea className="page-body-view">{children}</ScrollArea></div>
  </section>;
}

export function formatBytes(bytes: number) {
  if (!Number.isFinite(bytes) || bytes <= 0) return '—';
  if (bytes >= 1_000_000_000) return `${(bytes / 1_000_000_000).toFixed(1)} GB`;
  if (bytes >= 1_000_000) return `${(bytes / 1_000_000).toFixed(1)} MB`;
  return `${Math.max(1, Math.round(bytes / 1_000))} KB`;
}

export function formatDate(seconds: number) {
  if (!seconds) return '—';
  return new Date(seconds * 1000).toLocaleDateString(undefined, { year: 'numeric', month: 'short', day: 'numeric' });
}
