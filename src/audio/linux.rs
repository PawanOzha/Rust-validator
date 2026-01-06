// Linux audio backend using PulseAudio
// This implementation provides audio monitoring for Linux systems with PulseAudio

use super::{AudioAppSession, AudioBackend, AudioInfo};
use libpulse_binding as pulse;
use libpulse_binding::callbacks::ListResult;
use libpulse_binding::context::{Context, FlagSet as ContextFlagSet};
use libpulse_binding::mainloop::threaded::Mainloop;
use libpulse_binding::proplist::Proplist;
use libpulse_binding::volume::{ChannelVolumes, Volume};
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::ops::Deref;

// Implement the AudioBackend trait for Linux
impl AudioBackend for () {
    fn get_microphone_volume_and_mute() -> std::result::Result<AudioInfo, Box<dyn std::error::Error>> {
        get_microphone_volume_and_mute_impl()
    }

    fn get_microphone_device_name() -> std::result::Result<String, Box<dyn std::error::Error>> {
        get_microphone_device_name_impl()
    }

    fn get_apps_using_microphone() -> std::result::Result<Vec<String>, Box<dyn std::error::Error>> {
        get_apps_using_microphone_impl()
    }

    fn get_audio_output_volume_and_mute() -> std::result::Result<AudioInfo, Box<dyn std::error::Error>> {
        get_audio_output_volume_and_mute_impl()
    }

    fn get_audio_output_device_name() -> std::result::Result<String, Box<dyn std::error::Error>> {
        get_audio_output_device_name_impl()
    }

    fn get_audio_output_peak_level() -> std::result::Result<f32, Box<dyn std::error::Error>> {
        get_audio_output_peak_level_impl()
    }

    fn get_apps_playing_audio() -> std::result::Result<Vec<AudioAppSession>, Box<dyn std::error::Error>> {
        get_apps_playing_audio_impl()
    }
}

// Helper function to create PulseAudio context
fn create_pulse_context() -> std::result::Result<(Mainloop, Context), Box<dyn std::error::Error>> {
    let mut proplist = Proplist::new().ok_or("Failed to create proplist")?;
    proplist.set_str(pulse::proplist::properties::APPLICATION_NAME, "rust-audio-validator")
        .map_err(|_| "Failed to set app name")?;

    let mainloop = Mainloop::new().ok_or("Failed to create mainloop")?;
    let context = Context::new_with_proplist(&mainloop, "RustAudioContext", &proplist)
        .ok_or("Failed to create context")?;

    context.connect(None, ContextFlagSet::NOFLAGS, None)
        .map_err(|e| format!("Failed to connect to PulseAudio: {:?}", e))?;

    mainloop.lock();
    mainloop.start().map_err(|e| format!("Failed to start mainloop: {:?}", e))?;

    // Wait for context to be ready
    loop {
        match context.get_state() {
            pulse::context::State::Ready => break,
            pulse::context::State::Failed | pulse::context::State::Terminated => {
                mainloop.unlock();
                return Err("PulseAudio context failed".into());
            }
            _ => {
                mainloop.unlock();
                std::thread::sleep(std::time::Duration::from_millis(10));
                mainloop.lock();
            }
        }
    }

    mainloop.unlock();
    Ok((mainloop, context))
}

// Microphone volume and mute status
fn get_microphone_volume_and_mute_impl() -> std::result::Result<AudioInfo, Box<dyn std::error::Error>> {
    let (mainloop, context) = match create_pulse_context() {
        Ok(ctx) => ctx,
        Err(_) => {
            // Graceful fallback if PulseAudio not available
            return Ok(AudioInfo {
                volume: 0.0,
                is_muted: true,
            });
        }
    };

    let result = Arc::new(Mutex::new(None));
    let result_clone = Arc::clone(&result);

    mainloop.lock();
    let introspect = context.introspect();

    introspect.get_server_info(move |server_info| {
        if let Some(default_source) = server_info.default_source_name.as_ref() {
            let result_inner = Arc::clone(&result_clone);
            let introspect_inner = context.introspect();

            introspect_inner.get_source_info_by_name(default_source, move |list_result| {
                if let ListResult::Item(source_info) = list_result {
                    let volume_avg = source_info.volume.avg().0 as f32 / Volume::NORMAL.0 as f32 * 100.0;
                    let muted = source_info.mute;

                    *result_inner.lock().unwrap() = Some(AudioInfo {
                        volume: volume_avg,
                        is_muted: muted,
                    });
                }
            });
        }
    });

    mainloop.unlock();

    // Wait for result with timeout
    for _ in 0..50 {
        std::thread::sleep(std::time::Duration::from_millis(10));
        if result.lock().unwrap().is_some() {
            break;
        }
    }

    mainloop.lock();
    mainloop.stop();
    mainloop.unlock();

    result.lock().unwrap().take().ok_or("Failed to get microphone info".into())
}

