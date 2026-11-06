//! Windows Spatial Sound output for the Atmos mode.
//!
//! The app derives a 7.1.4 bed from its stereo mix and hands the twelve mono
//! feeds to `ISpatialAudioClient` as static objects. Windows then renders them
//! with the spatial format the user selected for the output — Dolby Atmos for
//! Headphones or Home Theater when that is the chosen format, Windows Sonic
//! otherwise. No Dolby decoder ships here: the bitstream formats are not touched,
//! and the bed is still derived from two channels.
//!
//! `GetMaxDynamicObjectCount` is the documented on/off signal: it is 0 when
//! spatial sound is not enabled for the endpoint. A zero count, a missing 7.1.4
//! static bed, or an activation failure all make `open` return an error so the
//! player can fall back to the app's own derived renderer instead of playing
//! silence.

use std::array;
use std::mem::ManuallyDrop;
use std::ptr;

use serde::Serialize;
use windows::core::HSTRING;
use windows::Win32::Foundation::{CloseHandle, HANDLE, WAIT_OBJECT_0};
use windows::Win32::Media::Audio::{
    AudioCategory_Media, AudioObjectType, AudioObjectType_BackLeft, AudioObjectType_BackRight,
    AudioObjectType_FrontCenter, AudioObjectType_FrontLeft, AudioObjectType_FrontRight,
    AudioObjectType_LowFrequency, AudioObjectType_None, AudioObjectType_SideLeft,
    AudioObjectType_SideRight, AudioObjectType_TopBackLeft, AudioObjectType_TopBackRight,
    AudioObjectType_TopFrontLeft, AudioObjectType_TopFrontRight,
    IMMDevice, IMMDeviceEnumerator, ISpatialAudioClient, ISpatialAudioObject,
    ISpatialAudioObjectRenderStream, MMDeviceEnumerator,
    SpatialAudioObjectRenderStreamActivationParams, WAVEFORMATEX, eMultimedia, eRender,
};
use windows::Win32::System::Com::StructuredStorage::{
    PROPVARIANT, PROPVARIANT_0, PROPVARIANT_0_0, PROPVARIANT_0_0_0,
};
use windows::Win32::System::Com::{
    BLOB, CLSCTX_ALL, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance,
    CoInitializeEx,
};
use windows::Win32::System::Threading::{CreateEventW, WaitForSingleObject};
use windows::Win32::System::Variant::VT_BLOB;

use crate::spatial::BED_CHANNELS;

/// The static channels of a 7.1.4 bed, in the same order as `spatial::Bed`:
/// front left, front right, centre, LFE, side left, side right, back left,
/// back right, top front left, top front right, top back left, top back right.
pub const BED_TYPES: [AudioObjectType; BED_CHANNELS] = [
    AudioObjectType_FrontLeft,
    AudioObjectType_FrontRight,
    AudioObjectType_FrontCenter,
    AudioObjectType_LowFrequency,
    AudioObjectType_SideLeft,
    AudioObjectType_SideRight,
    AudioObjectType_BackLeft,
    AudioObjectType_BackRight,
    AudioObjectType_TopFrontLeft,
    AudioObjectType_TopFrontRight,
    AudioObjectType_TopBackLeft,
    AudioObjectType_TopBackRight,
];

/// What the endpoint can do right now, for the interface.
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SpatialOutput {
    enabled: bool,
    max_objects: u32,
    reason: String,
}

impl SpatialOutput {
    pub fn unavailable(reason: impl Into<String>) -> Self {
        Self::off(reason)
    }

    fn off(reason: impl Into<String>) -> Self {
        Self {
            enabled: false,
            max_objects: 0,
            reason: reason.into(),
        }
    }
}

fn bed_mask() -> AudioObjectType {
    BED_TYPES
        .iter()
        .fold(AudioObjectType_None, |mask, kind| mask | *kind)
}

