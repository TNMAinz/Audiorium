import { workspaces } from '../model';
import type { ScenePart } from './observatory';

/**
 * The only text the observatory shows, and only while a part is under the pointer:
 * one line naming it, its live reading and the page behind it. It never takes the
 * pointer itself — the part under the cursor stays hovered, and pressing that part is
 * what opens the page, so the caption cannot swallow the hover that produced it.
 */
export function SceneCaption({ part }: { part: ScenePart | null }) {
  if (!part) return null;
  return <p className="scene-caption">
    <strong>{part.label}</strong>
    <span className="scene-caption-value">{part.value}</span>
    <span className="scene-caption-open">{workspaces[part.action].label} →</span>
  </p>;
}