// Microphone device name
fn get_microphone_device_name_impl() -> std::result::Result<String, Box<dyn std::error::Error>> {
    let (mainloop, context) = match create_pulse_context() {
        Ok(ctx) => ctx,
        Err(_) => return Ok("Default Microphone".to_string()),
    };

    let result = Arc::new(Mutex::new(None));
    let result_clone = Arc::clone(&result);

    mainloop.lock();
    let introspect = context.introspect();

    introspect.get_server_info(move |server_info| {
        if let Some(default_source) = server_info.default_source_name.as_ref() {
            let result_inner = Arc::clone(&result_clone);
            let introspect_inner = context.introspect();

            introspect_inner.get_source_info_by_name(default_source, move |list_result| {
                if let ListResult::Item(source_info) = list_result {
                    let name = source_info.description.as_ref()
                        .map(|d| d.to_string())
                        .unwrap_or_else(|| "Default Microphone".to_string());

                    *result_inner.lock().unwrap() = Some(name);
                }
            });
        }
    });

    mainloop.unlock();

    for _ in 0..50 {
        std::thread::sleep(std::time::Duration::from_millis(10));
        if result.lock().unwrap().is_some() {
            break;
        }
    }

    mainloop.lock();
    mainloop.stop();
    mainloop.unlock();

    Ok(result.lock().unwrap().take().unwrap_or_else(|| "Default Microphone".to_string()))
}

// Get applications using microphone
fn get_apps_using_microphone_impl() -> std::result::Result<Vec<String>, Box<dyn std::error::Error>> {
    let (mainloop, context) = match create_pulse_context() {
        Ok(ctx) => ctx,
        Err(_) => return Ok(Vec::new()),
    };

    let result = Arc::new(Mutex::new(Vec::new()));
    let result_clone = Arc::clone(&result);

    mainloop.lock();
    let introspect = context.introspect();

    introspect.get_source_output_info_list(move |list_result| {
        if let ListResult::Item(output_info) = list_result {
            // Get application name from properties
            if let Some(props) = output_info.proplist.as_ref() {
                if let Some(app_name) = props.get_str(pulse::proplist::properties::APPLICATION_PROCESS_BINARY) {
                    result_clone.lock().unwrap().push(app_name);
                } else if let Some(app_name) = props.get_str(pulse::proplist::properties::APPLICATION_NAME) {
                    result_clone.lock().unwrap().push(app_name);
                }
            }
        }
    });

    mainloop.unlock();

    std::thread::sleep(std::time::Duration::from_millis(100));

    mainloop.lock();
    mainloop.stop();
    mainloop.unlock();

    Ok(result.lock().unwrap().clone())
}

// Audio output volume and mute status
fn get_audio_output_volume_and_mute_impl() -> std::result::Result<AudioInfo, Box<dyn std::error::Error>> {
    let (mainloop, context) = match create_pulse_context() {
        Ok(ctx) => ctx,
        Err(_) => {
            return Ok(AudioInfo {
                volume: 0.0,
                is_muted: true,
            });
        }
    };

    let result = Arc::new(Mutex::new(None));
    let result_clone = Arc::clone(&result);

    mainloop.lock();
    let introspect = context.introspect();

    introspect.get_server_info(move |server_info| {
        if let Some(default_sink) = server_info.default_sink_name.as_ref() {
            let result_inner = Arc::clone(&result_clone);
            let introspect_inner = context.introspect();

            introspect_inner.get_sink_info_by_name(default_sink, move |list_result| {
                if let ListResult::Item(sink_info) = list_result {
                    let volume_avg = sink_info.volume.avg().0 as f32 / Volume::NORMAL.0 as f32 * 100.0;
                    let muted = sink_info.mute;

                    *result_inner.lock().unwrap() = Some(AudioInfo {
                        volume: volume_avg,
                        is_muted: muted,
                    });
                }
            });
        }
    });

    mainloop.unlock();

    for _ in 0..50 {
        std::thread::sleep(std::time::Duration::from_millis(10));
        if result.lock().unwrap().is_some() {
            break;
        }
    }

    mainloop.lock();
    mainloop.stop();
    mainloop.unlock();

    result.lock().unwrap().take().ok_or("Failed to get audio output info".into())
}