fn endpoint(device_id: &str) -> Result<IMMDevice, String> {
    unsafe {
        // S_FALSE (already initialised on this thread) is fine; the player's
        // output thread has already initialized MTA for WASAPI.
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        let enumerator: IMMDeviceEnumerator =
            CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)
                .map_err(|error| format!("Cannot open the audio device list: {error}"))?;
        if device_id.is_empty() {
            enumerator
                .GetDefaultAudioEndpoint(eRender, eMultimedia)
                .map_err(|error| format!("No playback output is available: {error}"))
        } else {
            enumerator
                .GetDevice(&HSTRING::from(device_id))
                .map_err(|error| format!("The selected playback output is unavailable: {error}"))
        }
    }
}

fn object_format() -> WAVEFORMATEX {
    WAVEFORMATEX {
        // WAVE_FORMAT_IEEE_FLOAT, the format every spatial object buffer uses.
        wFormatTag: 3,
        nChannels: 1,
        nSamplesPerSec: 48_000,
        nAvgBytesPerSec: 48_000 * 4,
        nBlockAlign: 4,
        wBitsPerSample: 32,
        cbSize: 0,
    }
}

unsafe fn client(device: &IMMDevice) -> Result<ISpatialAudioClient, String> {
    unsafe {
        device
            .Activate::<ISpatialAudioClient>(CLSCTX_INPROC_SERVER, None)
            .map_err(|error| format!("Windows spatial sound is not available for this output: {error}"))
    }
}

unsafe fn capability(device: &IMMDevice) -> Result<(u32, AudioObjectType), String> {
    unsafe {
        let client = client(device)?;
        let format = object_format();
        client
            .IsAudioObjectFormatSupported(&format)
            .map_err(|error| format!("Windows spatial sound does not accept 48 kHz mono float objects: {error}"))?;
        let max = client
            .GetMaxDynamicObjectCount()
            .map_err(|error| format!("Cannot read the spatial object budget: {error}"))?;
        let native = client
            .GetNativeStaticObjectTypeMask()
            .map_err(|error| format!("Cannot read the spatial static-channel mask: {error}"))?;
        Ok((max, native))
    }
}

/// Reads whether spatial sound is enabled for the output and which static bed
/// the current format supports. Never fails: the reason travels with the answer.
pub fn probe(device_id: &str) -> SpatialOutput {
    let result = endpoint(device_id).and_then(|device| unsafe { capability(&device) });
    match result {
        Ok((0, _native)) => SpatialOutput::off(
            "Spatial sound is off for this output. In Windows Settings → System → Sound, open this output's Spatial audio and choose Dolby Atmos for Headphones, then start the track again.",
        ),
        Ok((_, native)) if !native.contains(bed_mask()) => SpatialOutput::off(
            "The selected spatial format does not expose a 7.1.4 static bed. In Windows Settings → System → Sound → Spatial audio, choose Dolby Atmos for Headphones.",
        ),
        Ok((max, _native)) => SpatialOutput {
            enabled: true,
            max_objects: max,
            reason: String::new(),
        },
        Err(reason) => SpatialOutput::off(reason),
    }
}

/// An open spatial stream. It lives on the playback thread: every interface is
/// `!Send`, and the render loop is the only code that touches it.
pub struct SpatialSink {
    stream: ISpatialAudioObjectRenderStream,
    event: HANDLE,
    objects: [Option<ISpatialAudioObject>; BED_CHANNELS],
}

