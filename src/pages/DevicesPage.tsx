import { RefreshCw } from 'lucide-react';
import { ScrollArea } from '../components/instrument/ScrollArea';
import type { AudioDevice, useAnalysis } from '../useAnalysis';
import { Page } from './Page';

/**
 * Where sound leaves the app and what it measures.
 *
 * The two directions are the same instrument: one list of devices each, the same
 * row, the same tags, so a device reads the same way whichever side it is on.
 * Windows' default is used on both sides until another device is chosen, and the
 * choice is remembered for the next launch.
 *
 * The input list carries two kinds of source: capture endpoints (microphones, a
 * virtual device) and the outputs, read back in loopback — what that device is
 * playing. Loopback is how another application's audio reaches the panels without
 * changing that application or installing anything.
 *
 * The Audiorium device is optional and sits beside the route it would carry: this
 * build plays files and needs no driver at all.
 */
export function DevicesPage({ audio, error = '' }: { audio: ReturnType<typeof useAnalysis>; error?: string }) {
  const playing = audio.status.playing;
  const capturing = audio.capture.active;
  // An engine that predates listening answers without it, and a missing field must
  // read as "not listening" rather than take the page down.
  const monitoring = audio.capture.monitor ?? { active: false, deviceName: '', error: null };
  // What the capture is being heard on: the output it was started with, while it
  // runs. Listening is a choice made before the stream opens.
  const monitorLabel = !audio.monitor
    ? 'Off'
    : monitoring.active
      ? monitoring.deviceName || audio.outputName || 'Windows default'
      : capturing ? 'Not playing' : 'Ready when capturing';
  return <Page
    title="Devices"
    state={`${audio.devices.length} outputs · ${audio.inputs.length} inputs · ${audio.loopbacks.length} loopbacks · ${capturing
      ? `capturing ${audio.capture.deviceName || audio.inputName || 'the Windows default input'}${audio.capture.loopback ? ' (loopback)' : ''}`
      : `playing to ${audio.outputName || 'the Windows default'}`}`}
    actions={<button className="page-button" onClick={() => void audio.refreshDevices()} aria-label="Refresh audio devices"><RefreshCw size={13} />Refresh</button>}
  >
    {/* The playback engine's message, and the enumeration's own failure when a list
        could not be read: a silent empty list would look like a machine with no devices. */}
    {(error || audio.error) && <p className="audio-error" role="alert">{error || audio.error}</p>}
    <div className="page-grid devices-grid">
      <DeviceCard
        side="output"
        devices={audio.devices}
        chosen={audio.output}
        rememberedMissing={audio.rememberedMissing}
        onChoose={audio.setOutput}
        playing={playing}
        native={audio.native}
      />
      <DeviceCard
        side="input"
        devices={[...audio.inputs, ...audio.loopbacks]}
        chosen={audio.input}
        rememberedMissing={audio.rememberedInputMissing}
        onChoose={audio.setInput}
        playing={playing}
        native={audio.native}
        capturing={capturing ? audio.capture.device : ''}
      />

      <article className="page-card">
        <div className="panel-heading"><h2>Current route</h2><span className={`panel-meta ${playing || capturing ? 'live-meta' : ''}`}>{capturing ? 'CAPTURING' : playing ? 'LIVE' : 'IDLE'}</span></div>
        <dl className="stat-list">
          <div><dt>Source</dt><dd title={capturing ? 'The chosen input is what the panels measure while the capture runs' : 'Files decoded on this machine, measured after the chain'}>{capturing ? 'Input capture' : 'Library playback'}</dd></div>
          <div><dt>Output</dt><dd title={audio.outputName || 'Windows default'}>{audio.outputName || 'Windows default'}</dd></div>
          <div><dt>Input</dt><dd title={audio.inputName || 'Windows default' || undefined}>{capturing
            ? `${audio.capture.deviceName || audio.inputName || 'Windows default'}${audio.capture.loopback ? ' (loopback)' : ''}`
            : audio.inputName || 'Windows default'}</dd></div>
          <div><dt>Monitor</dt><dd title={audio.monitor ? 'What is captured is played to this output, through the processors' : 'The capture is measured only; nothing is played'}>{monitorLabel}</dd></div>
          <div><dt>Format</dt><dd title={audio.status.format}>{audio.status.format}</dd></div>
        </dl>
        <div className="device-actions">
          <button
            className="page-button"
            onClick={() => void audio.startCapture()}
            disabled={!audio.native || audio.captureBusy || capturing}
            aria-label="Capture the chosen input"
          >
            {capturing ? 'Capturing' : audio.captureBusy ? 'Starting…' : 'Capture input'}
          </button>
          <button
            className="page-button"
            onClick={() => void audio.stopCapture()}
            disabled={!audio.native || audio.captureBusy || !capturing}
            aria-label="Stop capturing the input"
          >
            Stop capture
          </button>
          <button
            className="page-button"
            aria-pressed={audio.monitor}
            onClick={() => audio.setMonitor(!audio.monitor)}
            disabled={!audio.native || audio.captureBusy}
            aria-label="Play the captured input to the chosen output"
            title={capturing ? 'Re-opens the capture and changes what is heard' : 'Hear the capture on the output chosen above'}
          >
            {audio.monitor ? 'Listen: on' : 'Listen: off'}
          </button>
        </div>
        {audio.capture.error && <p className="audio-error" role="alert">{audio.capture.error}</p>}
        {monitoring.error && <p className="audio-error" role="alert">{monitoring.error}</p>}
        {audio.captureError && <p className="audio-error" role="alert">{audio.captureError}</p>}
      </article>

      <article className="page-card">
        <div className="panel-heading">
          <h2>Audiorium device</h2>
          <span className={`panel-meta ${audio.driver.active ? 'live-meta' : ''}`}>
            {audio.driver.active ? 'ACTIVE' : audio.driver.installed ? 'INSTALLED' : 'NOT INSTALLED'}
          </span>
        </div>
        <dl className="stat-list">
          <div><dt>Speakers</dt><dd title={audio.driver.renderName || 'Not present'}>{audio.driver.renderName || 'Not present'}</dd></div>
          <div><dt>Microphone</dt><dd title={audio.driver.captureName || 'Not present'}>{audio.driver.captureName || 'Not present'}</dd></div>
          <div><dt>State</dt><dd>{audio.driver.active ? 'Endpoints running' : audio.driver.installed ? 'Waiting for a reboot' : 'Not installed'}</dd></div>
        </dl>
        <div className="device-actions">
          <button
            className="page-button"
            onClick={() => void audio.installDriver()}
            disabled={!audio.native || audio.installing}
            aria-label="Install the Audiorium audio device"
            title="Asks for administrator rights, turns on test signing, and needs a reboot before the endpoints appear"
          >
            {audio.installing ? 'Installing…' : audio.driver.installed ? 'Reinstall device' : 'Install device'}
          </button>
          <button
            className="page-button"
            onClick={() => void audio.refreshDriver()}
            disabled={!audio.native || audio.installing}
            aria-label="Check for the Audiorium audio device again"
          >
            Check again
          </button>
        </div>
        {audio.driver.secureBoot && <p className="audio-warning" role="status">Secure Boot is on, so Windows will not load a locally signed driver. Turn it off in the firmware once, then install and reboot.</p>}
        {audio.installError && <p className="audio-error" role="alert">{audio.installError}</p>}
        {audio.installLog && <pre className="install-log" aria-label="Installer output">{audio.installLog}</pre>}
        <p className="panel-note">Optional. Adds Audiorium’s Speakers and Microphone endpoints for other apps; every device Windows already lists works exactly as it does now.</p>
      </article>
    </div>
  </Page>;
}