// Audio output device name
fn get_audio_output_device_name_impl() -> std::result::Result<String, Box<dyn std::error::Error>> {
    let (mainloop, context) = match create_pulse_context() {
        Ok(ctx) => ctx,
        Err(_) => return Ok("Default Speakers".to_string()),
    };

    let result = Arc::new(Mutex::new(None));
    let result_clone = Arc::clone(&result);

    mainloop.lock();
    let introspect = context.introspect();

    introspect.get_server_info(move |server_info| {
        if let Some(default_sink) = server_info.default_sink_name.as_ref() {
            let result_inner = Arc::clone(&result_clone);
            let introspect_inner = context.introspect();

            introspect_inner.get_sink_info_by_name(default_sink, move |list_result| {
                if let ListResult::Item(sink_info) = list_result {
                    let name = sink_info.description.as_ref()
                        .map(|d| d.to_string())
                        .unwrap_or_else(|| "Default Speakers".to_string());

                    *result_inner.lock().unwrap() = Some(name);
                }
            });
        }
    });

    mainloop.unlock();

    for _ in 0..50 {
        std::thread::sleep(std::time::Duration::from_millis(10));
        if result.lock().unwrap().is_some() {
            break;
        }
    }

    mainloop.lock();
    mainloop.stop();
    mainloop.unlock();

    Ok(result.lock().unwrap().take().unwrap_or_else(|| "Default Speakers".to_string()))
}

// Audio output peak level
fn get_audio_output_peak_level_impl() -> std::result::Result<f32, Box<dyn std::error::Error>> {
    // PulseAudio doesn't provide direct peak metering in the same way as WASAPI
    // This would require subscribing to sink monitors which is more complex
    // For now, return a placeholder that indicates "possibly active"
    // A full implementation would need to use pa_stream with monitor source
    Ok(0.0)
}

// Get applications playing audio
fn get_apps_playing_audio_impl() -> std::result::Result<Vec<AudioAppSession>, Box<dyn std::error::Error>> {
    let (mainloop, context) = match create_pulse_context() {
        Ok(ctx) => ctx,
        Err(_) => return Ok(Vec::new()),
    };

    let result = Arc::new(Mutex::new(Vec::new()));
    let result_clone = Arc::clone(&result);

    mainloop.lock();
    let introspect = context.introspect();

    introspect.get_sink_input_info_list(move |list_result| {
        if let ListResult::Item(input_info) = list_result {
            let mut app_name = String::new();
            let mut process_id = 0u32;
            let mut window_title = String::new();

            if let Some(props) = input_info.proplist.as_ref() {
                // Get application name
                if let Some(name) = props.get_str(pulse::proplist::properties::APPLICATION_PROCESS_BINARY) {
                    app_name = name;
                } else if let Some(name) = props.get_str(pulse::proplist::properties::APPLICATION_NAME) {
                    app_name = name;
                }

                // Get process ID
                if let Some(pid_str) = props.get_str(pulse::proplist::properties::APPLICATION_PROCESS_ID) {
                    process_id = pid_str.parse().unwrap_or(0);
                }

                // Try to get window title (may not always be available)
                if let Some(title) = props.get_str("window.name") {
                    window_title = title;
                } else {
                    window_title = app_name.clone();
                }
            }

            let volume_avg = input_info.volume.avg().0 as f32 / Volume::NORMAL.0 as f32 * 100.0;
            let is_corked = input_info.corked;

            result_clone.lock().unwrap().push(AudioAppSession {
                name: app_name,
                volume: volume_avg,
                is_active: !is_corked,
                peak_level: 0.0,  // Would need sink monitor for accurate peak
                process_id,
                window_title,
            });
        }
    });

    mainloop.unlock();

    std::thread::sleep(std::time::Duration::from_millis(100));

    mainloop.lock();
    mainloop.stop();
    mainloop.unlock();

    Ok(result.lock().unwrap().clone())
}

// Public convenience functions
pub fn get_microphone_volume_and_mute() -> std::result::Result<AudioInfo, Box<dyn std::error::Error>> {
    get_microphone_volume_and_mute_impl()
}

pub fn get_microphone_device_name() -> std::result::Result<String, Box<dyn std::error::Error>> {
    get_microphone_device_name_impl()
}

pub fn get_apps_using_microphone() -> std::result::Result<Vec<String>, Box<dyn std::error::Error>> {
    get_apps_using_microphone_impl()
}

pub fn get_audio_output_volume_and_mute() -> std::result::Result<AudioInfo, Box<dyn std::error::Error>> {
    get_audio_output_volume_and_mute_impl()
}

pub fn get_audio_output_device_name() -> std::result::Result<String, Box<dyn std::error::Error>> {
    get_audio_output_device_name_impl()
}

pub fn get_audio_output_peak_level() -> std::result::Result<f32, Box<dyn std::error::Error>> {
    get_audio_output_peak_level_impl()
}

pub fn get_apps_playing_audio() -> std::result::Result<Vec<AudioAppSession>, Box<dyn std::error::Error>> {
    get_apps_playing_audio_impl()
}