impl SpatialSink {
    /// Opens the spatial stream for the 7.1.4 bed. The stream is not started;
    /// `start` does that once the player is ready to feed it.
    pub fn open(device_id: &str) -> Result<Self, String> {
        let device = endpoint(device_id)?;
        unsafe {
            let (max, native) = capability(&device)?;
            if max == 0 {
                return Err(
                    "Spatial sound is off for this output; choose Dolby Atmos for Headphones in Windows Settings → System → Sound → Spatial audio."
                        .into(),
                );
            }
            if !native.contains(bed_mask()) {
                return Err(
                    "The selected spatial format does not expose a 7.1.4 static bed; choose Dolby Atmos for Headphones in Windows Settings → System → Sound → Spatial audio."
                        .into(),
                );
            }
            let client = client(&device)?;
            let format = object_format();
            let event = CreateEventW(None, false, false, None)
                .map_err(|error| format!("Cannot create the spatial buffer event: {error}"))?;
            let params = SpatialAudioObjectRenderStreamActivationParams {
                ObjectFormat: &format,
                StaticObjectTypeMask: bed_mask(),
                MinDynamicObjectCount: 0,
                MaxDynamicObjectCount: 0,
                Category: AudioCategory_Media,
                EventHandle: event,
                NotifyObject: ManuallyDrop::new(None),
            };
            let activation = PROPVARIANT {
                Anonymous: PROPVARIANT_0 {
                    Anonymous: ManuallyDrop::new(PROPVARIANT_0_0 {
                        vt: VT_BLOB,
                        wReserved1: 0,
                        wReserved2: 0,
                        wReserved3: 0,
                        Anonymous: PROPVARIANT_0_0_0 {
                            blob: BLOB {
                                cbSize: std::mem::size_of::<
                                    SpatialAudioObjectRenderStreamActivationParams,
                                >() as u32,
                                pBlobData: (&params as *const SpatialAudioObjectRenderStreamActivationParams)
                                    as *mut u8,
                            },
                        },
                    }),
                },
            };
            match client.ActivateSpatialAudioStream::<ISpatialAudioObjectRenderStream>(&activation) {
                Ok(stream) => Ok(Self {
                    stream,
                    event,
                    objects: array::from_fn(|_| None),
                }),
                Err(error) => {
                    let _ = CloseHandle(event);
                    Err(format!("Cannot open the Windows spatial stream: {error}"))
                }
            }
        }
    }

    pub fn start(&mut self) -> Result<(), String> {
        unsafe {
            self.stream
                .Start()
                .map_err(|error| format!("Cannot start the Windows spatial stream: {error}"))?;
        }
        Ok(())
    }

    /// Waits up to 100 ms for the platform's next buffer. A timeout means the
    /// endpoint or format changed; the caller resets the stream and tries again.
    pub fn wait(&self) -> bool {
        unsafe { WaitForSingleObject(self.event, 100) == WAIT_OBJECT_0 }
    }

    pub fn reset(&mut self) -> Result<(), String> {
        // Reset frees the platform's buffers, so the static objects must be
        // activated again on the next pass.
        for slot in &mut self.objects {
            *slot = None;
        }
        unsafe {
            self.stream
                .Reset()
                .map_err(|error| format!("Cannot reset the Windows spatial stream: {error}"))
        }
    }

    /// Begins a buffer and returns its frame count. Static objects are created
    /// on the first pass, as the Windows samples do.
    pub fn begin(&mut self) -> Result<usize, String> {
        unsafe {
            let mut available = 0u32;
            let mut frames = 0u32;
            self.stream
                .BeginUpdatingAudioObjects(&mut available, &mut frames)
                .map_err(|error| format!("Cannot begin a spatial buffer: {error}"))?;
            for (slot, kind) in BED_TYPES.iter().enumerate() {
                if self.objects[slot].is_none() {
                    match self.stream.ActivateSpatialAudioObject(*kind) {
                        Ok(object) => self.objects[slot] = Some(object),
                        Err(error) => {
                            let _ = self.stream.EndUpdatingAudioObjects();
                            return Err(format!(
                                "Cannot activate a spatial channel ({}): {error}",
                                channel_name(slot)
                            ));
                        }
                    }
                }
            }
            Ok(frames as usize)
        }
    }

