"""One-off: matte panel surfaces and flat chrome for styles.css."""

path = 'src/styles.css'
text = open(path, encoding='utf-8').read().replace('\r\n', '\n')
pairs = []

pairs.append(("""  background:
    linear-gradient(158deg, rgba(24, 30, 62, .8), rgba(8, 11, 24, .9));
  box-shadow: inset 0 1px 0 rgba(255, 255, 255, .06), 0 26px 60px -34px rgba(0, 0, 0, .95);
}""", """  background: var(--panel);
}"""))

pairs.append(("""  background: linear-gradient(100deg, rgba(3, 5, 13, .93), rgba(3, 5, 13, .62) 54%, rgba(3, 5, 13, 0) 96%);
  border: 0;
  border-radius: 0;
}
.sidebar::before {
  content: ''; position: absolute; inset: 0 -24px 0 0; pointer-events: none;
  background: radial-gradient(120% 70% at -10% 30%, rgba(74, 96, 190, .22), transparent 70%);
}""", """  background: var(--panel);
  border: 1px solid var(--panel-line);
  border-radius: 8px;
}"""))

pairs.append(("""  background: radial-gradient(circle at 34% 28%, #eaf1ff, #8ea8ff 34%, #4453b4 62%, #0b1030 88%);
  box-shadow: 0 0 20px rgba(132, 166, 255, .55), inset -3px -4px 7px rgba(0, 0, 0, .55);
}""", """  border: 1.5px solid #cfdcff;
  box-shadow: none;
}"""))

pairs.append(("""  padding: 0 14px; font-size: var(--fs-body); color: #bcc9e5; text-align: left;
  text-shadow: 0 1px 8px rgba(3, 5, 13, .9);
}""", """  padding: 0 14px; font-size: var(--fs-body); color: #bcc9e5; text-align: left;
}"""))

pairs.append(("""  background: linear-gradient(96deg, rgba(124, 170, 255, .19), rgba(124, 170, 255, .03));
  border-color: rgba(168, 194, 255, .2);
  box-shadow: inset 0 1px 0 rgba(255, 255, 255, .07);
}""", """  background: rgba(124, 170, 255, .1);
  border-color: rgba(168, 194, 255, .2);
}"""))

pairs.append(("""  background: linear-gradient(var(--accent), var(--accent-2)); box-shadow: 0 0 12px var(--accent);
}""", """  background: var(--accent);
}"""))

pairs.append((""".sidebar-collection {
  position: relative; overflow: hidden; flex: 1; min-height: 0; margin: 0; container-type: size;
  padding: 16px 14px 15px; display: flex; flex-direction: column; align-items: center; justify-content: center; gap: 10px;
  text-align: center; border: 1px solid var(--hairline); border-radius: 16px;
  background:
    radial-gradient(96% 62% at 50% 0%, rgba(124, 170, 255, .13), transparent 62%),
    linear-gradient(158deg, rgba(17, 22, 50, .58), rgba(5, 8, 18, .7));
  box-shadow: inset 0 1px 0 rgba(255, 255, 255, .05);
}
.sidebar-collection::before {
  content: ''; position: absolute; inset: 0; pointer-events: none; opacity: .55;
  background-image:
    radial-gradient(1px 1px at 24% 17%, #ffffffb0, transparent 55%),
    radial-gradient(1px 1px at 70% 30%, #cfe0ff99, transparent 55%),
    radial-gradient(1.2px 1.2px at 38% 68%, #ffffffa0, transparent 55%),
    radial-gradient(1px 1px at 82% 82%, #ffe6cc99, transparent 55%);
}
.sidebar-collection > * { position: relative; }
.collection-symbol {
  position: relative; width: 58px; height: 58px; display: grid; place-items: center; flex: none;
  margin: 0; border-radius: 50%; color: #0c1730;
  background: radial-gradient(circle at 34% 28%, #f2f7ff, #a7c3ff 34%, #4a5fc0 66%, #101a3a 92%);
  box-shadow: 0 0 30px -8px rgba(124, 192, 255, .75), inset -4px -5px 10px rgba(0, 0, 0, .45);
}
.collection-symbol::after {
  content: ''; position: absolute; inset: -7px -12px; border: 1px solid rgba(186, 206, 255, .34); border-radius: 50%; transform: rotate(-20deg) scaleY(.42);
}
.sidebar-collection p { color: #aab7d4; font-size: var(--fs-sm); line-height: 1.65; margin: 0; text-shadow: 0 1px 10px rgba(3, 5, 13, .9); }
.sidebar-collection .nav-heading { padding: 0; margin: 0; }""", """.sidebar-collection {
  position: relative; overflow: hidden; flex: 1; min-height: 0; margin: 0; container-type: size;
  padding: 12px 14px; display: flex; flex-direction: column; gap: 8px;
  border: 1px solid var(--panel-line); border-radius: 8px;
  background: var(--panel-raised);
}
.sidebar-collection p { color: #9aa8c2; font-size: var(--fs-sm); line-height: 1.5; margin: 0; }
.sidebar-collection .nav-heading { padding: 0; margin: 0; }"""))