/** One direction's list: identical anatomy on both sides, so the page is symmetrical. */
function DeviceCard({ side, devices, chosen, rememberedMissing, onChoose, playing, native, capturing = '' }: {
  side: 'output' | 'input';
  devices: AudioDevice[];
  chosen: string;
  rememberedMissing: boolean;
  onChoose: (id: string) => void;
  playing: boolean;
  native: boolean;
  /** The endpoint a running capture actually opened, marked on its own row. */
  capturing?: string;
}) {
  const output = side === 'output';
  const heading = output ? 'Output devices' : 'Input devices';
  return <article className="page-card">
    <div className="panel-heading"><h2>{heading}</h2><span className="panel-meta">{devices.length} FOUND</span></div>
    {devices.length === 0
      ? <div className="page-empty">
        <strong>No devices listed</strong>
        <span>{native ? `Windows returned no ${side} devices.` : 'Device enumeration runs in the Windows app.'}</span>
      </div>
      : <ScrollArea className="device-list" role="list" ariaLabel={heading}>
        {devices.map(device => {
          // No choice means Windows' default, which stays the choice even if
          // Windows moves the default to another device.
          const selected = chosen ? device.id === chosen : device.isDefault;
          return <div key={device.id} role="listitem">
            <button
              type="button"
              className={`device-row ${selected ? 'chosen' : ''}`}
              aria-pressed={selected}
              aria-label={output
                ? `Use ${device.name} for playback`
                : device.loopback ? `Capture what ${device.name} is playing` : `Use ${device.name} as input`}
              title={selected
                ? `${heading.slice(0, -1)} uses ${device.name}`
                : device.loopback ? `Capture what ${device.name} is playing` : `Use ${device.name}`}
              onClick={() => {
                // A row already in use is not a request to change anything: pressing it
                // must not re-open a running capture for the endpoint it is already
                // reading. Choosing the default row still moves off another device.
                if (selected) return;
                onChoose(device.isDefault ? '' : device.id);
              }}
            >
              <span className={`status-light ${(selected && playing && output) || capturing === device.id ? 'live' : ''}`} />
              <strong>{device.name}</strong>
              <span className="device-tags">
                {device.isDefault && <em>Windows default</em>}
                {device.loopback && <em className="idle">Loopback</em>}
                {capturing === device.id && <em className="output">Capturing</em>}
                {selected ? <em className="output">{output ? 'Output' : 'Input'}</em> : <em className="idle">Available</em>}
              </span>
            </button>
          </div>;
        })}
      </ScrollArea>}
    {rememberedMissing && <p className="audio-warning" role="status">
      {output
        ? 'The output chosen last time is not connected, so this session plays to the Windows default.'
        : 'The input chosen last time is not connected, so the Windows default is used.'}
    </p>}
  </article>;
}
