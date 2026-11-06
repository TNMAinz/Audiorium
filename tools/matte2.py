"""One-off: matte panel surfaces and flat chrome for styles.css."""

import re

path = 'src/styles.css'
css = open(path, encoding='utf-8').read().replace('\r\n', '\n')


def body_of(selector):
    match = re.search(re.escape(selector) + r'\s*\{([^}]*)\}', css)
    if not match:
        raise SystemExit('missing block: ' + selector)
    return match.group(1)


def set_body(selector, body):
    global css
    css = re.sub(re.escape(selector) + r'\s*\{[^}]*\}', selector + ' {' + body + '}', css, count=1)


def set_prop(selector, prop, value):
    body = body_of(selector)
    if re.search(r'(?<![-\w])' + re.escape(prop) + r'\s*:', body):
        body = re.sub(r'(?<![-\w])' + re.escape(prop) + r'\s*:[^;]*;', prop + ': ' + value + ';', body, count=1)
    else:
        body = body.rstrip() + ' ' + prop + ': ' + value + ';'
    set_body(selector, body)


def drop_prop(selector, prop):
    body = body_of(selector)
    set_body(selector, re.sub(r'\s*' + re.escape(prop) + r'\s*:[^;]*;', '', body))


def drop_rule(selector):
    global css
    css = re.sub(r'\n?' + re.escape(selector) + r'\s*\{[^}]*\}', '', css, count=1)


# --- panels ---------------------------------------------------------------
set_prop('.glass-panel', 'background', 'var(--panel)')
set_prop('.glass-panel', 'border', '1px solid var(--panel-line)')
set_prop('.glass-panel', 'border-radius', '8px')
drop_prop('.glass-panel', 'box-shadow')

set_prop('.sidebar', 'background', 'var(--panel)')
set_prop('.sidebar', 'border', '1px solid var(--panel-line)')
set_prop('.sidebar', 'border-radius', '8px')

set_body('.sidebar-collection', """
  position: relative; overflow: hidden; flex: 1; min-height: 0; margin: 0; container-type: size;
  padding: 12px 14px; display: flex; flex-direction: column; gap: 8px;
  border: 1px solid var(--panel-line); border-radius: 8px;
  background: var(--panel-raised);
""")
set_prop('.sidebar-collection p', 'color', '#9aa8c2')
set_prop('.sidebar-collection p', 'line-height', '1.5')
drop_rule('.sidebar-collection::before')
drop_rule('.collection-symbol')
drop_rule('.collection-symbol::after')

set_prop('.sidebar-bottom', 'background', 'var(--panel-raised)')
set_prop('.sidebar-bottom', 'border', '1px solid var(--panel-line)')
set_prop('.sidebar-bottom', 'border-radius', '8px')

set_prop('.mini-artwork', 'background', '#0f1622')
set_prop('.mini-artwork', 'border', '1px solid var(--panel-line)')
set_prop('.mini-artwork', 'color', '#a9b8cf')
drop_prop('.mini-artwork', 'box-shadow')

set_prop('.brand-mark', 'background', 'none')
set_prop('.brand-mark', 'border', '1.5px solid #cfdcff')
drop_prop('.brand-mark', 'box-shadow')

set_prop('.nav-item.active', 'background', 'rgba(124, 170, 255, .1)')
drop_prop('.nav-item.active', 'box-shadow')
set_prop('.nav-item.active::before', 'background', 'var(--accent)')
drop_prop('.nav-item.active::before', 'box-shadow')

set_prop('.instrument-stack > section.glass-panel', 'background', 'var(--panel)')
set_prop('.instrument-stack > section.glass-panel', 'border-color', 'var(--panel-line)')
drop_prop('.instrument-stack > section.glass-panel', 'backdrop-filter')
drop_rule('.instrument-stack > section.glass-panel::before')
drop_rule('.panel-heading h2::before')

set_prop('.instrument-tag', 'border-radius', '4px')
set_prop('.instrument-tag', 'border', '1px solid var(--panel-line)')
set_prop('.instrument-tag', 'background', 'none')

set_prop('.source-badge', 'border-radius', '4px')
set_prop('.source-badge', 'border', '1px solid var(--panel-line)')
set_prop('.source-badge', 'background', 'var(--panel)')

set_body('.listening-card', """
  position: absolute; bottom: 30px; left: 50%; transform: translateX(-50%);
  display: flex; align-items: center; gap: 12px;
  width: min(560px, calc(100% - 40px)); padding: 10px 14px;
  border-radius: 8px; border: 1px solid var(--panel-line); background: var(--panel);
  pointer-events: none;
""")
css = css.replace(
    '.listening-card p {',
    '.listening-card strong { display: block; margin: 0; font-weight: 500; font-size: var(--fs-body); color: #eaf0ff; }\n.listening-card p {',
    1,
)
set_prop('.listening-card p', 'font-size', 'var(--fs-sm)')
set_prop('.listening-card p', 'margin', '3px 0 0')
drop_rule('.listening-art')
drop_rule('.listening-art::before')
drop_rule('.listening-art::after')
drop_rule('.format-chip')
drop_rule('.format-note')

set_prop('.page-card', 'background', 'var(--panel)')
set_prop('.page-card', 'border', '1px solid var(--panel-line)')
set_prop('.page-card', 'border-radius', '8px')
drop_prop('.page-card', 'backdrop-filter')
drop_prop('.page-card', 'box-shadow')

set_prop('.player', 'background', 'var(--panel)')
set_prop('.player', 'border-radius', '8px')
set_prop('.player', 'border-color', 'var(--panel-line)')
drop_prop('.player', 'backdrop-filter')

set_prop('.scene-settings', 'background', '#0c111b')
set_prop('.scene-settings', 'border-color', 'var(--panel-line)')

set_prop('.transport-button.primary', 'background', 'rgba(124, 170, 255, .2)')
set_prop('.transport-button.primary', 'border-color', 'rgba(160, 200, 255, .34)')
drop_prop('.transport-button.primary', 'box-shadow')
set_prop('.icon-button', 'background', 'transparent')
set_prop('.icon-button', 'border-radius', '6px')

drop_prop('.headphone-field > svg:first-child', 'filter')

drop_rule('.mode-tabs')
drop_rule('.mode-tabs button')
drop_rule('.mode-tabs button.selected')

# No halos and no glass anywhere: a tool does not glow its own labels.
css = re.sub(r'\s*text-shadow:\s*[^;}]+;', '', css)
css = re.sub(r'\s*backdrop-filter:\s*[^;}]+;', '', css)

open(path, 'w', encoding='utf-8').write(css)
print('matte pass written')