pairs.append(("""  border: 1px solid var(--hairline); border-radius: 14px;
  background: linear-gradient(140deg, rgba(17, 22, 50, .52), rgba(5, 8, 18, .64));
  box-shadow: inset 0 1px 0 rgba(255, 255, 255, .05);
}""", """  border: 1px solid var(--panel-line); border-radius: 8px;
  background: var(--panel-raised);
}"""))

pairs.append(("""  border: 1px solid rgba(150, 180, 255, .16); display: grid; place-items: center; color: #d8e6ff;
  background: radial-gradient(circle at 72% 26%, rgba(255, 190, 130, .5), transparent 42%), linear-gradient(158deg, #2a3f78, #14203f 52%, #070c1a);
  box-shadow: inset 0 1px 0 rgba(255, 255, 255, .12);
}""", """  border: 1px solid var(--panel-line); display: grid; place-items: center; color: #a9b8cf;
  background: #0f1622;
}"""))

pairs.append((""".mode-tabs {
  display: flex; align-items: center; gap: 2px; padding: 5px;
  border: 1px solid rgba(150, 175, 255, .12); border-radius: 999px;
  background: linear-gradient(120deg, rgba(26, 32, 64, .72), rgba(8, 11, 24, .78));
  box-shadow: inset 0 1px 0 rgba(255, 255, 255, .07), 0 14px 34px -22px #000;
}
.mode-tabs button { position: relative; padding: 8px 21px; border: 0; border-radius: 999px; background: transparent; color: #94a2c4; font-size: var(--fs-sm); letter-spacing: .01em; }
.mode-tabs button.selected {
  color: #0a101f; font-weight: 600;
  background: linear-gradient(140deg, #eaf2ff, #bcd6ff 52%, #c8b9ff);
  box-shadow: 0 8px 22px -12px rgba(150, 195, 255, .9), inset 0 1px 0 #fff;
}
""", ""))

pairs.append(("""  font-size: var(--fs-micro); letter-spacing: .01em; color: #a9b8d4;
  padding: 6px 10px; border-radius: 999px; border: 1px solid rgba(150, 175, 235, .14);
  background: rgba(8, 12, 26, .6);
}""", """  font-size: var(--fs-micro); letter-spacing: .01em; color: #a9b8d4;
  padding: 5px 9px; border-radius: 4px; border: 1px solid var(--panel-line);
  background: var(--panel);
}"""))

pairs.append((""".listening-card {
  position: absolute; bottom: 34px; left: 50%; transform: translateX(-50%);
  display: flex; align-items: center; gap: 16px;
  width: min(408px, calc(100% - 40px)); padding: 11px;
  border-color: rgba(170, 195, 255, .2);
  background: linear-gradient(125deg, rgba(32, 40, 78, .82), rgba(8, 11, 24, .92));
  box-shadow: 0 30px 70px -34px #000, inset 0 1px 0 rgba(255, 255, 255, .08);
  pointer-events: none;
}
.listening-art {
  position: relative; width: 82px; height: 94px; flex: none; border-radius: 10px; overflow: hidden;
  display: grid; place-items: center;
  background: radial-gradient(circle at 50% 42%, #1a2549, #080d1e 72%);
  box-shadow: inset 0 0 0 1px rgba(150, 180, 255, .16), 0 10px 24px -14px #000;
}
.listening-art::before {
  content: ''; width: 44%; aspect-ratio: 1; border-radius: 50%;
  background: radial-gradient(circle at 34% 30%, #f2f7ff, #8fbcff 38%, #4a5cc0 68%, #0a1030 92%);
  box-shadow: 0 0 22px rgba(124, 192, 255, .6);
}
.listening-art::after {
  content: ''; position: absolute; width: 74%; height: 30%; border: 1px solid rgba(196, 214, 255, .42);
  border-radius: 50%; transform: rotate(-22deg);
}
.listening-card h2 { margin: 6px 0; font-weight: 400; font-size: var(--fs-title); letter-spacing: -.015em; color: #f2f6ff; }
.listening-card p { font-size: var(--fs-sm); line-height: 1.55; color: #a8b6d2; margin: 0 0 10px; }
.format-chip { font-size: var(--fs-micro); letter-spacing: .01em; border: 1px solid rgba(180, 200, 255, .22); padding: 4px 6px; border-radius: 5px; color: #d5ddf3; }
.format-note { font-size: var(--fs-micro); margin-left: 7px; color: #93a1be; }""", """.listening-card {
  position: absolute; bottom: 30px; left: 50%; transform: translateX(-50%);
  display: flex; align-items: center; gap: 12px;
  width: min(560px, calc(100% - 40px)); padding: 10px 14px;
  border-radius: 8px;
  pointer-events: none;
}
.listening-card strong { display: block; margin: 0; font-weight: 500; font-size: var(--fs-body); color: #eaf0ff; }
.listening-card p { font-size: var(--fs-sm); line-height: 1.4; color: #93a2bc; margin: 3px 0 0; }"""))

