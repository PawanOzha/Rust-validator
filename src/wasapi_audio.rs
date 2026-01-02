#[cfg(target_os = "windows")]
pub mod wasapi {
    use windows::core::*;
    use windows::Win32::Foundation::*;
    use windows::Win32::Media::Audio::Endpoints::*;
    use windows::Win32::Media::Audio::*;
    use windows::Win32::System::Com::*;

    pub struct AudioInfo {
        pub volume: f32,
        pub is_muted: bool,
    }

    #[derive(Debug)]
    pub struct AudioAppSession {
        pub name: String,
        pub volume: f32,
        pub is_active: bool,
        pub peak_level: f32,
        pub process_id: u32,
        pub window_title: String,
    }

    pub fn get_microphone_volume_and_mute() -> Result<AudioInfo> {
        unsafe {
            // Initialize COM
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);

            // Create device enumerator
            let enumerator: IMMDeviceEnumerator =
                CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;

            // Get default audio capture device (microphone)
            let device = enumerator.GetDefaultAudioEndpoint(eCapture, eConsole)?;

            // Activate the IAudioEndpointVolume interface
            let volume_interface: IAudioEndpointVolume = device.Activate(CLSCTX_ALL, None)?;

            // Get volume level (0.0 to 1.0)
            let volume_scalar = volume_interface.GetMasterVolumeLevelScalar()?;

            // Get mute status
            let is_muted = volume_interface.GetMute()?;

            // Cleanup COM
            CoUninitialize();

            Ok(AudioInfo {
                volume: volume_scalar * 100.0, // Convert to percentage
                is_muted: is_muted.as_bool(),
            })
        }
    }

    pub fn get_microphone_device_name() -> Result<String> {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);

            let enumerator: IMMDeviceEnumerator =
                CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;

            let device = enumerator.GetDefaultAudioEndpoint(eCapture, eConsole)?;

            // Get device ID as string (simpler than getting friendly name)
            let id = device.GetId()?;
            let device_name = id.to_string()?;

            CoUninitialize();

            // Return a simplified name or ID
            if device_name.is_empty() {
                Ok("Default Microphone".to_string())
            } else {
                // Extract a readable name from the ID
                Ok("Microphone".to_string())
            }
        }
    }

    /// Get list of apps currently using the microphone
    pub fn get_apps_using_microphone() -> Result<Vec<String>> {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);

            let enumerator: IMMDeviceEnumerator =
                CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;

            let device = enumerator.GetDefaultAudioEndpoint(eCapture, eConsole)?;

            // Get the audio session manager
            let session_manager: IAudioSessionManager2 = device.Activate(CLSCTX_ALL, None)?;

            // Get the session enumerator
            let session_enum = session_manager.GetSessionEnumerator()?;
            let session_count = session_enum.GetCount()?;

            let mut apps = Vec::new();

            // Enumerate all audio sessions
            for i in 0..session_count {
                if let Ok(session) = session_enum.GetSession(i) {
                    if let Ok(session_control) = session.cast::<IAudioSessionControl2>() {
                        // Get the process ID
                        if let Ok(process_id) = session_control.GetProcessId() {
                            if process_id != 0 {
                                // Get process name
                                if let Ok(process_name) = get_process_name(process_id) {
                                    // Check if this session is actively capturing audio
                                    if let Ok(state) = session_control.GetState() {
                                        if state == AudioSessionStateActive {
                                            apps.push(process_name);
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }

            CoUninitialize();

            Ok(apps)
        }
    }

    /// Get process name from process ID
    unsafe fn get_process_name(process_id: u32) -> Result<String> {
        use windows::Win32::System::Threading::*;
        use windows::core::PWSTR;

        let process_handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, process_id)?;

        let mut buffer = vec![0u16; 260]; // MAX_PATH
        let mut size = buffer.len() as u32;

        let result = QueryFullProcessImageNameW(process_handle, PROCESS_NAME_FORMAT(0), PWSTR(buffer.as_mut_ptr()), &mut size);

        if result.is_ok() {
            let _ = CloseHandle(process_handle);
            let path = String::from_utf16_lossy(&buffer[..size as usize]);

            // Extract just the filename from the path
            if let Some(name) = path.split('\\').last() {
                return Ok(name.to_string());
            }
            return Ok(path);
        }

        let _ = CloseHandle(process_handle);
        Err(Error::from_win32())
    }

    /// Get window title for a given process ID
    /// For multi-process apps like browsers, finds any window from the same executable
    unsafe fn get_window_title_for_process(target_pid: u32) -> String {
        use windows::Win32::UI::WindowsAndMessaging::*;
        use std::sync::Mutex;

        // Store found window title in a static mutex
        static WINDOW_TITLE: Mutex<Option<String>> = Mutex::new(None);
        static PROCESS_NAME: Mutex<Option<String>> = Mutex::new(None);

        // Reset state
        *WINDOW_TITLE.lock().unwrap() = None;

        // Get the process name for fallback searching
        let _target_process_name = if let Ok(name) = get_process_name(target_pid) {
            *PROCESS_NAME.lock().unwrap() = Some(name.clone());
            name
        } else {
            *PROCESS_NAME.lock().unwrap() = None;
            String::new()
        };

        // Callback function for EnumWindows
        unsafe extern "system" fn enum_window_callback(hwnd: HWND, lparam: LPARAM) -> BOOL {
            let target_pid = lparam.0 as u32;
            let mut window_pid: u32 = 0;

            GetWindowThreadProcessId(hwnd, Some(&mut window_pid as *mut u32));

            // Check if window is visible and has text
            if IsWindowVisible(hwnd).as_bool() {
                let mut buffer = vec![0u16; 512];
                let length = GetWindowTextW(hwnd, &mut buffer);

                if length > 0 {
                    let title = String::from_utf16_lossy(&buffer[..length as usize]);
                    if !title.trim().is_empty() {
                        // Priority 1: Exact PID match
                        if window_pid == target_pid {
                            *WINDOW_TITLE.lock().unwrap() = Some(title);
                            return BOOL(0); // Stop enumeration
                        }

                        // Priority 2: Same process name (for multi-process apps like browsers)
                        if let Some(target_name) = PROCESS_NAME.lock().unwrap().as_ref() {
                            if let Ok(window_process_name) = get_process_name(window_pid) {
                                if &window_process_name == target_name {
                                    // Only save if we don't have a title yet
                                    if WINDOW_TITLE.lock().unwrap().is_none() {
                                        *WINDOW_TITLE.lock().unwrap() = Some(title);
                                    }
                                }
                            }
                        }
                    }
                }
            }

            BOOL(1) // Continue enumeration
        }

        // Enumerate all top-level windows
        let _ = EnumWindows(Some(enum_window_callback), LPARAM(target_pid as isize));

        // Return the found window title or empty string
        WINDOW_TITLE.lock().unwrap().clone().unwrap_or_default()
    }

    /// Get audio output (speakers/headphones) volume and mute status
    pub fn get_audio_output_volume_and_mute() -> Result<AudioInfo> {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);

            let enumerator: IMMDeviceEnumerator =
                CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;

            // Get default audio RENDER device (speakers/headphones)
            let device = enumerator.GetDefaultAudioEndpoint(eRender, eConsole)?;

            let volume_interface: IAudioEndpointVolume = device.Activate(CLSCTX_ALL, None)?;

            let volume_scalar = volume_interface.GetMasterVolumeLevelScalar()?;
            let is_muted = volume_interface.GetMute()?;

            CoUninitialize();

            Ok(AudioInfo {
                volume: volume_scalar * 100.0,
                is_muted: is_muted.as_bool(),
            })
        }
    }

    /// Get audio output device name
    pub fn get_audio_output_device_name() -> Result<String> {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);

            let enumerator: IMMDeviceEnumerator =
                CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;

            let device = enumerator.GetDefaultAudioEndpoint(eRender, eConsole)?;

            let id = device.GetId()?;
            let device_name = id.to_string()?;

            CoUninitialize();

            if device_name.is_empty() {
                Ok("Default Speakers".to_string())
            } else {
                Ok("Speakers".to_string())
            }
        }
    }

    /// Get current audio output peak level (0.0 to 1.0)
    pub fn get_audio_output_peak_level() -> Result<f32> {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);

            let enumerator: IMMDeviceEnumerator =
                CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;

            let device = enumerator.GetDefaultAudioEndpoint(eRender, eConsole)?;

            // Get the audio meter interface
            let meter: IAudioMeterInformation = device.Activate(CLSCTX_ALL, None)?;

            // Get current peak value
            let peak = meter.GetPeakValue()?;

            CoUninitialize();

            Ok(peak)
        }
    }

    /// Get list of apps currently playing audio
    pub fn get_apps_playing_audio() -> Result<Vec<AudioAppSession>> {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);

            let enumerator: IMMDeviceEnumerator =
                CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;

            // Get default audio RENDER device (speakers)
            let device = enumerator.GetDefaultAudioEndpoint(eRender, eConsole)?;

            let session_manager: IAudioSessionManager2 = device.Activate(CLSCTX_ALL, None)?;
            let session_enum = session_manager.GetSessionEnumerator()?;
            let session_count = session_enum.GetCount()?;

            let mut apps = Vec::new();

            for i in 0..session_count {
                if let Ok(session) = session_enum.GetSession(i) {
                    if let Ok(session_control) = session.cast::<IAudioSessionControl2>() {
                        if let Ok(process_id) = session_control.GetProcessId() {
                            if process_id != 0 {
                                if let Ok(process_name) = get_process_name(process_id) {
                                    if let Ok(state) = session_control.GetState() {
                                        let is_active = state == AudioSessionStateActive;

                                        // Get session volume
                                        let volume = if let Ok(volume_control) = session.cast::<ISimpleAudioVolume>() {
                                            if let Ok(vol) = volume_control.GetMasterVolume() {
                                                vol * 100.0
                                            } else {
                                                0.0
                                            }
                                        } else {
                                            0.0
                                        };

                                        // Get peak meter for this session
                                        let peak_level = if let Ok(meter) = session.cast::<IAudioMeterInformation>() {
                                            meter.GetPeakValue().unwrap_or(0.0)
                                        } else {
                                            0.0
                                        };

                                        // Only include if the app is actually playing audio or was recently
                                        if is_active || peak_level > 0.0 {
                                            // Get window title for this process
                                            let window_title = get_window_title_for_process(process_id);

                                            apps.push(AudioAppSession {
                                                name: process_name,
                                                volume,
                                                is_active,
                                                peak_level,
                                                process_id,
                                                window_title,
                                            });
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }

            CoUninitialize();

            Ok(apps)
        }
    }
}
