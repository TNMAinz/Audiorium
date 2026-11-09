#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod analysis;
mod artwork;
mod autoeq;
mod chain;
mod compressor;
mod eq;
mod export;
mod loudness;
mod saturator;
mod library;
mod limiter;
mod log;
mod panner;
mod presets;
mod resample;
mod scale;
mod reverb;
mod spatial;
mod store;
mod synth;
mod tray;
mod window;

#[cfg(windows)]
mod audio;
#[cfg(windows)]
mod capture;
#[cfg(windows)]
mod driver;
#[cfg(windows)]
mod filescan;
#[cfg(windows)]
mod midi;
#[cfg(windows)]
mod player;
#[cfg(windows)]
mod render;
#[cfg(windows)]
mod synth_out;
#[cfg(windows)]
mod spatial_out;

use std::sync::Arc;
use tauri::Manager;

fn main() {
    // A release build has no console, so this is the only place a panic can be
    // read after the window has gone.
    log::install();
    let builder = tauri::Builder::default();
    #[cfg(windows)]
    let builder = {
        let audio_state = audio::AudioState::default();
        // Playback is the only sound source, and it reads the spatial renderer, the
        // processing chain and the analysis meter through these same handles.
        let player_state = player::PlayerState::new(
            audio_state.spatial_handle(),
            audio_state.chain_handle(),
            audio_state.analysis_handle(),
        );
        // The input capture is the second source of measurements, and when the user
        // asks to hear it, it renders through the same processors playback uses.
        let capture_state = capture::CaptureState::new(
            audio_state.chain_handle(),
            audio_state.spatial_handle(),
        );
        let render_state = Arc::new(render::RenderState::new());
        // The instrument has a clock of its own and its own output stream, but
        // plays through the same rack, which is what makes it part of this app
        // rather than a second one.
        let synth_state = synth_out::SynthState::new(
            audio_state.chain_handle(),
            audio_state.spatial_handle(),
        );
        builder
            .manage(audio_state.chain_handle())
            .manage(render_state)
            .manage(synth_state)
            .manage(audio_state.analysis_handle())
            .manage(audio_state)
            .manage(capture_state)
            .manage(player_state)
            .invoke_handler(tauri::generate_handler![
                audio::list_audio_outputs,
                audio::list_audio_inputs,
                capture::start_input_capture,
                capture::stop_input_capture,
                capture::capture_status,
                driver::audiorium_device_status,
                driver::install_audiorium_device,
                audio::set_spatial,
                audio::spatial_layout,
                audio::spatial_output,
                analysis::analysis_status,
                player::playback_status,
                player::track_durations,
                player::play_track,
                player::set_playback_device,
                player::pause_playback,
                player::resume_playback,
                player::stop_playback,
                player::seek_playback,
                player::set_playback_volume,
                player::set_monitor_trim,
                player::set_monitor_direct,
                chain::chain_config,
                chain::chain_meters,
                chain::set_panner,
                chain::set_reverb,
                chain::set_limiter,
                chain::set_order,
                chain::set_stereo_mode,
                chain::set_compressor,
                chain::set_saturator,
                chain::set_eq,
                chain::eq_response,
                chain::eq_presets,
                chain::apply_eq_preset,
                chain::rename_eq_preset,
                chain::delete_eq_preset,
                chain::random_reverb_network,
                chain::panner_timbres,
                render::master_destination,
                render::render_start,
                render::render_album,
                render::render_status,
                render::render_cancel,
                render::reference_measure,
                render::null_test,
                filescan::scan_file,
                synth_out::synth_patch,
                synth_out::set_synth,
                synth_out::synth_status,
                synth_out::synth_start,
                synth_out::synth_stop,
                synth_out::synth_note_on,
                synth_out::synth_note_off,
                synth_out::synth_all_notes_off,
                synth_out::synth_bend,
                synth_out::synth_mod_wheel,
                synth_out::synth_apply_preset,
                synth_out::synth_save_preset,
                synth_out::synth_delete_preset,
                synth_out::synth_record,
                synth_out::synth_bounce,
                synth_out::synth_table_shape,
                synth_out::synth_import_table,
                synth_out::synth_choose_table,
                synth_out::synth_scales,
                synth_out::synth_import_scale,
                synth_out::synth_choose_scale,
                synth_out::synth_midi_devices,
                synth_out::synth_midi_open,
                synth_out::synth_midi_close,
                artwork::track_artwork,
                library::scan_library,
                library::choose_library_folder,
                tray::hide_to_tray,
                tray::quit_audiorium
            ])
    };
    let app = builder
        // The window is configured hidden so it can be put back where it was left
        // before the first paint, instead of opening in the middle and jumping. Setup
        // runs once the configured windows exist, which is the only point where the
        // position can be applied and the window then shown.
        .setup(|app| {
            if let Some(window) = app.get_webview_window("main") {
                window::restore(&window);
                app.manage(window::Watcher::start(&window));
                let _ = window.show();
                // The icon is up before the window can be put away, so a hidden
                // window is always one press from coming back.
                tray::install(app)?;
            }
            Ok(())
        })
        // Closing asks instead of quitting: the answer is the interface's, and the
        // commands it calls are the same ones the tray menu uses.
        .on_window_event(|window, event| {
            tray::intercept_close(window, event);
            tray::intercept_focus(window, event);
        })
        .build(tauri::generate_context!())
        .expect("error while running AUDIORIUM");
    app.run(|app, event| {
        // The drag just before quitting is the one the user means.
        if let tauri::RunEvent::Exit = event {
            if let Some(watcher) = app.try_state::<window::Watcher>() {
                watcher.flush();
            }
        }
    });
}