    /// Writes one buffer of bed feeds; `feeds[i][channel]` is the sample for
    /// frame `i` of static channel `channel`.
    pub fn write(&mut self, feeds: &[[f32; BED_CHANNELS]]) -> Result<(), String> {
        unsafe {
            for (slot, object) in self.objects.iter().enumerate() {
                let Some(object) = object else {
                    continue;
                };
                let mut buffer: *mut u8 = ptr::null_mut();
                let mut length = 0u32;
                if let Err(error) = object.GetBuffer(&mut buffer, &mut length) {
                    let _ = self.stream.EndUpdatingAudioObjects();
                    return Err(format!(
                        "Cannot get the spatial buffer for {}: {error}",
                        channel_name(slot)
                    ));
                }
                let count = (length as usize / std::mem::size_of::<f32>()).min(feeds.len());
                if count == 0 {
                    continue;
                }
                let samples = std::slice::from_raw_parts_mut(buffer as *mut f32, count);
                for (index, sample) in samples.iter_mut().enumerate() {
                    *sample = feeds[index][slot].clamp(-1.0, 1.0);
                }
                // A short buffer is not expected from the platform; writing the
                // samples it gave us is safer than ending the static channel.
            }
            Ok(())
        }
    }

    pub fn end(&mut self) -> Result<(), String> {
        unsafe {
            self.stream
                .EndUpdatingAudioObjects()
                .map_err(|error| format!("Cannot finish a spatial buffer: {error}"))
        }
    }
}

impl Drop for SpatialSink {
    fn drop(&mut self) {
        // Release every object before resetting the stream, then stop it.
        for slot in &mut self.objects {
            *slot = None;
        }
        unsafe {
            let _ = self.stream.Stop();
            let _ = self.stream.Reset();
            let _ = CloseHandle(self.event);
        }
    }
}

fn channel_name(slot: usize) -> &'static str {
    const NAMES: [&str; BED_CHANNELS] = [
        "front left",
        "front right",
        "centre",
        "LFE",
        "side left",
        "side right",
        "back left",
        "back right",
        "top front left",
        "top front right",
        "top back left",
        "top back right",
    ];
    NAMES.get(slot).copied().unwrap_or("spatial channel")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spatial::{self, SpatialConfig, SpatialMode};

    #[test]
    fn the_static_mask_is_a_7_1_4_bed() {
        // 2 + 4 + 8 + 16 + 32 + 64 + 128 + 256 + 512 + 1024 + 2048 + 4096.
        assert_eq!(bed_mask().0, 0x1FFE);
        assert_eq!(bed_mask().0.count_ones(), BED_CHANNELS as u32);
        assert!(bed_mask().contains(AudioObjectType_LowFrequency));
        assert!(bed_mask().contains(AudioObjectType_TopBackRight));
    }

    #[test]
    fn the_static_channel_order_matches_the_bed() {
        let layout = spatial::describe(SpatialConfig {
            mode: SpatialMode::Atmos,
            ..SpatialConfig::default()
        });
        let expected = [
            (-30.0, 0.0),
            (30.0, 0.0),
            (0.0, 0.0),
            (0.0, -20.0),
            (-90.0, 0.0),
            (90.0, 0.0),
            (-150.0, 0.0),
            (150.0, 0.0),
            (-45.0, 45.0),
            (45.0, 45.0),
            (-135.0, 45.0),
            (135.0, 45.0),
        ];
        assert_eq!(layout.len(), BED_TYPES.len());
        for (speaker, (azimuth, elevation)) in layout.iter().zip(expected) {
            assert!((speaker.azimuth - azimuth).abs() < 1e-4);
            assert!((speaker.elevation - elevation).abs() < 1e-4);
        }
    }

    #[test]
    fn the_probe_never_panics_on_this_machine() {
        // On a machine without spatial sound enabled this reports the reason;
        // on one with it, the object budget and an enabled answer. Either way
        // it must not fail the caller.
        let output = probe("");
        println!(
            "spatial probe: enabled={} max={} reason={}",
            output.enabled, output.max_objects, output.reason
        );
        assert!(output.max_objects > 0 || !output.reason.is_empty());
    }
}