pairs.append(("""  background:
    radial-gradient(130% 90% at 100% 0%, rgba(124, 170, 255, .12), transparent 58%),
    linear-gradient(158deg, rgba(17, 22, 50, .62), rgba(5, 8, 18, .72));
  backdrop-filter: blur(16px) saturate(125%);
  border-color: rgba(146, 170, 235, .16);
}
.instrument-stack > section.glass-panel::before {
  content: ''; position: absolute; top: 0; left: 16px; right: 32%; height: 1px;
  background: linear-gradient(90deg, var(--accent), transparent); opacity: .5;
}""", """  background: var(--panel);
  border-color: var(--panel-line);
}"""))

pairs.append((".panel-heading h2::before { content: ''; display: inline-block; width: 4px; height: 4px; border-radius: 50%; margin-right: 8px; vertical-align: 2px; background: var(--accent); box-shadow: 0 0 8px var(--accent); opacity: .8; }\n", ""))

pairs.append(("""  font-size: var(--fs-micro); letter-spacing: .01em; padding: 4px 7px; border-radius: 999px;
  border: 1px solid rgba(140, 185, 255, .18); background: rgba(120, 170, 255, .08); color: #a9c4ec; white-space: nowrap;
}""", """  font-size: var(--fs-micro); letter-spacing: .01em; padding: 3px 6px; border-radius: 4px;
  border: 1px solid var(--panel-line); color: #9fb0cc; white-space: nowrap;
}"""))

pairs.append(("  max-width: 68ch; font-size: var(--fs-sm); line-height: 1.5; color: #b3c2dc; text-shadow: 0 1px 10px rgba(3, 5, 13, .95); }",
              "  max-width: 68ch; font-size: var(--fs-sm); line-height: 1.5; color: #b3c2dc; }"))

pairs.append(("  border-radius: 14px; border: 1px solid var(--hairline); background: linear-gradient(158deg, rgba(13, 18, 42, .9), rgba(4, 7, 16, .94)); backdrop-filter: blur(18px) saturate(120%); box-shadow: 0 24px 54px -38px #000; }",
              "  border-radius: 8px; border: 1px solid var(--panel-line); background: var(--panel); }"))

pairs.append(("  background: linear-gradient(160deg, #101633, #070a16); border-color: rgba(160, 190, 255, .2); }",
              "  background: #0c111b; border-color: var(--panel-line); }"))
pairs.append((".scene-settings::backdrop { background: #01030bb8; backdrop-filter: blur(3px); }",
              ".scene-settings::backdrop { background: #01030bd0; }"))

pairs.append(("  z-index: 3; border-radius: 18px; border-color: rgba(150, 175, 255, .12); background: linear-gradient(115deg, rgba(13, 18, 40, .74), rgba(5, 8, 18, .84)); backdrop-filter: blur(18px) saturate(120%); }",
              "  z-index: 3; border-radius: 8px; border-color: var(--panel-line); background: var(--panel); }"))

pairs.append(("  border: 1px solid transparent; background: rgba(150, 190, 255, .06); color: #c2d4f0; }",
              "  border: 1px solid var(--panel-line); background: transparent; color: #c2d4f0; }"))
pairs.append(("  border-radius: 50%; border-color: rgba(160, 200, 255, .3); background: linear-gradient(140deg, rgba(160, 200, 255, .28), rgba(124, 170, 255, .08)); color: #f2f8ff; box-shadow: 0 6px 18px -12px rgba(150, 200, 255, .9); }",
              "  border-radius: 50%; border-color: rgba(160, 200, 255, .34); background: rgba(124, 170, 255, .2); color: #f2f8ff; }"))

pairs.append(("  fill: none; stroke: rgba(146, 194, 255, .55); stroke-width: .7; filter: drop-shadow(0 0 6px rgba(124, 192, 255, .35)); }",
              "  fill: none; stroke: rgba(146, 194, 255, .55); stroke-width: .7; }"))

pairs.append(("  border-radius: 9px; background: rgba(20, 26, 52, .5); color: #b4c3e0; }",
              "  border-radius: 6px; background: transparent; color: #b4c3e0; }"))

for old, new in pairs:
    if old not in text:
        raise SystemExit('MISSING: ' + old[:80])
    text = text.replace(old, new, 1)

# No text halos anywhere: a tool does not glow its own labels.
import re
text = re.sub(r'\s*text-shadow:\s*[^;}]+;', '', text)

open(path, 'w', encoding='utf-8').write(text)
print('applied', len(pairs), 'surface edits')
